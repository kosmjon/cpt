//! The CPT's 8251 USART model (4AH-4CH): the control sequence the TTY disk writes, status
//! bits, line timing, the two interrupt lines and the host side.

use cpt_machine::cpt_usart::*;
use cpt_machine::{Device, LoopbackTransport, Machine, MachineConfig};
use i8080_cpu::Bus;

/// The TTY disk's set-up: reset, then mode 4EH (async x16, 8 data bits, no parity, 1 stop
/// bit) and command 37H (TxEN, DTR, RxE, error reset, RTS).
fn set_up(u: &mut Usart8251) {
    for v in [0x00, 0x00, 0x00, 0x40, 0x4E, 0x37] {
        u.io_out(0x4A, v);
    }
}

/// Tick in small steps, as the executive does.
fn run(u: &mut Usart8251, cycles: u32) {
    let mut left = cycles;
    while left > 0 {
        let n = left.min(10);
        u.tick(n);
        left -= n;
    }
}

#[test]
fn claims_4ah_to_4ch() {
    let u = Usart8251::cpt();
    assert_eq!(u.ports(), &[0x4A, 0x4B, 0x4C]);
    assert_eq!((u.rx_vector, u.tx_vector), (Some(1), Some(4)));
}

#[test]
fn the_reset_sequence_reaches_mode_and_command() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    assert_eq!(u.mode(), 0x4E);
    assert_eq!(u.command(), 0x37);
    // 1200 baud, 10 bits a character: 16,666 cycles at 2 MHz.
    assert_eq!(u.char_cycles(), 16_666);
    // From a mode byte that started async (not sync), the zeros are a sync mode and its two
    // sync characters, so the sequence also works from the command state.
    u.io_out(0x4A, 0x00);
    u.io_out(0x4A, 0x00);
    u.io_out(0x4A, 0x00);
    u.io_out(0x4A, 0x40);
    assert_eq!(u.command(), 0, "internal reset");
    u.io_out(0x4A, 0x4E);
    u.io_out(0x4A, 0x16);
    assert_eq!((u.mode(), u.command()), (0x4E, 0x16));
    // 4CH is recorded.
    u.io_out(0x4C, 0x09);
    assert_eq!(u.rate_select(), Some(0x09));
    assert_eq!(u.control_log.last(), Some(&(0x4C, 0x09)));
}

#[test]
fn status_bits() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    assert_eq!(u.io_in(0x4A), ST_TXRDY | ST_TXEMPTY, "idle, nothing attached");
    u.set_dsr(true);
    assert_eq!(u.io_in(0x4A) & ST_DSR, ST_DSR);
    u.io_out(0x4B, b'A');
    assert_eq!(u.io_in(0x4A) & (ST_TXRDY | ST_TXEMPTY), 0, "holding register full");
    u.tick(1);
    assert_eq!(u.io_in(0x4A) & (ST_TXRDY | ST_TXEMPTY), ST_TXRDY, "on the line; holding free");
}

#[test]
fn a_character_takes_its_time_on_the_line() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    u.io_out(0x4B, 0x48);
    run(&mut u, 16_000);
    assert!(u.take_tx().is_empty(), "still sending");
    run(&mut u, 1_000);
    assert_eq!(u.take_tx(), vec![0x48]);
    assert_eq!(u.sent, 1);
    assert_eq!(u.io_in(0x4A) & ST_TXEMPTY, ST_TXEMPTY);
}

#[test]
fn the_transmitter_holds_its_character_while_txen_is_off() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    u.io_out(0x4A, 0x16); // TxEN off, as the TTY disk does after a transmission
    u.io_out(0x4B, b'x');
    run(&mut u, 40_000);
    assert!(u.take_tx().is_empty());
    assert_eq!(u.take_irq(), None, "TxRDY's interrupt is masked by TxEN");
    u.io_out(0x4A, 0x37);
    run(&mut u, 20_000);
    assert_eq!(u.take_tx(), vec![b'x']);
}

