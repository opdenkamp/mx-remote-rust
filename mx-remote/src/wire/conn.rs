// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The UDP socket, and the only write to it.
//!
//! One socket serves both directions: it is bound to the MX Remote port and
//! either joined to the discovery group or enabled for broadcast, so what this
//! client sends and what it hears come and go by the same interface.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

use socket2::{Domain, InterfaceIndexOrAddress, Protocol, SockAddr, Socket, Type};

use super::netif;

/// How long a receive waits before returning empty-handed.
///
/// The receive thread has no other way to notice that the client is shutting
/// down: it is parked in the kernel until a datagram or this timeout wakes it.
/// Closing the socket underneath it would wake it sooner, but the descriptor
/// can be reissued to something else in the moment between the close and the
/// thread returning, and the read would then land on whatever took it.
const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// Multicast hop limit.
///
/// Discovery is meant to reach an AV installation, which can span a couple of
/// switches, and to stop there.
const MULTICAST_TTL: u32 = 3;

/// Which interface carries the traffic.
///
/// The two are not interchangeable. An address names the interface indirectly
/// and is what every platform accepts. An index names the interface itself, so
/// it reaches one with no address of its own - a tagged VLAN - but pinning
/// sends to it needs `SO_BINDTODEVICE`, which is Linux-only.
#[derive(Clone, Debug)]
enum Egress {
    Address(Ipv4Addr),
    Interface { name: String, index: u32 },
}

/// The bound socket and the address it sends to.
#[derive(Debug)]
pub(crate) struct Conn {
    socket: UdpSocket,
    target: SocketAddrV4,
}

impl Conn {
    /// Binds the socket, and joins the discovery group when `target` is a
    /// multicast address.
    ///
    /// `interface` selects the interface by name and takes precedence over
    /// `local_ip`, which selects it by address; when neither is given the host
    /// picks. The interface decides both which NIC frames leave by and which
    /// one they are accepted on, and getting it wrong is one-sided: periodic
    /// broadcasts still arrive, so discovery looks healthy while every request
    /// this client sends leaves by the wrong NIC and is never answered.
    pub(crate) fn open(
        target: Ipv4Addr,
        port: u16,
        local_ip: Option<Ipv4Addr>,
        interface: Option<&str>,
    ) -> io::Result<Self> {
        let multicast = target.is_multicast();
        let egress = match interface {
            // An address is preferred even when the caller named an interface,
            // because naming one by index is the platform-restricted path; the
            // index is the fallback an addressless interface has to take.
            Some(name) => match netif::address_of(name) {
                Some(ip) => Egress::Address(ip),
                None => Egress::Interface {
                    name: name.to_owned(),
                    index: netif::index_of(name)?,
                },
            },
            None => Egress::Address(match local_ip {
                Some(ip) => ip,
                None => netif::default_local_ip()?,
            }),
        };

        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        // Every client on the host must get its own copy of each datagram.
        // SO_REUSEADDR is what allows the second bind and still fans out;
        // SO_REUSEPORT would instead hash each datagram to one member of the
        // group, so a second client would silently take half of this one's
        // frames. It is deliberately not set.
        socket.set_reuse_address(true)?;
        socket.bind(&SockAddr::from(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            port,
        )))?;
        socket.set_read_timeout(Some(READ_TIMEOUT))?;

        if multicast {
            socket.set_multicast_ttl_v4(MULTICAST_TTL)?;
            match &egress {
                Egress::Address(ip) => {
                    socket.set_multicast_if_v4(ip)?;
                    socket.join_multicast_v4(&target, ip)?;
                }
                Egress::Interface { name, index } => {
                    socket.join_multicast_v4_n(&target, &InterfaceIndexOrAddress::Index(*index))?;
                    bind_to_interface(&socket, name)?;
                }
            }
        } else {
            socket.set_broadcast(true)?;
            if let Egress::Interface { name, .. } = &egress {
                bind_to_interface(&socket, name)?;
            }
        }

        Ok(Self {
            socket: socket.into(),
            target: SocketAddrV4::new(target, port),
        })
    }

    /// Writes a frame to the wire.
    ///
    /// The only call to the socket in this library. Every frame reaches it
    /// through [`Tx::send`](super::tx::Tx::send), which is what makes the
    /// protocol-floor check unskippable; a second write anywhere would bypass
    /// that gate rather than merely duplicate this function.
    pub(super) fn send(&self, data: &[u8]) -> io::Result<usize> {
        self.socket.send_to(data, self.target)
    }

    /// Waits for a datagram, for at most [`READ_TIMEOUT`].
    ///
    /// `Ok(None)` is that timeout expiring, which is how the caller gets a
    /// chance to notice it is shutting down. A sender that is not IPv4 has no
    /// address to report, which no MX Remote device does.
    pub(crate) fn recv<'a>(&self, buf: &'a mut [u8]) -> io::Result<Option<(&'a [u8], Ipv4Addr)>> {
        match self.socket.recv_from(buf) {
            Ok((n, SocketAddr::V4(from))) => Ok(buf.get(..n).map(|data| (data, *from.ip()))),
            Ok((_, SocketAddr::V6(_))) => Ok(None),
            Err(e) if is_timeout(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The address frames are sent to.
    pub(crate) fn target(&self) -> SocketAddrV4 {
        self.target
    }

    /// Whether the socket is in a reuseport group, which it must not be.
    #[cfg(test)]
    pub(crate) fn reuse_port(&self) -> io::Result<bool> {
        socket2::SockRef::from(&self.socket).reuse_port()
    }

    /// Waits for a datagram for up to `timeout`, restoring the socket's own
    /// timeout afterwards.
    #[cfg(test)]
    pub(crate) fn recv_within(&self, timeout: Duration) -> Option<Vec<u8>> {
        self.socket.set_read_timeout(Some(timeout)).ok()?;
        let mut buf = vec![0u8; 2048];
        let got = self
            .recv(&mut buf)
            .ok()
            .flatten()
            .map(|(data, _)| data.to_vec());
        let _ = self.socket.set_read_timeout(Some(READ_TIMEOUT));
        got
    }
}

/// Pins the socket to one interface, for an interface that has no address to
/// select it by.
///
/// The multicast join can name an interface by index, but choosing which
/// interface a send leaves by cannot: that is an address, or `SO_BINDTODEVICE`.
/// A failure here is reported rather than absorbed - an unpinned socket sends
/// by whichever interface the route table prefers, which is the outcome naming
/// an interface was meant to rule out.
#[cfg(any(target_os = "android", target_os = "linux"))]
fn bind_to_interface(socket: &Socket, name: &str) -> io::Result<()> {
    socket.bind_device(Some(name.as_bytes()))
}

/// Refuses an interface with no address of its own, which only Linux can pin a
/// socket to.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn bind_to_interface(_socket: &Socket, name: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("interface {name:?} has no IPv4 address, and only Linux can select an interface without one"),
    ))
}

/// Whether a receive failed only because nothing arrived in time.
///
/// The two kinds are one condition under two names: a Unix socket reports the
/// expired `SO_RCVTIMEO` as `EAGAIN` and a Windows one as `WSAETIMEDOUT`.
fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}
