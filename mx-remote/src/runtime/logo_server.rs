// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The HTTP server units fetch an overlay sync's logo images from.
//!
//! A unit asks for `GET /osd-sync/slotN.png`, N being 1 to 4, one image at a
//! time. It takes only a 200 whose body is exactly the size the sync gave,
//! sent plainly with a `Content-Length`: it reads neither chunked nor
//! compressed bodies. A 404 fails the unit's sync, so an empty or unknown
//! slot answers that.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::types::OSD_SYNC_SLOTS;

use super::{lock, SHUTDOWN_POLL};

/// The longest request head read before giving up on a connection.
const REQUEST_HEAD_MAX: usize = 8192;

/// How long a unit may take to send its request, and to read the image.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// The images being served, slot 1 first. An empty one is not served.
pub(super) type Images = [Arc<[u8]>; OSD_SYNC_SLOTS];

/// A running server. Dropping it stops the thread and waits for it.
pub(super) struct LogoServer {
    port: u16,
    images: Arc<Mutex<Images>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LogoServer {
    /// Listens on `port` on every interface, or on a free port when it is
    /// zero.
    ///
    /// Every interface rather than the one the client talks on: a unit
    /// fetches from the address the sync came from, which is whichever
    /// address the host sent it with.
    pub(super) fn start(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))?;
        // Polled rather than parked in accept, so dropping the server does
        // not have to wait for a unit to connect.
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let empty: Images = core::array::from_fn(|_| Arc::from(&[][..]));
        let images = Arc::new(Mutex::new(empty));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let images = Arc::clone(&images);
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("mxr-logos".to_owned())
                .spawn(move || accept_loop(&listener, &images, &stop))?
        };
        Ok(Self {
            port,
            images,
            stop,
            thread: Some(thread),
        })
    }

    /// The port it listens on.
    pub(super) fn port(&self) -> u16 {
        self.port
    }

    /// Replaces the images being served.
    pub(super) fn set_images(&self, images: Images) {
        *lock(&self.images) = images;
    }
}

impl Drop for LogoServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept_loop(listener: &TcpListener, images: &Mutex<Images>, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            // Served one at a time, as units fetch them one at a time.
            Ok((stream, _)) => {
                let _ = serve(stream, images);
            }
            Err(_) => std::thread::sleep(SHUTDOWN_POLL),
        }
    }
}

/// Answers one request and closes the connection.
fn serve(mut stream: TcpStream, images: &Mutex<Images>) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf)?;
        if n == 0 || head.len() + n > REQUEST_HEAD_MAX {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    let image = slot_of(&head).map(|slot| Arc::clone(&lock(images)[slot]));
    match image.filter(|png| !png.is_empty()) {
        Some(png) => {
            let crc = crate::wire::crc32(&png);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\
                 ETag: \"{crc:08x}\"\r\nConnection: close\r\n\r\n",
                png.len()
            )?;
            stream.write_all(&png)?;
        }
        None => stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?,
    }
    stream.flush()
}

/// The slot a request asks for, counting from 0.
fn slot_of(head: &[u8]) -> Option<usize> {
    let line = head.split(|&b| b == b'\r').next()?;
    let mut parts = line.split(|&b| b == b' ');
    if parts.next()? != b"GET" {
        return None;
    }
    let path = parts.next()?;
    let n = path
        .strip_prefix(b"/osd-sync/slot")?
        .strip_suffix(b".png")?;
    match n {
        [d @ b'1'..=b'4'] => Some(usize::from(d - b'1')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, path: &str) -> Vec<u8> {
        request(port, "GET", path)
    }

    fn request(port: u16, method: &str, path: &str) -> Vec<u8> {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\n\r\n").expect("send");
        let mut out = Vec::new();
        s.read_to_end(&mut out).expect("read");
        out
    }

    fn split(response: &[u8]) -> (String, Vec<u8>) {
        let at = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("no head");
        (
            String::from_utf8_lossy(&response[..at]).into_owned(),
            response[at + 4..].to_vec(),
        )
    }

    #[test]
    fn a_slot_is_served_whole_with_its_length() {
        let server = LogoServer::start(0).expect("start");
        let png: Vec<u8> = (0..70_000u32).map(|i| (i * 7) as u8).collect();
        server.set_images([
            Arc::from(&b""[..]),
            Arc::from(&b"two"[..]),
            Arc::from(&b""[..]),
            Arc::from(png.as_slice()),
        ]);
        let (head, body) = split(&get(server.port(), "/osd-sync/slot4.png"));
        assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
        assert!(head.contains("\r\nContent-Length: 70000\r\n"), "{head}");
        assert!(!head.contains("Transfer-Encoding"), "{head}");
        assert_eq!(body, png);
        let (_, body) = split(&get(server.port(), "/osd-sync/slot2.png"));
        assert_eq!(body, b"two");
    }

    #[test]
    fn an_empty_or_unknown_slot_is_not_found() {
        let server = LogoServer::start(0).expect("start");
        server.set_images([
            Arc::from(&b""[..]),
            Arc::from(&b"two"[..]),
            Arc::from(&b"three"[..]),
            Arc::from(&b"four"[..]),
        ]);
        for path in [
            "/osd-sync/slot1.png",
            "/osd-sync/slot0.png",
            "/osd-sync/slot5.png",
            "/osd-sync/slot2.jpg",
            "/other/slot2.png",
            "/osd-sinc/slot2.png",
        ] {
            let (head, body) = split(&get(server.port(), path));
            assert!(head.starts_with("HTTP/1.1 404 "), "{path}: {head}");
            assert!(body.is_empty(), "{path}");
        }
        let (head, _) = split(&request(server.port(), "POST", "/osd-sync/slot2.png"));
        assert!(head.starts_with("HTTP/1.1 404 "), "POST: {head}");
    }
}