#[test]
fn receiving_paces_characters_and_interrupts_on_rst_1() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    u.io_out(0x4A, 0x16); // receiver on, transmitter off (no TX interrupt)
    u.push_rx(b"AB");
    assert_eq!(u.take_irq(), None);
    run(&mut u, 17_000);
    assert_eq!(u.io_in(0x4A) & ST_RXRDY, ST_RXRDY);
    assert_eq!(u.take_irq(), Some(1));
    // The next character waits until the CPU has read this one, however long that takes.
    run(&mut u, 100_000);
    assert_eq!(u.io_in(0x4A) & ST_OE, 0, "no overrun");
    assert_eq!(u.io_in(0x4B), b'A');
    assert_eq!(u.io_in(0x4A) & ST_RXRDY, 0);
    assert_eq!(u.take_irq(), None, "the level drops once the character is read");
    run(&mut u, 17_000);
    assert_eq!(u.io_in(0x4B), b'B');
    assert_eq!(u.received, 2);
}

#[test]
fn characters_arriving_with_the_receiver_off_are_lost() {
    let mut u = Usart8251::cpt();
    u.push_rx(b"lost");
    run(&mut u, 100_000);
    assert_eq!(u.io_in(0x4A) & ST_RXRDY, 0);
    set_up(&mut u);
    run(&mut u, 100_000);
    assert_eq!(u.received, 0);
}

/// Without an acknowledge port, TxRDY is a plain level.
#[test]
fn the_transmit_interrupt_is_rst_4_while_txen_and_the_holding_register_is_empty() {
    let mut u = Usart8251::new(0x4A, Some(1), Some(4));
    assert_eq!(u.take_irq(), None, "after power-up, nothing is enabled");
    set_up(&mut u);
    assert_eq!(u.take_irq(), Some(4));
    assert_eq!(u.take_irq(), Some(4), "a level: it asks again until something is written");
    u.io_out(0x4B, b'1');
    assert_eq!(u.take_irq(), None, "the holding register is full");
    u.tick(1);
    assert_eq!(u.take_irq(), Some(4), "double-buffered: ready for the next while one is sent");
}

/// The CPT's 8251: the transmit request is taken once, then held off until 10H (RST 4's bit)
/// is written to the acknowledge port 00H. The CPT's handler writes 10H only after sending a
/// byte, and its queueing routine writes it when there is a new byte.
#[test]
fn the_cpt_transmit_request_waits_for_10h_on_port_00h() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    assert_eq!(u.take_irq(), Some(4));
    assert_eq!(u.take_irq(), None, "held off: nothing to send, so no 10H yet");
    u.observe_out(0x00, 0x80);
    assert_eq!(u.take_irq(), None, "another level's acknowledge doesn't re-arm it");
    u.observe_out(0x01, 0x10);
    assert_eq!(u.take_irq(), None, "nor does 10H to another port");
    u.observe_out(0x00, 0x10);
    assert_eq!(u.take_irq(), Some(4), "10H on 00H re-arms it");
    u.io_out(0x4B, b'1');
    u.observe_out(0x00, 0x10);
    assert_eq!(u.take_irq(), None, "armed, but the holding register is full");
    u.tick(1);
    assert_eq!(u.take_irq(), Some(4), "the character moved to the line: ready for the next");
    assert_eq!(u.tx_interrupts, 3);
}

#[test]
fn send_break_is_counted() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    u.io_out(0x4A, 0x37 | CMD_SBRK);
    u.io_out(0x4A, 0x37);
    assert_eq!(u.breaks, 1);
}

#[test]
fn pump_moves_bytes_through_a_transport() {
    let mut u = Usart8251::cpt();
    set_up(&mut u);
    u.io_out(0x4B, b'z');
    run(&mut u, 20_000);
    // A loopback line: what the CPT sends comes straight back to it.
    let mut line = LoopbackTransport::new();
    u.pump(&mut line);
    u.pump(&mut line);
    run(&mut u, 20_000);
    assert_eq!(u.io_in(0x4B), b'z');
}

#[test]
fn the_machine_routes_4ah_to_4ch_to_it() {
    let mut m = Machine::new(MachineConfig::cpt_real());
    m.attach_device(Box::new(Usart8251::cpt()));
    for v in [0x00, 0x00, 0x00, 0x40, 0x4E, 0x37] {
        m.io_out(0x4A, v);
    }
    m.io_out(0x4C, 0x09);
    assert_eq!(m.io_in(0x4A) & ST_TXRDY, ST_TXRDY);
    assert!(m.unmapped_io.is_empty(), "{:?}", m.unmapped_io);
    assert!(m.unmapped_in.is_empty());
}
