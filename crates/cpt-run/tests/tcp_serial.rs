//! Verify the TCP serial transport moves bytes both directions over a real socket.

#[path = "../src/tcp_serial.rs"]
mod tcp_serial;

use cpt_machine::Transport;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

#[test]
fn tcp_transport_round_trip() {
    // Pick an ephemeral port by binding, then hand the address to the transport's listener.
    // We run the emulator-side transport in listen mode on a background thread, and act as the
    // peer from the test thread.
    let addr = "127.0.0.1:0";
    let listener = std::net::TcpListener::bind(addr).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener); // free it so TcpTransport::listen can re-bind the same port

    let handle = thread::spawn(move || {
        let mut t = tcp_serial::TcpTransport::listen(&format!("127.0.0.1:{}", port)).unwrap();
        // Give the peer a moment to connect/send.
        thread::sleep(Duration::from_millis(200));
        // Emulator sends "HI" to the peer.
        t.send(b"HI");
        // Collect whatever the peer sent (poll briefly).
        let mut got = Vec::new();
        for _ in 0..50 {
            got.extend(t.recv());
            if got.len() >= 2 {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        got
    });

    // Peer side: connect, send "OK", and read the emulator's "HI".
    thread::sleep(Duration::from_millis(100));
    let mut peer = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    peer.write_all(b"OK").unwrap();
    peer.flush().unwrap();

    let mut buf = [0u8; 8];
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let n = peer.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"HI", "peer should receive the emulator's bytes");

    let got = handle.join().unwrap();
    assert_eq!(got, b"OK", "emulator should receive the peer's bytes");
}

#[test]
fn telnet_listen_negotiates_and_filters() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let handle = thread::spawn(move || {
        let mut t = tcp_serial::TcpTransport::listen_with(&format!("127.0.0.1:{}", port), true).unwrap();
        let mut got = Vec::new();
        for _ in 0..100 {
            got.extend(t.recv());
            if got.len() >= 4 {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        got
    });

    thread::sleep(Duration::from_millis(100));
    let mut peer = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut buf = [0u8; 6];
    peer.read_exact(&mut buf).unwrap();
    // IAC WILL ECHO, IAC WILL SUPPRESS-GO-AHEAD.
    assert_eq!(buf, [255, 251, 1, 255, 251, 3]);

    // Client replies with DO ECHO / DO SGA, a terminal-type subnegotiation, then "dir\r\0".
    let mut msg = vec![255, 253, 1, 255, 253, 3, 255, 250, 24, 0, b'V', b'T', 255, 240];
    msg.extend_from_slice(b"dir\r");
    peer.write_all(&msg).unwrap();
    peer.flush().unwrap();

    assert_eq!(handle.join().unwrap(), b"dir\r", "only the data bytes should reach the guest");
}

#[test]
fn telnet_filter_unescapes_iac() {
    let mut f = tcp_serial::TelnetFilter::default();
    let out: Vec<u8> = [b'a', 255, 255, b'b', 255, 241, b'c'].iter().filter_map(|&b| f.feed(b)).collect();
    assert_eq!(out, vec![b'a', 255, b'b', b'c']);
}
