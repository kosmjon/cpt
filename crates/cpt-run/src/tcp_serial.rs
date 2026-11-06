//! A TCP-backed `Transport` for the serial device (native only).
//!
//! The serial device's `Transport::send`/`recv` are called synchronously from the main loop,
//! so they must not block. This transport owns a background thread that holds the `TcpStream`
//! and does the blocking socket I/O; it communicates with the main thread over channels. `send`
//! pushes outbound bytes to the thread; `recv` drains bytes the thread has read. Both are
//! non-blocking.
//!
//! Two modes: **listen** (wait for a peer to connect to us) or **connect** (dial a host:port).

use cpt_machine::Transport;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// Telnet protocol bytes (RFC 854/857/858).
const IAC: u8 = 255;
const DONT: u8 = 254;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;

/// Removes Telnet command sequences from the client's byte stream, passing data through.
#[derive(Default)]
pub struct TelnetFilter {
    state: FilterState,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum FilterState {
    #[default]
    Data,
    /// Just saw IAC.
    Iac,
    /// Saw IAC WILL/WONT/DO/DONT; the next byte is the option.
    Option,
    /// Inside a subnegotiation (IAC SB ... IAC SE).
    Sub,
    /// Saw IAC inside a subnegotiation.
    SubIac,
}

impl TelnetFilter {
    /// Feed one received byte; returns it if it is data, None if it was part of a command.
    pub fn feed(&mut self, b: u8) -> Option<u8> {
        use FilterState::*;
        match (self.state, b) {
            (Data, IAC) => {
                self.state = Iac;
                None
            }
            (Data, _) => Some(b),
            (Iac, IAC) => {
                self.state = Data;
                Some(IAC) // escaped 0xFF data byte
            }
            (Iac, WILL..=DONT) => {
                self.state = Option;
                None
            }
            (Iac, SB) => {
                self.state = Sub;
                None
            }
            (Iac, _) => {
                self.state = Data; // a two-byte command (NOP, GA, AYT, ...)
                None
            }
            (Option, _) => {
                self.state = Data;
                None
            }
            (Sub, IAC) => {
                self.state = SubIac;
                None
            }
            (Sub, _) => None,
            (SubIac, SE) => {
                self.state = Data;
                None
            }
            (SubIac, _) => {
                self.state = Sub;
                None
            }
        }
    }
}

pub struct TcpTransport {
    to_peer: Sender<Vec<u8>>,
    from_peer: Receiver<u8>,
}

impl TcpTransport {
    /// Listen on `addr` (e.g. "127.0.0.1:2323") and accept one connection.
    #[allow(dead_code)] // plain-TCP listen; main.rs uses listen_with
    pub fn listen(addr: &str) -> std::io::Result<Self> {
        Self::listen_with(addr, false)
    }

    /// Listen on `addr`; if `telnet`, act as a Telnet server: tell the client we echo and
    /// suppress go-ahead (so it stops echoing locally and sends each keystroke as typed, not
    /// a buffered line), and strip Telnet commands (IAC ...) from what it sends us.
    pub fn listen_with(addr: &str, telnet: bool) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        eprintln!("serial: listening on {} (waiting for a connection)...", addr);
        let (mut stream, peer) = listener.accept()?;
        eprintln!("serial: connected from {}", peer);
        if telnet {
            stream.write_all(&[IAC, WILL, OPT_ECHO, IAC, WILL, OPT_SGA])?;
            stream.flush()?;
        }
        Ok(Self::from_stream(stream, telnet))
    }

    /// Connect out to `addr` (e.g. "127.0.0.1:2323").
    // (Used by main.rs; the `#[path]`-included copy in the integration test only exercises
    // `listen`, so silence the unused warning there.)
    #[allow(dead_code)]
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        eprintln!("serial: connected to {}", addr);
        Ok(Self::from_stream(stream, false))
    }

    fn from_stream(stream: TcpStream, telnet: bool) -> Self {
        // One channel each direction between the main thread and the socket thread.
        let (tx_out, rx_out) = mpsc::channel::<Vec<u8>>(); // main -> socket (to send)
        let (tx_in, rx_in) = mpsc::channel::<u8>(); // socket -> main (received)

        // Reader thread: blocking reads from the socket, forwards bytes to the main thread.
        let read_stream = stream.try_clone().expect("clone TcpStream for reader");
        thread::spawn(move || {
            let mut s = read_stream;
            let mut buf = [0u8; 256];
            let mut filter = TelnetFilter::default();
            loop {
                match s.read(&mut buf) {
                    Ok(0) => break, // peer closed
                    Ok(n) => {
                        for &raw in &buf[..n] {
                            let b = if telnet {
                                match filter.feed(raw) {
                                    Some(b) => b,
                                    None => continue,
                                }
                            } else {
                                raw
                            };
                            if tx_in.send(b).is_err() {
                                return; // main side gone
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Writer thread: drains the outbound channel and writes to the socket.
        let mut write_stream = stream;
        thread::spawn(move || loop {
            match rx_out.recv_timeout(Duration::from_millis(200)) {
                Ok(bytes) => {
                    if write_stream.write_all(&bytes).is_err() {
                        break;
                    }
                    let _ = write_stream.flush();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        });

        TcpTransport {
            to_peer: tx_out,
            from_peer: rx_in,
        }
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.to_peer.send(bytes.to_vec());
        }
    }

    fn recv(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            match self.from_peer.try_recv() {
                Ok(b) => out.push(b),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}
