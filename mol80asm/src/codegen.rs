//! Two-pass code generation: AST -> 8080 machine code.
//!
//! Strategy: lower the AST into a flat list of [`Emit`] units. Each unit has a known size, so
//! pass 1 assigns addresses to labels by summing sizes, and pass 2 resolves label references
//! (now that all addresses are known) and produces the final byte image. Forward references
//! therefore work (Req 3.3).
//!
//! The structured constructs are lowered here:
//!   * `Fx=v? /target`      -> conditional JMP to target.
//!   * `cond ? ( body )`    -> compare + INVERTED conditional branch to an internal label
//!                             placed after the body, so the body runs only when cond holds.

use crate::ast::*;
use crate::diag::{Diagnostic, Diagnostics};
use crate::isa::table::PCHL;
use crate::isa::{
    cmp_reg, cond_call, cond_jump, cond_return, encode_alu, encode_dad, encode_dcx, encode_exchange,
    encode_inx, encode_move, pop_opcode, push_opcode, CALL, CPI, JMP, RET,
};
use crate::symbols::SymbolTable;
use crate::token::{FlagKind, Reg, Span};

/// Origin address for assembly (default 0). A future ORG directive will set this.
const DEFAULT_ORG: u16 = 0;

/// A lowered emission unit.
#[derive(Debug, Clone)]
enum Emit {
    /// Literal bytes, known at lowering time.
    Bytes(Vec<u8>),
    /// A 3-byte control instruction (opcode + 16-bit address) whose address is a label.
    /// Resolved in pass 2.
    AddrRef { opcode: u8, label: String, span: Span },
    /// Marks where an internal/user label resolves to the current address. Zero bytes.
    LabelDef(String),
    /// `ORG addr` — set the location counter. Forward moves zero-fill; backward is an error.
    Org { addr: u16, span: Span },
    /// `DS count` — reserve `count` zero bytes.
    Space(u16),
}

impl Emit {
    /// Size in bytes for the simple linear case. `Org` is handled specially by the passes
    /// (it repositions the counter rather than adding a fixed size), so it reports 0 here.
    fn size(&self) -> u16 {
        match self {
            Emit::Bytes(b) => b.len() as u16,
            Emit::AddrRef { .. } => 3,
            Emit::LabelDef(_) => 0,
            Emit::Org { .. } => 0,
            Emit::Space(n) => *n,
        }
    }
}

/// A relocation record: a 16-bit address in the output that is relative to the module's origin
/// and must be adjusted when a linker places the module at a different base.
///
/// This is linker groundwork. The exact MOL80 object-module format is unknown ([VERIFY]), but
/// *any* relocatable scheme needs this information, so emitting it now is not a guess. Only
/// addresses derived from **in-module labels** are relocatable; addresses written as literal
/// constants by the programmer are absolute and get no record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    /// Byte offset within `Assembly.bytes` of the low byte of the 16-bit address to patch.
    pub offset: u16,
    /// Width in bytes of the address field (always 2 for the current 8080 forms).
    pub width: u8,
    /// The label the address was resolved from (for debugging / listings).
    pub symbol: String,
}

/// Output of a successful assembly.
pub struct Assembly {
    pub origin: u16,
    pub bytes: Vec<u8>,
    /// Relocation records for the linker: positions of origin-relative 16-bit addresses.
    pub relocations: Vec<Relocation>,
}

