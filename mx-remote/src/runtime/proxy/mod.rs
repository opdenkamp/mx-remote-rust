// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Peer mode: reaching a mesh through a OneIP controller's driver proxy.
//!
//! The controller relays its mesh to one driver on the customer network over
//! unicast UDP on [`PROXY_PORT`](crate::PROXY_PORT). The driver joins by
//! proving the driver key the controller's web interface shows, the
//! controller proves it back, and every frame after that is sealed with
//! AES-256-GCM, a key per direction. The controller relays each frame
//! unchanged, by an allow-list towards the mesh.

mod crypto;
mod link;

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::raw::FrameHeader;
use crate::wire::{DeviceUid, SendError, Wrap, PROXY_PORT};

use self::link::{Link, Out};
use super::{lock, Shared};

/// How a client reaches a mesh through a OneIP controller's driver proxy
/// ([`Config::proxy`](crate::Config::proxy)).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProxyConfig {
    /// The controller's address on the customer network. A hello from
    /// [`uplink_uid`](Self::uplink_uid) at another address makes this
    /// client join there, keeping the session it has until that succeeds.
    pub controller: Ipv4Addr,
    /// The controller's uid on the customer network, which its hello and
    /// announcement carry: the one
    /// [`on_proxy_controller_changed`](crate::EventHandler::on_proxy_controller_changed)
    /// named. It is pinned, and frames under any other are not taken.
    pub uplink_uid: DeviceUid,
    /// The driver key, as the controller's web interface shows it.
    pub key: [u8; 32],
    /// The port of the proxy, on both sides:
    /// [`PROXY_PORT`](crate::PROXY_PORT) but for tests and development.
    pub port: u16,
}

impl ProxyConfig {
    /// The proxy of the controller `uplink_uid` at `controller`, joined
    /// with `key`, on [`PROXY_PORT`](crate::PROXY_PORT).
    pub fn new(controller: Ipv4Addr, uplink_uid: DeviceUid, key: [u8; 32]) -> Self {
        Self {
            controller,
            uplink_uid,
            key,
            port: PROXY_PORT,
        }
    }

    /// Reads a driver key as the controller's web interface shows it: 64
    /// hex digits. `None` for anything else.
    pub fn parse_key(hex: &str) -> Option<[u8; 32]> {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.is_ascii() {
            return None;
        }
        let mut key = [0u8; 32];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(key)
    }
}

/// How a controller answered a join (`RESULT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProxyResult(pub u8);

impl ProxyResult {
    /// Joined.
    pub const JOINED: Self = Self(0);
    /// The proof was wrong: the driver key is not the controller's.
    pub const BAD_PROOF: Self = Self(1);
    /// The controller's proxy is switched off.
    pub const PROXY_OFF: Self = Self(2);
    /// The controller refuses this client's uid: it is one of the
    /// controller's own, a unit's on its mesh, or another driver's joined
    /// from elsewhere.
    pub const UID_REFUSED: Self = Self(3);
    /// The controller has no session for this client, as after it
    /// restarted.
    pub const NOT_JOINED: Self = Self(4);
    /// The controller speaks another version of the proxy.
    pub const UNKNOWN_VERSION: Self = Self(5);
}

/// Where a client's join to a controller's driver proxy stands
/// ([`EventHandler::on_proxy_join_status`](crate::EventHandler::on_proxy_join_status)).
///
/// While not joined, the client joins again: at once after a session ends,
/// and 10, 20, 40 and then every 60 s after a join to the same address
/// failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProxyJoinStatus {
    /// Joined, the controller having proven the key. The session lasts while
    /// frames keep coming within `lease` each way.
    Joined {
        /// How long the session lasts without a frame.
        lease: Duration,
    },
    /// The controller refused the join, or ended the session.
    Refused(ProxyResult),
    /// The controller did not answer a join within 10 s.
    NoAnswer,
    /// Nothing came from the controller for the lease: the session is over.
    LeaseExpired,
    /// The controller ended the session (`LEAVE`).
    Left,
}

