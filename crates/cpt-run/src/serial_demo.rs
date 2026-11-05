//! A tiny hand-assembled 8080 program that writes a sign-on string out the serial port, then
//! echoes anything it receives. Used to prove the serial path end to end (emulator -> TCP ->
//! VT100 terminal) before we invest in a MOL80 BIOS.
//!
//! This is raw 8080 (not MOL80) on purpose: it needs IN/OUT and string data, whose MOL80 syntax
//! is still [VERIFY]. Increment 1 just proves the wiring; the MOL80 BIOS comes later.
//!
//! Serial ports (MachineConfig defaults): DATA = 0x70, STATUS = 0x71.
//! STATUS bits: bit0 = RX ready, bit1 = TX ready.

const SERIAL_DATA: u8 = 0x70;
const SERIAL_STATUS: u8 = 0x71;
const ST_RX_READY: u8 = 0x01;
const ST_TX_READY: u8 = 0x02;

/// Build the serial sign-on + echo program, loaded at 0x0000.
///
/// Layout:
///   0000: LXI H, msg            ; HL -> message
///   print_loop:
///         MOV A,M               ; A = *HL
///         ORA A ; JZ echo       ; 0 terminator -> go to echo loop
///         CALL putc             ; send A
///         INX H ; JMP print_loop
///   echo:
///         CALL getc             ; A = received byte (blocks via poll)
///         CALL putc             ; send it back
///         JMP echo
///   putc: ; wait TX ready, then OUT DATA
///         push psw
///   putc_wait: IN STATUS ; ANI TX_READY ; JZ putc_wait
///         pop psw ; OUT DATA ; RET
///   getc: ; wait RX ready, then IN DATA
///         IN STATUS ; ANI RX_READY ; JZ getc ; IN DATA ; RET
///   msg:  "CPT serial demo online.\r\n" , 0
pub fn serial_demo() -> Vec<u8> {
    // We assemble by tracking absolute addresses (program loads at 0x0000).
    let mut c: Vec<u8> = Vec::new();

    // Reserve: we need addresses for several labels. Build in two passes would be cleaner, but
    // the layout is small and fixed, so compute addresses by emitting in order and recording.
    // To keep it simple and correct, we lay out with known fixed offsets using placeholders and
    // patch jump targets after.

    // Helper closures can't easily mutate `c` and read addresses, so emit linearly and record
    // label addresses as we go, backpatching forward references at the end.
    let mut fixups: Vec<(usize, Label)> = Vec::new();
    let mut labels = Labels::default();

    // 0000: LXI H, msg   (target backpatched)
    c.push(0x21);
    fixups.push((c.len(), Label::Msg));
    c.push(0);
    c.push(0);

    labels.print_loop = c.len();
    c.push(0x7E); // MOV A,M
    c.push(0xB7); // ORA A
    c.push(0xCA); // JZ echo
    fixups.push((c.len(), Label::Echo));
    c.push(0);
    c.push(0);
    c.push(0xCD); // CALL putc
    fixups.push((c.len(), Label::Putc));
    c.push(0);
    c.push(0);
    c.push(0x23); // INX H
    c.push(0xC3); // JMP print_loop
    push_addr(&mut c, labels.print_loop as u16);

    labels.echo = c.len();
    c.push(0xCD); // CALL getc
    fixups.push((c.len(), Label::Getc));
    c.push(0);
    c.push(0);
    c.push(0xCD); // CALL putc
    fixups.push((c.len(), Label::Putc));
    c.push(0);
    c.push(0);
    c.push(0xC3); // JMP echo
    fixups.push((c.len(), Label::Echo));
    c.push(0);
    c.push(0);

    // putc: send the byte in A (preserve A across the status poll).
    labels.putc = c.len();
    c.push(0xF5); // PUSH PSW
    let putc_wait = c.len();
    c.push(0xDB); // IN STATUS
    c.push(SERIAL_STATUS);
    c.push(0xE6); // ANI TX_READY
    c.push(ST_TX_READY);
    c.push(0xCA); // JZ putc_wait
    push_addr(&mut c, putc_wait as u16);
    c.push(0xF1); // POP PSW
    c.push(0xD3); // OUT DATA
    c.push(SERIAL_DATA);
    c.push(0xC9); // RET

    // getc: poll until RX ready, return the byte in A.
    labels.getc = c.len();
    let getc_wait = c.len();
    c.push(0xDB); // IN STATUS
    c.push(SERIAL_STATUS);
    c.push(0xE6); // ANI RX_READY
    c.push(ST_RX_READY);
    c.push(0xCA); // JZ getc_wait
    push_addr(&mut c, getc_wait as u16);
    c.push(0xDB); // IN DATA
    c.push(SERIAL_DATA);
    c.push(0xC9); // RET

    // msg: the sign-on string, NUL-terminated.
    labels.msg = c.len();
    for &b in b"CPT serial demo online.\r\nType and it echoes:\r\n" {
        c.push(b);
    }
    c.push(0);

    // Backpatch forward references.
    for (at, label) in fixups {
        let target = labels.resolve(label) as u16;
        c[at] = (target & 0xFF) as u8;
        c[at + 1] = (target >> 8) as u8;
    }

    c
}

fn push_addr(c: &mut Vec<u8>, addr: u16) {
    c.push((addr & 0xFF) as u8);
    c.push((addr >> 8) as u8);
}

#[derive(Clone, Copy)]
enum Label {
    Echo,
    Putc,
    Getc,
    Msg,
}

#[derive(Default)]
struct Labels {
    print_loop: usize,
    echo: usize,
    putc: usize,
    getc: usize,
    msg: usize,
}

impl Labels {
    fn resolve(&self, l: Label) -> usize {
        match l {
            Label::Echo => self.echo,
            Label::Putc => self.putc,
            Label::Getc => self.getc,
            Label::Msg => self.msg,
        }
    }
}