/// Assemble a parsed program into bytes, collecting diagnostics. Returns `None` if errors
/// occurred (no partial binary is returned as if complete — Req 6.4).
pub fn assemble(program: &Program, diags: &mut Diagnostics) -> Option<Assembly> {
    let mut symbols = SymbolTable::new();
    let mut emits: Vec<Emit> = Vec::new();

    // --- Lowering: AST -> flat Emit list, allocating internal labels as needed. ---
    for line in &program.lines {
        lower_line(line, &mut emits, &mut symbols, diags);
    }
    if diags.has_errors() {
        return None;
    }

    // --- Pass 1: assign addresses to every LabelDef, honoring ORG. ---
    // The origin is the address of the first emitted byte: if the program leads with an ORG,
    // that sets it; otherwise DEFAULT_ORG. We track the current address; ORG repositions it.
    let mut origin: Option<u16> = None;
    let mut addr = DEFAULT_ORG;
    for e in &emits {
        match e {
            Emit::Org { addr: target, span } => {
                if let Some(o) = origin {
                    // ORG after output has begun: only forward moves are supported (zero-fill).
                    if *target < addr {
                        diags.push(Diagnostic::error(
                            format!("ORG {:#06X} moves backward (current {:#06X})", target, addr),
                            *span,
                        ));
                    }
                    let _ = o;
                    addr = *target;
                } else {
                    // Leading ORG sets the origin.
                    origin = Some(*target);
                    addr = *target;
                }
            }
            Emit::LabelDef(name) => {
                if origin.is_none() {
                    origin = Some(DEFAULT_ORG);
                }
                symbols.define(name, addr, Span::new(0, 0, 0), diags);
            }
            _ => {
                if origin.is_none() {
                    origin = Some(DEFAULT_ORG);
                }
                addr = addr.wrapping_add(e.size());
            }
        }
    }
    if diags.has_errors() {
        return None;
    }
    let origin = origin.unwrap_or(DEFAULT_ORG);

    // --- Pass 2: resolve references, emit bytes, and record relocations. ---
    // `bytes` is the image starting at `origin`; ORG forward-fills with zeros so offsets stay
    // consistent with the addresses assigned in pass 1.
    let mut bytes = Vec::new();
    let mut relocations = Vec::new();
    for e in &emits {
        match e {
            Emit::Bytes(b) => bytes.extend_from_slice(b),
            Emit::Space(n) => bytes.extend(std::iter::repeat(0u8).take(*n as usize)),
            Emit::Org { addr: target, .. } => {
                // Zero-fill from the current output position up to the ORG target.
                let cur = origin as usize + bytes.len();
                let tgt = *target as usize;
                if tgt > cur {
                    bytes.extend(std::iter::repeat(0u8).take(tgt - cur));
                }
                // (backward ORG already reported as an error in pass 1)
            }
            Emit::LabelDef(_) => {}
            Emit::AddrRef { opcode, label, span } => {
                match symbols.get(label) {
                    Some(a) => {
                        bytes.push(*opcode);
                        // The 16-bit address starts here; this position is origin-relative, so
                        // record a relocation for the linker before writing the bytes.
                        let addr_offset = bytes.len() as u16;
                        bytes.push((a & 0xFF) as u8); // little-endian low byte
                        bytes.push((a >> 8) as u8); // high byte
                        relocations.push(Relocation {
                            offset: addr_offset,
                            width: 2,
                            symbol: label.clone(),
                        });
                    }
                    None => {
                        diags.push(
                            Diagnostic::error(
                                format!("undefined label '{}'", label),
                                *span,
                            )
                            .with_snippet(label.clone()),
                        );
                    }
                }
            }
        }
    }
    if diags.has_errors() {
        return None;
    }

    Some(Assembly {
        origin,
        bytes,
        relocations,
    })
}

fn lower_line(
    line: &Line,
    emits: &mut Vec<Emit>,
    symbols: &mut SymbolTable,
    diags: &mut Diagnostics,
) {
    if let Some(label) = &line.label {
        emits.push(Emit::LabelDef(label.clone()));
    }
    lower_item(&line.item, line.span, emits, symbols, diags);
}

