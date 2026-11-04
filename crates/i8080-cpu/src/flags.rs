//! The 8080 condition flags (the flag half of the PSW).
//!
//! The 8080 has five flags: Sign, Zero, Auxiliary Carry, Parity, and Carry. When pushed as the
//! low byte of the PSW they occupy fixed bit positions, with bit 1 always 1 and bits 3 and 5
//! always 0:
//! ```text
//!   bit:  7  6  5  4  3  2  1  0
//!         S  Z  0  AC 0  P  1  CY
//! ```

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    pub sign: bool,
    pub zero: bool,
    pub aux_carry: bool,
    pub parity: bool,
    pub carry: bool,
}

impl Flags {
    /// Pack the flags into the PSW low byte, with the fixed bits applied.
    pub fn to_psw(self) -> u8 {
        let mut b = 0b0000_0010; // bit 1 is always set
        if self.sign {
            b |= 0b1000_0000;
        }
        if self.zero {
            b |= 0b0100_0000;
        }
        if self.aux_carry {
            b |= 0b0001_0000;
        }
        if self.parity {
            b |= 0b0000_0100;
        }
        if self.carry {
            b |= 0b0000_0001;
        }
        b
    }

    /// Unpack the flags from a PSW low byte (ignoring the fixed bits).
    pub fn from_psw(b: u8) -> Self {
        Flags {
            sign: b & 0b1000_0000 != 0,
            zero: b & 0b0100_0000 != 0,
            aux_carry: b & 0b0001_0000 != 0,
            parity: b & 0b0000_0100 != 0,
            carry: b & 0b0000_0001 != 0,
        }
    }

    /// Set S, Z, and P from an 8-bit result (the common case after logic/INR/DCR). Carry and
    /// aux-carry are left to the caller since they depend on the specific operation.
    pub fn set_szp(&mut self, value: u8) {
        self.sign = value & 0x80 != 0;
        self.zero = value == 0;
        self.parity = value.count_ones() % 2 == 0; // true = even parity
    }
}
