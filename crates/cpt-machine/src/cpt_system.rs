//! The real CPT system booted from a program disk, with the hardware found so far. `cpt-run
//! --cpt-boot` and the browser (cpt-wasm) both use it.
//!
//! The boot ROM is not available, so [`CptSystem::boot`] does what the boot code shows it must
//! have done: load track 1 sector 0 (the sector with the deleted-data mark) to 0000H and run it
//! with A = 0 and D = 0 (drive 0). See `.kiro/specs/cpt8000-emulator/program-disk.md`.

use crate::cpt_display::{self, CptScreen};
use crate::cpt_floppy::{self, CptFloppy};
use crate::cpt_interrupts::{InterruptAck, Pit8253};
use crate::cpt_keyboard::CptKeyboard;
use crate::cpt_printer::CptPrinter;
use crate::cpt_usart::Usart8251;
use crate::{DiskImage, Executive, Machine, MachineConfig, TimerDevice};
use i8080_cpu::{Bus, Cpu};

/// Where the display hardware starts walking the display list.
pub const DISPLAY_HEAD: u16 = 0x7FF8;

/// The read time used by default: 40,000 cycles (20 ms at 2 MHz). The software checks disk
/// timing: the loader waits 31,080 cycles and fails ("E1") if the next read is already done,
/// and the main system reports "THE DISK IN STATION 2 SEEMS TO BE FAULTY." if reads take
/// 20,000 cycles or less (32,000 passes). A real 8-inch disk at 360 rpm, read in the skew-3
/// order, takes about 31 ms between consecutive logical sectors, so start-up cannot be made
/// faster by shortening reads; run the emulation faster than real time instead.
pub const READ_DELAY_CYCLES: u32 = 40_000;

/// Emulated cycles until the started system has settled on its editing screen and takes keys
/// (about 37 s at 2 MHz with [`READ_DELAY_CYCLES`]; the tests type at 80M cycles).
pub const SETTLED_CYCLES: u64 = 80_000_000;

/// A booted real system and handles to its devices.
pub struct CptSystem {
    pub exec: Executive,
    /// Device indexes in `exec.machine` (for tools that take the executive apart).
    pub fdc: usize,
    pub pit: usize,
    pub ack: usize,
    pub kb: usize,
    pub usart: usize,
    pub printer: usize,
    /// The display memory as of the last vertical retrace (see [`CptSystem::run_cycles`]).
    frame: FrameLatch,
}

/// The display's RST vector and period (60 Hz is a guess).
pub const RETRACE_VECTOR: u8 = 6;
pub const RETRACE_PERIOD: u32 = 33_333;

/// If the retrace handler has not returned this long after it was entered, latch anyway.
const HANDLER_LIMIT_CYCLES: u64 = 20_000;

/// A latch older than this (no retrace delivered for about 4 frames, e.g. during start-up or
/// with interrupts off) is stale: [`CptSystem::frame_screen`] decodes live memory instead.
const STALE_CYCLES: u64 = 4 * RETRACE_PERIOD as u64;

/// Bank 0 as the display would scan it: copied when the retrace handler (which applies the
/// queued display-list patches at 0504H) returns. The real software makes its big screen
/// changes between retraces and the beam scans after the handler, so a host that decodes
/// from this copy never shows a half-made change, as a decode of live memory at an arbitrary
/// moment can.
struct FrameLatch {
    bytes: Box<[u8; crate::memory::BANK_SIZE]>,
    /// CPU cycle of the last copy; `None` until the first.
    at: Option<u64>,
    /// The retrace handler is running: its return address is on the stack at this SP, and it
    /// was entered at this cycle.
    pending: Option<(u16, u64)>,
}

impl CptSystem {
    /// Boot a CPT program disk image (77 x 16 x 256 bytes, e.g. from `tools/td0.py --raw`).
    /// The image is copied; the caller's bytes are never written.
    /// `read_delay` is the cycles each sector transfer takes (use [`READ_DELAY_CYCLES`]; 0 =
    /// instant, which fails the loader's timing check).
    pub fn boot(image: &[u8], read_delay: u32) -> Result<CptSystem, String> {
        let geom = cpt_floppy::cpt_geometry();
        if image.len() != geom.total_bytes() {
            return Err(format!("the image is {} bytes; a CPT program disk is {}", image.len(), geom.total_bytes()));
        }
        let mut disk = DiskImage::blank(geom);
        disk.load_bytes(image);
        Ok(CptSystem::boot_disk(disk, read_delay))
    }