fn lower_item(
    item: &Item,
    span: Span,
    emits: &mut Vec<Emit>,
    symbols: &mut SymbolTable,
    diags: &mut Diagnostics,
) {
    match item {
        Item::Empty => {}

        // `SYMBOL:rp` — load a 16-bit immediate (a label address) into a register pair via LXI.
        // Lower to an AddrRef so codegen resolves the symbol (and records a relocation), rather
        // than the encoder's placeholder-0 path.
        Item::Move { src: Operand::Symbol(name), dst: Operand::Reg(rp) } if lxi_opcode(*rp).is_some() => {
            emits.push(Emit::AddrRef {
                opcode: lxi_opcode(*rp).unwrap(),
                label: name.clone(),
                span,
            });
        }

        Item::Move { src, dst } => match encode_move(src, dst) {
            Ok(b) => emits.push(Emit::Bytes(b)),
            Err(e) => diags.push(Diagnostic::error(format!("{:?}", e), span)),
        },

        Item::AluOp { op, lhs, rhs, dst } => match encode_alu(*op, lhs, rhs, dst) {
            Ok(b) => emits.push(Emit::Bytes(b)),
            Err(e) => diags.push(Diagnostic::error(format!("{:?}", e), span)),
        },

        Item::Jump { target } => lower_jump(JMP, target, span, emits, diags),

        Item::CondJump { flag, value, target } => {
            let info = cond_jump(*flag, *value);
            lower_jump(info.opcode, target, span, emits, diags);
        }

        Item::CondBlock { cond, body } => {
            lower_cond_block(cond, body, span, emits, symbols, diags);
        }

        // `CALL(0)`..`CALL(7)` are RST 0..7: one byte 0xC7 + 8n, calling address 8n.
        // [CONFIRMED from scan] So `CALL(n)` with n < 8 is a restart, not a call to address n.
        Item::Call { target: JumpTarget::Addr(n @ 0..=7) } => {
            emits.push(Emit::Bytes(vec![0xC7 + 8 * (*n as u8)]));
        }

        Item::Call { target } => lower_jump(CALL, target, span, emits, diags),

        // `Fx=v? CALL(s)`. As with plain CALL, a literal 0..7 means restart n (address 8n);
        // there is no conditional RST, so that becomes a conditional call to 8n.
        Item::CondCall { flag, value, target } => {
            let op = cond_call(*flag, *value).opcode;
            let target = match target {
                JumpTarget::Addr(n @ 0..=7) => JumpTarget::Addr(n * 8),
                t => t.clone(),
            };
            lower_jump(op, &target, span, emits, diags);
        }

        Item::Return => emits.push(Emit::Bytes(vec![RET])),

        Item::CompareJump { rhs, target } => {
            match compare_bytes(&Operand::Reg(Reg::A), rhs) {
                Ok(b) => {
                    emits.push(Emit::Bytes(b));
                    lower_jump(cond_jump(FlagKind::Z, 1).opcode, target, span, emits, diags);
                }
                Err(m) => diags.push(Diagnostic::error(m, span)),
            }
        }

        Item::CompareReturn { rhs } => match compare_bytes(&Operand::Reg(Reg::A), rhs) {
            Ok(b) => {
                emits.push(Emit::Bytes(b));
                emits.push(Emit::Bytes(vec![cond_return(FlagKind::Z, 1).opcode]));
            }
            Err(m) => diags.push(Diagnostic::error(m, span)),
        },

        Item::CondReturn { flag, value } => {
            let info = cond_return(*flag, *value);
            emits.push(Emit::Bytes(vec![info.opcode]));
        }

        Item::Push { rp } => match push_opcode(*rp) {
            Some(op) => emits.push(Emit::Bytes(vec![op])),
            None => diags.push(Diagnostic::error(
                format!("cannot PUSH {:?} (expected BC/DE/HL/PSW)", rp),
                span,
            )),
        },

        Item::Pop { rp } => match pop_opcode(*rp) {
            Some(op) => emits.push(Emit::Bytes(vec![op])),
            None => diags.push(Diagnostic::error(
                format!("cannot POP {:?} (expected BC/DE/HL/PSW)", rp),
                span,
            )),
        },

        Item::JumpHL => emits.push(Emit::Bytes(vec![PCHL])),

        Item::Exchange { first, second } => emit_or_err(encode_exchange(*first, *second), span, emits, diags),

        Item::IncPair { rp } => emit_or_err(encode_inx(*rp), span, emits, diags),

        Item::DecPair { rp } => emit_or_err(encode_dcx(*rp), span, emits, diags),

        Item::AddPair { rp } => emit_or_err(encode_dad(*rp), span, emits, diags),

        // --- directives ---
        Item::Org { addr } => emits.push(Emit::Org { addr: *addr, span }),

        Item::DefineBytes { items } => {
            let mut bytes = Vec::new();
            for it in items {
                match it {
                    crate::ast::DataItem::Byte(b) => bytes.push(*b),
                    crate::ast::DataItem::Str(s) => bytes.extend_from_slice(s.as_bytes()),
                }
            }
            emits.push(Emit::Bytes(bytes));
        }

        Item::DefineWords { values } => {
            let mut bytes = Vec::new();
            for v in values {
                bytes.push((v & 0xFF) as u8); // little-endian
                bytes.push((v >> 8) as u8);
            }
            emits.push(Emit::Bytes(bytes));
        }

        Item::DefineSpace { count } => emits.push(Emit::Space(*count)),

        Item::Fixed { bytes, .. } => emits.push(Emit::Bytes(bytes.clone())),

        Item::CarryAlu { subtract, rhs } => {
            let (reg_base, imm_op) = if *subtract { (0x98u8, 0xDEu8) } else { (0x88, 0xCE) };
            match rhs {
                // ADC A (143) and SBB A (159) are never used in MOL80. [CONFIRMED]
                Operand::Reg(Reg::A) => diags.push(Diagnostic::error(
                    if *subtract {
                        "`A-FC-A:A` (SBB A, opcode 159) is not a MOL80 instruction"
                    } else {
                        "`A+FC+A:A` (ADC A, opcode 143) is not a MOL80 instruction"
                    },
                    span,
                )),
                Operand::Reg(r) => match crate::isa::table::reg_index(*r) {
                    Some(i) => emits.push(Emit::Bytes(vec![reg_base + i as u8])),
                    None => diags.push(Diagnostic::error(
                        format!("cannot use {:?} in carry arithmetic", r),
                        span,
                    )),
                },
                Operand::Imm(n) if (-128..=255).contains(n) => {
                    emits.push(Emit::Bytes(vec![imm_op, (*n & 0xFF) as u8]))
                }
                Operand::Imm(n) => diags.push(Diagnostic::error(
                    format!("value {} does not fit in a byte", n),
                    span,
                )),
                other => diags.push(Diagnostic::error(
                    format!("carry arithmetic needs a register or number, got {:?}", other),
                    span,
                )),
            }
        }
    }
}

