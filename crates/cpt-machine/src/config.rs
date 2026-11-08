//! Machine configuration — the single place where CPT hardware constants live.
//!
//! Many of these are still unverified against the CPT system source; they are centralized here
//! so a confirmed value is a one-line change rather than a hunt through the codebase. Device
//! port ranges will be added as those devices are implemented.

/// How the bank-select I/O port encodes the chosen block in the byte written to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankSelectEncoding {
    /// The low `bits` of the written byte form the block index (e.g. 3 bits -> 0..=7).
    LowBits { bits: u8 },
}

impl BankSelectEncoding {
    /// Decode a byte written to the bank-select port into a block index.
    pub fn decode(self, value: u8) -> usize {
        match self {
            BankSelectEncoding::LowBits { bits } => {
                let mask = if bits >= 8 { 0xFF } else { (1u16 << bits) as u8 - 1 };
                (value & mask) as usize
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct MachineConfig {
    /// I/O port that selects which physical 32K block is mapped into the 8000-FFFF window.
    /// [VERIFY] — placeholder value until confirmed from the long-call handler source.
    pub bank_select_port: u8,

    /// How the byte written to `bank_select_port` encodes the block index. [VERIFY]
    pub bank_select_encoding: BankSelectEncoding,

    /// Number of physical 32K blocks available for bank 1.
    /// 8 blocks = 256K of switchable RAM (256K / 32K). [CONFIRMED: 256K common machine]
    pub block_count: usize,

    /// Which block is selected into bank 1 at power-on / reset.
    pub initial_block: usize,

    /// Base address (in bank 0) of the memory-mapped display RAM. The CPU writes character
    /// codes here; the display card reads them. [VERIFY] exact address from the system source.
    pub display_base: u16,

    /// CPU cycles between vertical-retrace interrupts. ~1/60 s of the CPU clock. The retrace
    /// IRQ is essential: CPT's retrace handler drains the queued scroll/screen actions, so
    /// without it the screen never updates. [VERIFY] exact clock/refresh.
    pub retrace_period_cycles: u32,

    /// RST vector the vertical-retrace interrupt uses. [VERIFY]
    pub retrace_vector: u8,

    /// CPU cycles between task-switch timer interrupts. The multitasking monitor's timer ran at
    /// about 100 Hz and was **not programmable** (no ports): 2 MHz / 100 Hz = 20,000 cycles.
    /// It is a separate interrupt from the display retrace. [CONFIRMED separate; VERIFY rate]
    pub timer_period_cycles: u32,

    /// RST vector the task-switch timer uses. [VERIFY]
    pub timer_vector: u8,

    /// CPU clock in Hz. Used only to convert CPU cycles to time for audio rendering; the
    /// interrupt periods above are already expressed in cycles. [VERIFY] (2 MHz placeholder)
    pub cpu_clock_hz: u32,

    /// Base I/O port of the speaker (2 consecutive ports: divisor low, divisor high). One
    /// channel only. The whole interface is a guess; see `speaker.rs`. [VERIFY]
    pub speaker_base_port: u8,

    /// Clock the speaker's divider counts, in Hz (tone = clock / (2 * divisor)). [VERIFY]
    pub speaker_tone_clock_hz: u32,

    /// Base I/O port of the disk controller (it claims 5 consecutive ports). [VERIFY]
    pub disk_base_port: u8,

    /// Base I/O port of the keyboard (STATUS at +0, DATA at +1). [VERIFY]
    pub keyboard_base_port: u8,

    /// RST vector the keyboard raises on a key event (the real keyboard was interrupt-driven).
    /// [VERIFY]
    pub keyboard_vector: u8,

    /// Base I/O port of the serial port / USART (DATA at +0, STATUS/CONTROL at +1). [VERIFY]
    pub serial_base_port: u8,

    /// RST vector the serial port raises when a byte is received, if interrupt-driven. [VERIFY]
    pub serial_vector: u8,
}

impl MachineConfig {
    /// The real CPT hardware as far as it is known from a real program disk's boot code
    /// (`program-disk.md`): bank select is port 03H. The floppy hardware is `CptFloppy`. Other
    /// fields keep the placeholder defaults until the real values are found.
    pub fn cpt_real() -> Self {
        MachineConfig { bank_select_port: 0x03, ..MachineConfig::default() }
    }
}

impl Default for MachineConfig {
    /// A reasonable default for a 256K machine (8 switchable blocks). The bank-select port
    /// address and encoding are placeholders pending the system source.
    fn default() -> Self {
        MachineConfig {
            bank_select_port: 0x40, // [VERIFY] placeholder
            bank_select_encoding: BankSelectEncoding::LowBits { bits: 3 }, // 8 blocks -> 3 bits
            block_count: 8,
            initial_block: 0,
            display_base: 0x4000, // [VERIFY] placeholder: display RAM lives in bank 0
            // ~2 MHz 8080 / 60 Hz ≈ 33,333 cycles per frame (placeholder). [VERIFY]
            retrace_period_cycles: 33_333,
            retrace_vector: 1, // [VERIFY] RST 1 placeholder
            // ~2 MHz 8080 / 100 Hz = 20,000 cycles (placeholder clock). [VERIFY]
            timer_period_cycles: 20_000,
            timer_vector: 4, // [VERIFY] RST 4 placeholder (distinct from retrace/kbd/serial)
            cpu_clock_hz: 2_000_000, // [VERIFY] placeholder
            speaker_base_port: 0x80, // [VERIFY] placeholder: divisor low 0x80, high 0x81
            speaker_tone_clock_hz: 1_000_000, // [VERIFY] placeholder: 440 Hz = divisor 1136
            disk_base_port: 0x50, // [VERIFY] placeholder: 5 ports 0x50..=0x54
            keyboard_base_port: 0x60, // [VERIFY] placeholder: STATUS 0x60, DATA 0x61
            keyboard_vector: 2, // [VERIFY] RST 2 placeholder
            serial_base_port: 0x70, // [VERIFY] placeholder: DATA 0x70, STATUS 0x71
            serial_vector: 3, // [VERIFY] RST 3 placeholder
        }
    }
}
