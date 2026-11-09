//! A pure, hardware-agnostic Intel 8080 CPU emulator core.
//!
//! The [`Cpu`] holds all 8080 state and executes one instruction per [`Cpu::step`] call,
//! reaching memory and I/O only through a [`Bus`]. It has no knowledge of the CPT machine's
//! banking or devices — that lives in a separate crate that implements `Bus`.
//!
//! Correctness is validated by the exhaustive CP/M diagnostic programs (see `tests/`).

pub mod bus;
pub mod flags;

pub use bus::{Bus, FlatMemory};
pub use flags::Flags;

/// The 8080 register file and processor state.
#[derive(Debug, Clone)]
pub struct Cpu {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub flags: Flags,
    /// Interrupt-enable flip-flop (set by EI, after the instruction that follows it; cleared
    /// by DI and on interrupt accept).
    pub int_enabled: bool,
    /// True after a HLT, until an interrupt wakes the CPU.
    pub halted: bool,
    /// Total cycles executed (for timing).
    pub cycles: u64,
    /// An EI has executed and the instruction after it has not finished. The 8080 enables
    /// interrupts only once that next instruction completes, so `EI; RET` at the end of an
    /// interrupt handler can't be interrupted between the two.
    ei_pending: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Cpu {
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            sp: 0,
            pc: 0,
            flags: Flags::default(),
            int_enabled: false,
            halted: false,
            cycles: 0,
            ei_pending: false,
        }
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- 16-bit register-pair accessors ----

    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }
    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }
    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }
    pub fn set_bc(&mut self, v: u16) {
        let [hi, lo] = v.to_be_bytes();
        self.b = hi;
        self.c = lo;
    }
    pub fn set_de(&mut self, v: u16) {
        let [hi, lo] = v.to_be_bytes();
        self.d = hi;
        self.e = lo;
    }
    pub fn set_hl(&mut self, v: u16) {
        let [hi, lo] = v.to_be_bytes();
        self.h = hi;
        self.l = lo;
    }

    // ---- fetch helpers ----

    fn fetch8(&mut self, bus: &mut dyn Bus) -> u8 {
        let b = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        b
    }

    fn fetch16(&mut self, bus: &mut dyn Bus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }

    // ---- memory-at-HL (the "M" operand) ----

    fn read_m(&mut self, bus: &mut dyn Bus) -> u8 {
        bus.read(self.hl())
    }
    fn write_m(&mut self, bus: &mut dyn Bus, v: u8) {
        bus.write(self.hl(), v);
    }

    // ---- stack ----

    fn push16(&mut self, bus: &mut dyn Bus, v: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (v >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, v as u8);
    }
    fn pop16(&mut self, bus: &mut dyn Bus) -> u16 {
        let lo = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    /// Request an interrupt, supplying the RST vector number (0..=7) the device forced onto the
    /// bus. Honored only if interrupts are enabled; wakes the CPU from HLT. Returns true if the
    /// interrupt was accepted.
    pub fn interrupt(&mut self, rst_vector: u8, bus: &mut dyn Bus) -> bool {
        if !self.int_enabled {
            return false;
        }
        self.int_enabled = false;
        self.halted = false;
        let pc = self.pc;
        self.push16(bus, pc);
        self.pc = (rst_vector as u16 & 0x7) * 8;
        self.cycles += 11;
        true
    }

    // ---- ALU helpers (set flags per 8080 rules) ----

    /// ADD/ADC core: A = A + value + carry_in, updating all flags.
    fn add(&mut self, value: u8, carry_in: bool) {
        let a = self.a;
        let cin = carry_in as u16;
        let result = a as u16 + value as u16 + cin;
        let r = result as u8;
        self.flags.carry = result > 0xFF;
        self.flags.aux_carry = ((a & 0x0F) + (value & 0x0F) + cin as u8) > 0x0F;
        self.flags.set_szp(r);
        self.a = r;
    }

    /// SUB/SBB core: A = A - value - borrow_in, updating all flags.
    fn sub(&mut self, value: u8, borrow_in: bool) {
        let a = self.a;
        let bin = borrow_in as u16;
        let result = (a as u16).wrapping_sub(value as u16).wrapping_sub(bin);
        let r = result as u8;
        self.flags.carry = (a as u16) < (value as u16 + bin);
        // Aux carry (half borrow): set when no borrow from bit 4, per 8080 convention.
        self.flags.aux_carry = ((a & 0x0F) as i16 - (value & 0x0F) as i16 - bin as i16) >= 0;
        self.flags.set_szp(r);
        self.a = r;
    }

    /// CMP: like SUB but discards the result (only flags change).
    fn cmp(&mut self, value: u8) {
        let saved = self.a;
        self.sub(value, false);
        self.a = saved;
    }

    fn ana(&mut self, value: u8) {
        // 8080 ANA sets AC from the OR of bit-3 of the operands (a documented quirk).
        self.flags.aux_carry = ((self.a | value) & 0x08) != 0;
        self.a &= value;
        self.flags.carry = false;
        self.flags.set_szp(self.a);
    }
    fn ora(&mut self, value: u8) {
        self.a |= value;
        self.flags.carry = false;
        self.flags.aux_carry = false;
        self.flags.set_szp(self.a);
    }
    fn xra(&mut self, value: u8) {
        self.a ^= value;
        self.flags.carry = false;
        self.flags.aux_carry = false;
        self.flags.set_szp(self.a);
    }

    fn inr(&mut self, value: u8) -> u8 {
        let r = value.wrapping_add(1);
        self.flags.aux_carry = (value & 0x0F) + 1 > 0x0F;
        self.flags.set_szp(r);
        r
    }
    fn dcr(&mut self, value: u8) -> u8 {
        let r = value.wrapping_sub(1);
        // Aux carry: set when no borrow from bit 4.
        self.flags.aux_carry = (value & 0x0F) != 0;
        self.flags.set_szp(r);
        r
    }

    /// DAD: HL = HL + value (16-bit), setting only carry.
    fn dad(&mut self, value: u16) {
        let result = self.hl() as u32 + value as u32;
        self.flags.carry = result > 0xFFFF;
        self.set_hl(result as u16);
    }

    /// DAA: decimal-adjust accumulator. The fiddly one.
    fn daa(&mut self) {
        let mut add = 0u8;
        let mut carry = self.flags.carry;
        let lo = self.a & 0x0F;
        if self.flags.aux_carry || lo > 9 {
            add |= 0x06;
        }
        if self.flags.carry || self.a > 0x99 || (self.a > 0x8F && lo > 9) {
            add |= 0x60;
            carry = true;
        }
        // aux-carry out depends on the low-nibble addition.
        self.flags.aux_carry = (lo + (add & 0x0F)) > 0x0F;
        self.a = self.a.wrapping_add(add);
        self.flags.carry = carry;
        self.flags.set_szp(self.a);
    }

    // ---- conditionals ----

    fn cond(&self, code: u8) -> bool {
        // code is the 3-bit condition field (NZ,Z,NC,C,PO,PE,P,M).
        match code {
            0 => !self.flags.zero,  // NZ
            1 => self.flags.zero,   // Z
            2 => !self.flags.carry, // NC
            3 => self.flags.carry,  // C
            4 => !self.flags.parity, // PO (parity odd)
            5 => self.flags.parity,  // PE (parity even)
            6 => !self.flags.sign,   // P (plus)
            7 => self.flags.sign,    // M (minus)
            _ => unreachable!(),
        }
    }

    // ---- 8-bit register access by the 3-bit field (B,C,D,E,H,L,M,A) ----

    fn get_reg(&mut self, bus: &mut dyn Bus, idx: u8) -> u8 {
        match idx {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => self.read_m(bus),
            7 => self.a,
            _ => unreachable!(),
        }
    }
    fn set_reg(&mut self, bus: &mut dyn Bus, idx: u8, v: u8) {
        match idx {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            6 => self.write_m(bus, v),
            7 => self.a = v,
            _ => unreachable!(),
        }
    }

    /// Execute a single instruction, returning the number of machine cycles it took.
    pub fn step(&mut self, bus: &mut dyn Bus) -> u32 {
        // An EI just before this instruction takes effect when this instruction finishes.
        let enable_after = std::mem::take(&mut self.ei_pending);
        if self.halted {
            // A halted CPU consumes cycles doing nothing until an interrupt.
            self.cycles += 4;
            if enable_after {
                self.int_enabled = true;
            }
            return 4;
        }
        let op = self.fetch8(bus);
        let cyc = self.execute(bus, op);
        self.cycles += cyc as u64;
        // (EI followed by DI leaves interrupts off.)
        if enable_after && op != 0xF3 {
            self.int_enabled = true;
        }
        cyc
    }

    #[allow(clippy::too_many_lines)]
    fn execute(&mut self, bus: &mut dyn Bus, op: u8) -> u32 {
        match op {
            // NOP and the undocumented NOP aliases.
            0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => 4,

            // --- MOV r, r (0x40..=0x7F except 0x76 = HLT) ---
            0x40..=0x75 | 0x77..=0x7F => {
                let dst = (op >> 3) & 0x07;
                let src = op & 0x07;
                let v = self.get_reg(bus, src);
                self.set_reg(bus, dst, v);
                if dst == 6 || src == 6 {
                    7
                } else {
                    5
                }
            }

            // HLT
            0x76 => {
                self.halted = true;
                7
            }

            // --- MVI r, d8 ---
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
                let dst = (op >> 3) & 0x07;
                let imm = self.fetch8(bus);
                self.set_reg(bus, dst, imm);
                if dst == 6 {
                    10
                } else {
                    7
                }
            }

            // --- LXI rp, d16 ---
            0x01 => {
                let v = self.fetch16(bus);
                self.set_bc(v);
                10
            }
            0x11 => {
                let v = self.fetch16(bus);
                self.set_de(v);
                10
            }
            0x21 => {
                let v = self.fetch16(bus);
                self.set_hl(v);
                10
            }
            0x31 => {
                self.sp = self.fetch16(bus);
                10
            }

            // --- LDAX / STAX ---
            0x0A => {
                self.a = bus.read(self.bc());
                7
            }
            0x1A => {
                self.a = bus.read(self.de());
                7
            }
            0x02 => {
                bus.write(self.bc(), self.a);
                7
            }
            0x12 => {
                bus.write(self.de(), self.a);
                7
            }

            // --- LDA / STA a16 ---
            0x3A => {
                let addr = self.fetch16(bus);
                self.a = bus.read(addr);
                13
            }
            0x32 => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.a);
                13
            }

            // --- LHLD / SHLD a16 ---
            0x2A => {
                let addr = self.fetch16(bus);
                self.l = bus.read(addr);
                self.h = bus.read(addr.wrapping_add(1));
                16
            }
            0x22 => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.l);
                bus.write(addr.wrapping_add(1), self.h);
                16
            }

            // --- INX / DCX rp ---
            0x03 => {
                self.set_bc(self.bc().wrapping_add(1));
                5
            }
            0x13 => {
                self.set_de(self.de().wrapping_add(1));
                5
            }
            0x23 => {
                self.set_hl(self.hl().wrapping_add(1));
                5
            }
            0x33 => {
                self.sp = self.sp.wrapping_add(1);
                5
            }
            0x0B => {
                self.set_bc(self.bc().wrapping_sub(1));
                5
            }
            0x1B => {
                self.set_de(self.de().wrapping_sub(1));
                5
            }
            0x2B => {
                self.set_hl(self.hl().wrapping_sub(1));
                5
            }
            0x3B => {
                self.sp = self.sp.wrapping_sub(1);
                5
            }

            // --- DAD rp ---
            0x09 => {
                self.dad(self.bc());
                10
            }
            0x19 => {
                self.dad(self.de());
                10
            }
            0x29 => {
                self.dad(self.hl());
                10
            }
            0x39 => {
                self.dad(self.sp);
                10
            }

            // --- INR / DCR r ---
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                let r = (op >> 3) & 0x07;
                let v = self.get_reg(bus, r);
                let nv = self.inr(v);
                self.set_reg(bus, r, nv);
                if r == 6 {
                    10
                } else {
                    5
                }
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                let r = (op >> 3) & 0x07;
                let v = self.get_reg(bus, r);
                let nv = self.dcr(v);
                self.set_reg(bus, r, nv);
                if r == 6 {
                    10
                } else {
                    5
                }
            }

            // --- rotates ---
            0x07 => {
                // RLC
                let hi = self.a >> 7;
                self.a = (self.a << 1) | hi;
                self.flags.carry = hi != 0;
                4
            }
            0x0F => {
                // RRC
                let lo = self.a & 1;
                self.a = (self.a >> 1) | (lo << 7);
                self.flags.carry = lo != 0;
                4
            }
            0x17 => {
                // RAL
                let hi = self.a >> 7;
                self.a = (self.a << 1) | (self.flags.carry as u8);
                self.flags.carry = hi != 0;
                4
            }
            0x1F => {
                // RAR
                let lo = self.a & 1;
                self.a = (self.a >> 1) | ((self.flags.carry as u8) << 7);
                self.flags.carry = lo != 0;
                4
            }

            // --- misc accumulator/flag ops ---
            0x27 => {
                self.daa();
                4
            }
            0x2F => {
                // CMA
                self.a = !self.a;
                4
            }
            0x37 => {
                // STC
                self.flags.carry = true;
                4
            }
            0x3F => {
                // CMC
                self.flags.carry = !self.flags.carry;
                4
            }

            // --- ALU A, r (0x80..=0xBF) ---
            0x80..=0xBF => {
                let r = op & 0x07;
                let v = self.get_reg(bus, r);
                match (op >> 3) & 0x07 {
                    0 => self.add(v, false),              // ADD
                    1 => self.add(v, self.flags.carry),   // ADC
                    2 => self.sub(v, false),              // SUB
                    3 => self.sub(v, self.flags.carry),   // SBB
                    4 => self.ana(v),                     // ANA
                    5 => self.xra(v),                     // XRA
                    6 => self.ora(v),                     // ORA
                    7 => self.cmp(v),                     // CMP
                    _ => unreachable!(),
                }
                if r == 6 {
                    7
                } else {
                    4
                }
            }

            // --- immediate ALU ---
            0xC6 => {
                let v = self.fetch8(bus);
                self.add(v, false);
                7
            } // ADI
            0xCE => {
                let v = self.fetch8(bus);
                self.add(v, self.flags.carry);
                7
            } // ACI
            0xD6 => {
                let v = self.fetch8(bus);
                self.sub(v, false);
                7
            } // SUI
            0xDE => {
                let v = self.fetch8(bus);
                self.sub(v, self.flags.carry);
                7
            } // SBI
            0xE6 => {
                let v = self.fetch8(bus);
                self.ana(v);
                7
            } // ANI
            0xEE => {
                let v = self.fetch8(bus);
                self.xra(v);
                7
            } // XRI
            0xF6 => {
                let v = self.fetch8(bus);
                self.ora(v);
                7
            } // ORI
            0xFE => {
                let v = self.fetch8(bus);
                self.cmp(v);
                7
            } // CPI

            // --- jumps ---
            0xC3 | 0xCB => {
                // JMP (0xCB is an undocumented alias)
                self.pc = self.fetch16(bus);
                10
            }
            0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => {
                let addr = self.fetch16(bus);
                if self.cond((op >> 3) & 0x07) {
                    self.pc = addr;
                }
                10
            }
            0xE9 => {
                // PCHL
                self.pc = self.hl();
                5
            }

            // --- calls ---
            0xCD | 0xDD | 0xED | 0xFD => {
                // CALL (0xDD/0xED/0xFD undocumented aliases)
                let addr = self.fetch16(bus);
                let ret = self.pc;
                self.push16(bus, ret);
                self.pc = addr;
                17
            }
            0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => {
                let addr = self.fetch16(bus);
                if self.cond((op >> 3) & 0x07) {
                    let ret = self.pc;
                    self.push16(bus, ret);
                    self.pc = addr;
                    17
                } else {
                    11
                }
            }

            // --- returns ---
            0xC9 | 0xD9 => {
                // RET (0xD9 undocumented alias)
                self.pc = self.pop16(bus);
                10
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => {
                if self.cond((op >> 3) & 0x07) {
                    self.pc = self.pop16(bus);
                    11
                } else {
                    5
                }
            }

            // --- RST n ---
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                let n = (op >> 3) & 0x07;
                let ret = self.pc;
                self.push16(bus, ret);
                self.pc = (n as u16) * 8;
                11
            }

            // --- PUSH / POP rp ---
            0xC5 => {
                self.push16(bus, self.bc());
                11
            }
            0xD5 => {
                self.push16(bus, self.de());
                11
            }
            0xE5 => {
                self.push16(bus, self.hl());
                11
            }
            0xF5 => {
                let psw = ((self.a as u16) << 8) | self.flags.to_psw() as u16;
                self.push16(bus, psw);
                11
            }
            0xC1 => {
                let v = self.pop16(bus);
                self.set_bc(v);
                10
            }
            0xD1 => {
                let v = self.pop16(bus);
                self.set_de(v);
                10
            }
            0xE1 => {
                let v = self.pop16(bus);
                self.set_hl(v);
                10
            }
            0xF1 => {
                let v = self.pop16(bus);
                self.a = (v >> 8) as u8;
                self.flags = Flags::from_psw(v as u8);
                10
            }

            // --- XTHL / XCHG / SPHL ---
            0xE3 => {
                // XTHL: exchange HL with the word on top of the stack.
                let sp = self.sp;
                let lo = bus.read(sp);
                let hi = bus.read(sp.wrapping_add(1));
                bus.write(sp, self.l);
                bus.write(sp.wrapping_add(1), self.h);
                self.l = lo;
                self.h = hi;
                18
            }
            0xEB => {
                // XCHG: swap DE and HL.
                std::mem::swap(&mut self.d, &mut self.h);
                std::mem::swap(&mut self.e, &mut self.l);
                5
            }
            0xF9 => {
                // SPHL
                self.sp = self.hl();
                5
            }

            // --- I/O ---
            0xDB => {
                let port = self.fetch8(bus);
                self.a = bus.io_in(port);
                10
            }
            0xD3 => {
                let port = self.fetch8(bus);
                bus.io_out(port, self.a);
                10
            }

            // --- interrupt control ---
            0xF3 => {
                self.int_enabled = false;
                4
            } // DI
            0xFB => {
                // Takes effect after the next instruction (see `step`).
                self.ei_pending = true;
                4
            } // EI
        }
    }
}