/// The LXI opcode for a register pair (B=0x01, D=0x11, H=0x21, SP=0x31), or None for non-pairs.
fn lxi_opcode(rp: Reg) -> Option<u8> {
    match rp {
        Reg::BC => Some(0x01),
        Reg::DE => Some(0x11),
        Reg::HL => Some(0x21),
        Reg::SP => Some(0x31),
        _ => None,
    }
}

/// Push encoded bytes, or record the encode error as a diagnostic.
fn emit_or_err(
    res: Result<Vec<u8>, crate::isa::EncodeError>,
    span: Span,
    emits: &mut Vec<Emit>,
    diags: &mut Diagnostics,
) {
    match res {
        Ok(b) => emits.push(Emit::Bytes(b)),
        Err(e) => diags.push(Diagnostic::error(format!("{:?}", e), span)),
    }
}

fn lower_jump(
    opcode: u8,
    target: &JumpTarget,
    span: Span,
    emits: &mut Vec<Emit>,
    _diags: &mut Diagnostics,
) {
    match target {
        JumpTarget::Label(name) => emits.push(Emit::AddrRef {
            opcode,
            label: name.clone(),
            span,
        }),
        JumpTarget::Addr(a) => {
            let v = *a as u16;
            emits.push(Emit::Bytes(vec![opcode, (v & 0xFF) as u8, (v >> 8) as u8]));
        }
    }
}

/// Lower `cond ? ( body )`:
///   <compare for cond>
///   <conditional branch to @end when cond is FALSE>   (inverted sense)
///   <body>
///   @end:
fn lower_cond_block(
    cond: &Condition,
    body: &[Line],
    span: Span,
    emits: &mut Vec<Emit>,
    symbols: &mut SymbolTable,
    diags: &mut Diagnostics,
) {
    let end_label = symbols.fresh_internal();

    match cond {
        // Flag test: branch past the body when the flag does NOT hold (invert the value).
        Condition::Flag { flag, value } => {
            let inverted = if *value == 1 { 0 } else { 1 };
            let info = cond_jump(*flag, inverted);
            emits.push(Emit::AddrRef {
                opcode: info.opcode,
                label: end_label.clone(),
                span,
            });
        }
        // Register comparison, currently only equality `A=r`: CMP r, then JNZ @end
        // (skip the body when A != r). Other comparison operators await confirmation.
        Condition::Compare { lhs, rhs } => {
            lower_compare_eq(lhs, rhs, &end_label, span, emits, diags);
        }
    }

    // Body.
    for line in body {
        lower_line(line, emits, symbols, diags);
    }

    // End marker.
    emits.push(Emit::LabelDef(end_label));
}

/// Bytes for `A = rhs`: `CMP r` for a register, `CPI n` (opcode 254) for an immediate.
/// The comparison leaves Z set when equal.
fn compare_bytes(lhs: &Operand, rhs: &Operand) -> Result<Vec<u8>, String> {
    if !matches!(lhs, Operand::Reg(Reg::A)) {
        return Err("only 'A = <register|number>' comparisons are supported so far".to_string());
    }
    match rhs {
        Operand::Reg(r) => cmp_reg(*r)
            .map(|op| vec![op])
            .ok_or_else(|| format!("cannot compare against {:?}", r)),
        Operand::Imm(n) if (0..=255).contains(n) => Ok(vec![CPI, *n as u8]),
        Operand::Imm(n) => Err(format!("comparison value {} out of range (0..=255)", n)),
        _ => Err("only 'A = <register|number>' comparisons are supported so far".to_string()),
    }
}

/// Lower an equality comparison `A = rhs` and branch to `end_label` when NOT equal (JNZ), so a
/// following block body runs only when equal.
fn lower_compare_eq(
    lhs: &Operand,
    rhs: &Operand,
    end_label: &str,
    span: Span,
    emits: &mut Vec<Emit>,
    diags: &mut Diagnostics,
) {
    match compare_bytes(lhs, rhs) {
        Ok(b) => {
            emits.push(Emit::Bytes(b));
            let info = cond_jump(FlagKind::Z, 0); // JNZ
            emits.push(Emit::AddrRef {
                opcode: info.opcode,
                label: end_label.to_string(),
                span,
            });
        }
        Err(m) => diags.push(Diagnostic::error(m, span)),
    }
}