    /// Boot from a disk already in drive 1 (e.g. after a reset). The disk keeps its contents
    /// and its "written to" state. It must have the CPT geometry ([`cpt_floppy::cpt_geometry`]).
    pub fn boot_disk(disk: DiskImage, read_delay: u32) -> CptSystem {
        let mut boot_sector = [0u8; 256];
        disk.read_block(16, &mut boot_sector);

        let mut machine = Machine::new(MachineConfig::cpt_real());
        let mut fdc = CptFloppy::new();
        fdc.mount(0, disk);
        fdc.op_delay_cycles = read_delay;
        let fdc_dev = machine.device_count();
        machine.attach_device(Box::new(fdc));
        // The 8253 timer at 1CH-1FH (counter 1 -> RST 5, counter 2 -> RST 7: a guess) and the
        // interrupt-acknowledge port at 00H.
        let pit = machine.device_count();
        machine.attach_device(Box::new(Pit8253::cpt()));
        let ack = machine.device_count();
        machine.attach_device(Box::new(InterruptAck::new(0x00)));
        // The display's vertical retrace on RST 6 (its handler applies queued display-list
        // updates). 60 Hz is a guess.
        machine.attach_device(Box::new(TimerDevice::new(RETRACE_PERIOD, RETRACE_VECTOR)));
        // The keyboard on 01H/02H, raising RST 2.
        let kb = machine.device_count();
        machine.attach_device(Box::new(CptKeyboard::new()));
        // The serial port: an 8251 at 4AH/4BH (and 4CH), receive on RST 1, transmit on RST 4.
        // Nothing is attached to the line until the host sets DSR and pumps it.
        let usart = machine.device_count();
        machine.attach_device(Box::new(Usart8251::cpt()));
        // Printer A on the parallel daisy-wheel port at 42H/43H, always ready.
        let printer = machine.device_count();
        machine.attach_device(Box::new(CptPrinter::cpt()));

        machine.mem.load_bank0(0, &boot_sector);
        let mut cpu = Cpu::new();
        cpu.pc = 0;
        let frame = FrameLatch { bytes: Box::new([0; crate::memory::BANK_SIZE]), at: None, pending: None };
        CptSystem { exec: Executive::new(cpu, machine), fdc: fdc_dev, pit, ack, kb, usart, printer, frame }
    }

    /// Take both disks out (drive 1, drive 2), e.g. before a reset or power-off.
    pub fn take_disks(&mut self) -> [Option<DiskImage>; 2] {
        let f = self.floppy();
        [f.eject(0), f.eject(1)]
    }

