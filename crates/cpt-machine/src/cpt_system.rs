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
        machine.attach_device(Box::new(TimerDevice::new(33_333, 6)));
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
        CptSystem { exec: Executive::new(cpu, machine), fdc: fdc_dev, pit, ack, kb, usart, printer }
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
}
