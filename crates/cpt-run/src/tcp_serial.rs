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

pub struct TcpTransport {
    to_peer: Sender<Vec<u8>>,
    from_peer: Receiver<u8>,
}

impl TcpTransport {
    /// Listen on `addr` (e.g. "127.0.0.1:2323") and accept one connection.
    pub fn listen(addr: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        eprintln!("serial: listening on {} (waiting for a connection)...", addr);
        let (stream, peer) = listener.accept()?;
        eprintln!("serial: connected from {}", peer);
        Ok(Self::from_stream(stream))
    }

    /// Connect out to `addr` (e.g. "127.0.0.1:2323").
    // (Used by main.rs; the `#[path]`-included copy in the integration test only exercises
    // `listen`, so silence the unused warning there.)
    #[allow(dead_code)]
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        eprintln!("serial: connected to {}", addr);
        Ok(Self::from_stream(stream))
    }

    fn from_stream(stream: TcpStream) -> Self {
        // One channel each direction between the main thread and the socket thread.
        let (tx_out, rx_out) = mpsc::channel::<Vec<u8>>(); // main -> socket (to send)
        let (tx_in, rx_in) = mpsc::channel::<u8>(); // socket -> main (received)

        // Reader thread: blocking reads from the socket, forwards bytes to the main thread.
        let read_stream = stream.try_clone().expect("clone TcpStream for reader");
        thread::spawn(move || {
            let mut s = read_stream;
            let mut buf = [0u8; 256];
            loop {
                match s.read(&mut buf) {
                    Ok(0) => break, // peer closed
                    Ok(n) => {
                        for &b in &buf[..n] {
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
