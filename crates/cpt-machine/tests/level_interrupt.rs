//! A device that holds its interrupt line asserted (like the 8251's transmit-ready line while
//! the transmitter is on) must not make an interrupt handler ending `EI; RET` pile up on the
//! stack. On the 8080 the RET after EI always runs before the next interrupt is taken.

use cpt_machine::{Device, Executive, Machine, MachineConfig};
use i8080_cpu::Cpu;
use std::any::Any;

/// Requests RST 4 all the time (a level-sensitive line that is never released).
struct HeldLine {
    ports: [u8; 1],
    requests: u32,
}

impl Device for HeldLine {
    fn ports(&self) -> &[u8] {
        &self.ports
    }
    fn io_in(&mut self, _port: u8) -> u8 {
        0
    }
    fn io_out(&mut self, _port: u8, _val: u8) {}
    fn take_irq(&mut self) -> Option<u8> {
        self.requests += 1;
        Some(4)
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[test]
fn a_held_interrupt_line_does_not_grow_the_stack() {
    let mut machine = Machine::new(MachineConfig::cpt_real());
    machine.attach_device(Box::new(HeldLine { ports: [0x77], requests: 0 }));
    // RST 4 vector (0020H): PUSH PSW; POP PSW; EI; RET.
    machine.mem.load_bank0(0x0020, &[0xF5, 0xF1, 0xFB, 0xC9]);
    // Main program at 0100H: LXI SP,0F00H; EI; JMP 0104H (spin).
    machine.mem.load_bank0(0x0100, &[0x31, 0x00, 0x0F, 0xFB, 0xC3, 0x04, 0x01]);
    let mut cpu = Cpu::new();
    cpu.pc = 0x0100;
    let mut exec = Executive::new(cpu, machine);

    let mut lowest_sp = u16::MAX;
    for _ in 0..20_000 {
        exec.step();
        if exec.cpu.sp != 0 {
            lowest_sp = lowest_sp.min(exec.cpu.sp);
        }
    }
    // One interrupt level is 2 bytes of return address plus the handler's PUSH PSW: 4 bytes.
    assert!(lowest_sp >= 0x0F00 - 8, "the stack stayed shallow; lowest SP was {:04X}", lowest_sp);
    let dev = exec.machine.device_mut(0).unwrap().as_any_mut().downcast_mut::<HeldLine>().unwrap();
    assert!(dev.requests > 1_000, "the handler really was re-entered many times ({})", dev.requests);
}