/// A fresh join nonce, `None` when the host has no entropy to give.
fn nonce() -> Option<crypto::JoinNonce> {
    let mut n = [0u8; 16];
    getrandom::getrandom(&mut n).ok()?;
    Some(n)
}

/// The proxy side of a running client: its socket and its link.
pub(super) struct Peer {
    socket: UdpSocket,
    port: u16,
    link: Mutex<Link>,
}

impl Peer {
    /// Opens the proxy port for `config`, as `driver`.
    pub(super) fn open(config: &ProxyConfig, driver: DeviceUid) -> io::Result<Self> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.bind(&SockAddr::from(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            config.port,
        )))?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        Ok(Self {
            socket: socket.into(),
            port: config.port,
            link: Mutex::new(Link::new(
                config.key,
                driver,
                config.uplink_uid,
                config.controller,
            )),
        })
    }

    /// The controller's uid on its mesh, once it has said.
    pub(super) fn mesh_uid(&self) -> Option<DeviceUid> {
        lock(&self.link).mesh_uid()
    }

    /// Whether a session is up.
    pub(super) fn joined(&self) -> bool {
        lock(&self.link).joined()
    }

    /// Ends the session, telling the controller.
    pub(super) fn leave(&self) {
        let leave = lock(&self.link).leave();
        if let Some((address, datagram)) = leave {
            let _ = self
                .socket
                .send_to(&datagram, SocketAddrV4::new(address, self.port));
        }
    }
}

impl Wrap for Peer {
    /// Seals a frame for the controller and sends it there.
    fn send(&self, frame: &[u8]) -> Result<usize, SendError> {
        let sealed = lock(&self.link).seal(frame);
        let (address, datagram) = sealed.ok_or(SendError::NotConnected)?;
        Ok(self
            .socket
            .send_to(&datagram, SocketAddrV4::new(address, self.port))?)
    }
}

impl Shared {
    /// Carries out what the link asked.
    fn carry_out(&self, peer: &Peer, outs: Vec<Out>) {
        let mut joined = false;
        for out in outs {
            match out {
                Out::Send(address, datagram) => {
                    let _ = peer
                        .socket
                        .send_to(&datagram, SocketAddrV4::new(address, peer.port));
                }
                Out::Status(status) => {
                    joined |= matches!(status, ProxyJoinStatus::Joined { .. });
                    self.handler.on_proxy_join_status(status);
                }
                Out::Frame(from, frame) => self.process_datagram_on(&frame, from, None),
            }
        }
        // A new session: the mesh learns of this client and is asked for
        // everything, as on a flat network at start.
        if joined {
            self.announce();
            let _ = self.discover();
        }
    }

    /// Reads the proxy port until the client is closing.
    pub(super) fn proxy_loop(&self, peer: &Peer) {
        let mut buf = vec![0u8; 65535];
        while !self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            match peer.socket.recv_from(&mut buf) {
                Ok((n, std::net::SocketAddr::V4(from))) => {
                    let outs = lock(&peer.link).receive(*from.ip(), &buf[..n], Instant::now());
                    self.carry_out(peer, outs);
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
    }

    /// What the link has due: timeouts, an expired lease, a join again.
    pub(super) fn proxy_tick(&self, peer: &Peer, now: Instant) {
        let outs = lock(&peer.link).tick(now, nonce);
        self.carry_out(peer, outs);
    }

    /// Takes a datagram the mesh ports overheard: only a hello from the
    /// pinned controller, which may say it moved.
    pub(super) fn proxy_overheard(&self, peer: &Peer, data: &[u8], from: Ipv4Addr) {
        if FrameHeader::parse(data).is_none() {
            return;
        }
        let outs = lock(&peer.link).overheard(from, data, Instant::now(), nonce);
        self.carry_out(peer, outs);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod socket_tests;
#[cfg(test)]
mod tests;