    pub fn floppy(&mut self) -> &mut CptFloppy {
        self.exec.machine.device_mut(self.fdc).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    pub fn pit(&mut self) -> &mut Pit8253 {
        self.exec.machine.device_mut(self.pit).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    pub fn interrupt_ack(&mut self) -> &mut InterruptAck {
        self.exec.machine.device_mut(self.ack).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    pub fn keyboard(&mut self) -> &mut CptKeyboard {
        self.exec.machine.device_mut(self.kb).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    pub fn usart(&mut self) -> &mut Usart8251 {
        self.exec.machine.device_mut(self.usart).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    pub fn printer(&mut self) -> &mut CptPrinter {
        self.exec.machine.device_mut(self.printer).unwrap().as_any_mut().downcast_mut().unwrap()
    }

    /// Decode the screen the display hardware would show now.
    pub fn screen(&mut self, max_rows: usize) -> CptScreen {
        let m = &mut self.exec.machine;
        cpt_display::render(|a| m.read(a), DISPLAY_HEAD, max_rows)
    }

    /// Run about `budget` cycles (like [`Executive::run_cycles`]), latching the display memory
    /// each time the retrace handler finishes. Returns cycles run.
    pub fn run_cycles(&mut self, budget: u64) -> u64 {
        let start = self.exec.cpu.cycles;
        while self.exec.cpu.cycles.wrapping_sub(start) < budget && !self.exec.is_terminally_halted() {
            self.exec.step();
            let now = self.exec.cpu.cycles;
            if self.exec.last_irq == Some(RETRACE_VECTOR) {
                // The return address was just pushed at SP; the handler is done when it is
                // popped (SP above it). Interrupts nested in the handler only push lower.
                self.frame.pending = Some((self.exec.cpu.sp, now));
            } else if let Some((sp, entered)) = self.frame.pending {
                if self.exec.cpu.sp > sp || now.wrapping_sub(entered) >= HANDLER_LIMIT_CYCLES {
                    self.latch_frame();
                }
            }
        }
        self.exec.cpu.cycles.wrapping_sub(start)
    }

    fn latch_frame(&mut self) {
        self.frame.bytes.copy_from_slice(self.exec.machine.mem.bank0());
        self.frame.at = Some(self.exec.cpu.cycles);
        self.frame.pending = None;
    }

    /// Cycles since the display memory was last latched (`None` if never).
    pub fn frame_age(&self) -> Option<u64> {
        self.frame.at.map(|at| self.exec.cpu.cycles.wrapping_sub(at))
    }

    /// Decode the screen as of the last retrace (driven by [`CptSystem::run_cycles`]), or live
    /// memory if no retrace has been seen recently.
    pub fn frame_screen(&mut self, max_rows: usize) -> CptScreen {
        match self.frame.at {
            Some(at) if self.exec.cpu.cycles.wrapping_sub(at) <= STALE_CYCLES => {
                let b = &self.frame.bytes;
                cpt_display::render(|a| b[(a & 0x7FFF) as usize], DISPLAY_HEAD, max_rows)
            }
            _ => self.screen(max_rows),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_cpt_program_disk_image_boots() {
        assert!(CptSystem::boot(&[0u8; 1000], READ_DELAY_CYCLES).is_err());
        let size = cpt_floppy::cpt_geometry().total_bytes();
        let mut image = vec![0u8; size];
        // Track 1 sector 0 (the boot sector) goes to 0000H: HLT with interrupts off.
        image[16 * 256] = 0xF3; // DI
        image[16 * 256 + 1] = 0x76; // HLT
        let mut sys = CptSystem::boot(&image, READ_DELAY_CYCLES).expect("boots");
        sys.exec.run_cycles(1000);
        assert!(sys.exec.is_terminally_halted());
        assert_eq!(sys.exec.cpu.pc, 2);
        assert!(sys.floppy().image(0).is_some(), "the disk is in drive 0");
        assert!(sys.floppy().image(1).is_none());
        // No display list yet: zeroed memory never ends a row or the screen.
        assert_eq!(sys.screen(10).ending, cpt_display::Ending::ByteLimit);
    }

    /// A boot sector whose retrace handler counts at 7001H while the main code halts.
    fn retrace_counter(enable: bool) -> CptSystem {
        let mut image = vec![0u8; cpt_floppy::cpt_geometry().total_bytes()];
        let s = &mut image[16 * 256..17 * 256];
        // Every RST vector: EI; RET.
        for v in (8..0x40).step_by(8) {
            s[v] = 0xFB;
            s[v + 1] = 0xC9;
        }
        // 0000: LXI SP,7F00; EI (or DI); HLT; JMP 0003
        s[..8].copy_from_slice(&[0x31, 0x00, 0x7F, if enable { 0xFB } else { 0xF3 }, 0x76, 0xC3, 0x03, 0x00]);
        // 0030: PUSH PSW; LDA 7001; INR A; STA 7001; POP PSW; EI; RET
        s[0x30..0x3B].copy_from_slice(&[0xF5, 0x3A, 0x01, 0x70, 0x3C, 0x32, 0x01, 0x70, 0xF1, 0xFB, 0xC9]);
        CptSystem::boot(&image, READ_DELAY_CYCLES).expect("boots")
    }

    #[test]
    fn the_frame_is_latched_when_the_retrace_handler_returns() {
        let mut sys = retrace_counter(true);
        sys.run_cycles(10 * RETRACE_PERIOD as u64 + 1000);
        let live = sys.exec.machine.mem.bank0()[0x7001];
        assert!(live >= 9, "the retrace handler ran about 10 times, got {live}");
        assert!(sys.frame.at.is_some());
        // Copied after the handler's update, not when it was entered.
        assert_eq!(sys.frame.bytes[0x7001], live);
        assert!(sys.frame.pending.is_none());
        // A change made between retraces is not shown until the next one.
        sys.exec.machine.mem.load_bank0(0x7002, &[0xAA]);
        assert_eq!(sys.frame.bytes[0x7002], 0);
        sys.run_cycles(RETRACE_PERIOD as u64 + 1000);
        assert_eq!(sys.frame.bytes[0x7002], 0xAA);
    }

    #[test]
    fn without_retraces_the_screen_is_decoded_live() {
        let mut sys = retrace_counter(false);
        sys.run_cycles(10 * RETRACE_PERIOD as u64);
        assert!(sys.frame.at.is_none(), "interrupts are off: no retrace was delivered");
        assert_eq!(format!("{:?}", sys.frame_screen(10)), format!("{:?}", sys.screen(10)));
    }
}
