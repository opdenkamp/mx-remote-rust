// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The UDP sockets, and the only writes to them.
//!
//! Each endpoint is one socket serving both directions: bound to its port and
//! either joined to the discovery group or enabled for broadcast, so what this
//! client sends and what it hears come and go by the same interface. A client
//! has one endpoint, or both, as a unit does.

use std::io;
#[cfg(not(target_os = "linux"))]
use std::net::SocketAddr;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;

use socket2::{Domain, InterfaceIndexOrAddress, Protocol, SockAddr, Socket, Type};

use super::constants::{MULTICAST_IP, MULTICAST_PORT};
use super::netif;
use super::tx::Endpoints;

/// How long a receive waits before returning empty-handed.
///
/// The receive thread has no other way to notice that the client is shutting
/// down: it is parked in the kernel until a datagram or this timeout wakes it.
/// Closing the socket underneath it would wake it sooner, but the descriptor
/// can be reissued to something else in the moment between the close and the
/// thread returning, and the read would then land on whatever took it.
pub(super) const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// Multicast hop limit, the one units use, so this client reaches every unit
/// that can reach it.
const MULTICAST_TTL: u32 = 64;

/// Which of the two endpoints a datagram arrived by or leaves by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Path {
    /// The discovery group.
    Multicast,
    /// The subnet broadcast address, or a unicast target in its place.
    Broadcast,
}

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

/// One bound socket, the address it sends to, and the path that is.
#[derive(Debug)]
struct Endpoint {
    socket: UdpSocket,
    target: SocketAddrV4,
    path: Path,
    /// The index of the one interface a datagram is taken from, `None` for
    /// any. Always `None` off Linux, where nothing reads it.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    arrival: Option<u32>,
}

/// The bound endpoints: the one frames go to first, and in dual mode the
/// broadcast endpoint beside the multicast one.
#[derive(Debug)]
pub(crate) struct Conn {
    primary: Endpoint,
    secondary: Option<Endpoint>,
}

/// Which interface the caller selected, by name taking precedence over
/// address, or the host's choice when neither is given.
fn egress_for(local_ip: Option<Ipv4Addr>, interface: Option<&str>) -> io::Result<Egress> {
    Ok(match interface {
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
    })
}

/// The index of the interface the caller selected, by name or by address,
/// which is the only one datagrams are taken from. `None` when neither is
/// given, and off Linux, where nothing reports the interface a datagram
/// arrived on.
///
/// The socket is bound to every address, so without this a multi-homed host
/// hands it every broadcast and group frame on every other interface too.
/// Filtering on the arrival interface needs no privilege, where
/// `SO_BINDTODEVICE` needs `CAP_NET_RAW` on kernels before 5.7.
#[cfg(target_os = "linux")]
fn arrival_for(local_ip: Option<Ipv4Addr>, interface: Option<&str>) -> io::Result<Option<u32>> {
    match (interface, local_ip) {
        (Some(name), _) => netif::index_of(name).map(Some),
        (None, Some(ip)) => netif::index_holding(ip).map(Some),
        (None, None) => Ok(None),
    }
}

#[cfg(not(target_os = "linux"))]
fn arrival_for(_local_ip: Option<Ipv4Addr>, _interface: Option<&str>) -> io::Result<Option<u32>> {
    Ok(None)
}

/// Whether a datagram that arrived on interface `index` is taken by an
/// endpoint taking only `arrival`.
#[cfg(target_os = "linux")]
pub(crate) fn arrived_on(arrival: Option<u32>, index: u32) -> bool {
    arrival.map_or(true, |want| want == index)
}

impl Endpoint {
    fn open(
        target: Ipv4Addr,
        port: u16,
        egress: &Egress,
        arrival: Option<u32>,
    ) -> io::Result<Self> {
        let multicast = target.is_multicast();
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
        // The address each datagram was sent to, which is what tells a frame
        // to the group from one to this host on the same port.
        #[cfg(target_os = "linux")]
        nix::sys::socket::setsockopt(&socket, nix::sys::socket::sockopt::Ipv4PacketInfo, &true)
            .map_err(io::Error::from)?;

        if multicast {
            socket.set_multicast_ttl_v4(MULTICAST_TTL)?;
            match egress {
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
            if let Egress::Interface { name, .. } = egress {
                bind_to_interface(&socket, name)?;
            }
        }

        Ok(Self {
            socket: socket.into(),
            target: SocketAddrV4::new(target, port),
            path: if multicast {
                Path::Multicast
            } else {
                Path::Broadcast
            },
            arrival,
        })
    }

    /// Waits for a datagram, and reports the path it came by: judged by the
    /// address it was sent to where the platform says, and by this
    /// endpoint's own path where it does not.
    #[cfg(not(target_os = "linux"))]
    fn recv<'a>(&self, buf: &'a mut [u8]) -> io::Result<Option<Received<'a>>> {
        match self.socket.recv_from(buf) {
            Ok((n, SocketAddr::V4(from))) => Ok(buf
                .get(..n)
                .map(|data| (data, *from.ip(), path_to(None, self.path)))),
            Ok((_, SocketAddr::V6(_))) => Ok(None),
            Err(e) if is_timeout(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Waits for a datagram, and reports the path it came by, judged by the
    /// address it was sent to as `IP_PKTINFO` gives it.
    ///
    /// A datagram that arrived on an interface other than the selected one is
    /// dropped, and reported as `Ok(None)` as a timeout is.
    #[cfg(target_os = "linux")]
    fn recv<'a>(&self, buf: &'a mut [u8]) -> io::Result<Option<Received<'a>>> {
        use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags, SockaddrIn};
        use std::os::fd::AsRawFd;

        let mut control = nix::cmsg_space!(nix::libc::in_pktinfo);
        let mut iov = [io::IoSliceMut::new(buf)];
        let (n, from, info) = match recvmsg::<SockaddrIn>(
            self.socket.as_raw_fd(),
            &mut iov,
            Some(&mut control),
            MsgFlags::empty(),
        ) {
            Ok(msg) => {
                let info = msg.cmsgs().ok().and_then(|mut c| {
                    c.find_map(|m| match m {
                        ControlMessageOwned::Ipv4PacketInfo(info) => Some((
                            Ipv4Addr::from(u32::from_be(info.ipi_addr.s_addr)),
                            u32::try_from(info.ipi_ifindex).unwrap_or(0),
                        )),
                        _ => None,
                    })
                });
                (msg.bytes, msg.address.map(|a| a.ip()), info)
            }
            Err(e) => {
                let e = io::Error::from(e);
                return if is_timeout(&e) { Ok(None) } else { Err(e) };
            }
        };
        let Some(from) = from else {
            return Ok(None);
        };
        // A selected interface takes nothing whose arrival is unknown.
        let arrived = info.map_or(0, |(_, index)| index);
        if !arrived_on(self.arrival, arrived) {
            return Ok(None);
        }
        let destination = info.map(|(ip, _)| ip);
        Ok(buf
            .get(..n)
            .map(|data| (data, from, path_to(destination, self.path))))
    }
}

/// A datagram, the address it came from, and the path it came by.
pub(crate) type Received<'a> = (&'a [u8], Ipv4Addr, Path);

/// The path a datagram sent to `destination` came by, as a unit judges it.
///
/// A unit counts a frame as multicast by its multicast destination hardware
/// address, which an IPv4 frame carries exactly when it is sent to a
/// multicast group: a frame to a unicast address is not multicast, even on
/// the discovery port. Without a destination, the endpoint's own path stands
/// in for it.
pub(crate) fn path_to(destination: Option<Ipv4Addr>, endpoint: Path) -> Path {
    match destination {
        Some(ip) if ip.is_multicast() => Path::Multicast,
        Some(_) => Path::Broadcast,
        None => endpoint,
    }
}

impl Conn {
    /// Binds the socket, and joins the discovery group when `target` is a
    /// multicast address.
    ///
    /// `interface` selects the interface by name and takes precedence over
    /// `local_ip`, which selects it by address; when neither is given the host
    /// picks, and on Linux datagrams are taken from every interface. The
    /// interface decides both which NIC frames leave by and which one they
    /// are accepted on, and getting it wrong is one-sided: periodic
    /// broadcasts still arrive, so discovery looks healthy while every request
    /// this client sends leaves by the wrong NIC and is never answered.
    pub(crate) fn open(
        target: Ipv4Addr,
        port: u16,
        local_ip: Option<Ipv4Addr>,
        interface: Option<&str>,
    ) -> io::Result<Self> {
        let egress = egress_for(local_ip, interface)?;
        let arrival = arrival_for(local_ip, interface)?;
        Ok(Self {
            primary: Endpoint::open(target, port, &egress, arrival)?,
            secondary: None,
        })
    }

    /// Binds both endpoints a unit has: the discovery group on its port, and
    /// `broadcast` on `broadcast_port`, on the interface selected as
    /// [`Conn::open`] selects it.
    pub(crate) fn open_both(
        broadcast: Ipv4Addr,
        broadcast_port: u16,
        local_ip: Option<Ipv4Addr>,
        interface: Option<&str>,
    ) -> io::Result<Self> {
        let egress = egress_for(local_ip, interface)?;
        let arrival = arrival_for(local_ip, interface)?;
        Ok(Self {
            primary: Endpoint::open(MULTICAST_IP, MULTICAST_PORT, &egress, arrival)?,
            secondary: Some(Endpoint::open(broadcast, broadcast_port, &egress, arrival)?),
        })
    }

    /// Whether both endpoints are bound.
    pub(crate) fn is_dual(&self) -> bool {
        self.secondary.is_some()
    }

    /// Writes a frame to the wire, by the endpoints [`Endpoints`] names: the
    /// group is the first endpoint, and broadcast the second.
    ///
    /// The only write to the protocol endpoints in this library. Every frame
    /// reaches it through [`Tx::send`](super::tx::Tx::send), which is what
    /// makes the protocol-floor check unskippable; a second write anywhere
    /// would bypass that gate rather than merely duplicate this function.
    /// [`Announcer::send`] writes to sockets of its own, addressed to no
    /// device and so behind no floor.
    ///
    /// With one endpoint bound, every frame goes by it.
    pub(super) fn send(&self, data: &[u8], endpoints: Endpoints) -> io::Result<usize> {
        let Some(second) = &self.secondary else {
            return self.primary.socket.send_to(data, self.primary.target);
        };
        match endpoints {
            Endpoints::Multicast => self.primary.socket.send_to(data, self.primary.target),
            Endpoints::Broadcast => second.socket.send_to(data, second.target),
            Endpoints::Both => {
                let sent = self.primary.socket.send_to(data, self.primary.target)?;
                second.socket.send_to(data, second.target)?;
                Ok(sent)
            }
        }
    }

    /// Waits for a datagram on the first endpoint, for at most
    /// [`READ_TIMEOUT`].
    ///
    /// `Ok(None)` is that timeout expiring, which is how the caller gets a
    /// chance to notice it is shutting down. A sender that is not IPv4 has no
    /// address to report, which no MX Remote device does.
    pub(crate) fn recv<'a>(&self, buf: &'a mut [u8]) -> io::Result<Option<Received<'a>>> {
        self.primary.recv(buf)
    }

    /// Waits for a datagram on the second endpoint, as [`Conn::recv`] does
    /// on the first, and reports the path each arrived by. `Ok(None)` at once
    /// when there is no second endpoint.
    pub(crate) fn recv_secondary<'a>(&self, buf: &'a mut [u8]) -> io::Result<Option<Received<'a>>> {
        match &self.secondary {
            Some(second) => second.recv(buf),
            None => Ok(None),
        }
    }

    /// The index of the one interface datagrams are taken from, `None` for
    /// any.
    #[cfg(target_os = "linux")]
    pub(crate) fn arrival(&self) -> Option<u32> {
        self.primary.arrival
    }

    /// The path of the first endpoint and, when bound, the second.
    pub(crate) fn paths(&self) -> (Path, Option<Path>) {
        (self.primary.path, self.secondary.as_ref().map(|e| e.path))
    }

    /// The address frames are sent to first.
    pub(crate) fn target(&self) -> SocketAddrV4 {
        self.primary.target
    }

    /// The address of the second endpoint, when bound.
    pub(crate) fn secondary_target(&self) -> Option<SocketAddrV4> {
        self.secondary.as_ref().map(|e| e.target)
    }

    /// Whether the socket is in a reuseport group, which it must not be.
    ///
    /// Linux-only, because SO_REUSEPORT is: socket2 declares no accessor on
    /// Windows, so naming one there does not compile.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn reuse_port(&self) -> io::Result<bool> {
        socket2::SockRef::from(&self.primary.socket).reuse_port()
    }

    /// The multicast hop limit the socket sends with.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn multicast_ttl(&self) -> io::Result<u32> {
        socket2::SockRef::from(&self.primary.socket).multicast_ttl_v4()
    }

    /// Waits for a datagram for up to `timeout`, restoring the socket's own
    /// timeout afterwards.
    ///
    /// Carried on the one target whose socket tests call it, so that a build
    /// for any other does not warn on it as dead.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn recv_within(&self, timeout: Duration) -> Option<Vec<u8>> {
        self.primary.socket.set_read_timeout(Some(timeout)).ok()?;
        let mut buf = vec![0u8; 2048];
        let got = self
            .recv(&mut buf)
            .ok()
            .flatten()
            .map(|(data, _, _)| data.to_vec());
        let _ = self.primary.socket.set_read_timeout(Some(READ_TIMEOUT));
        got
    }
}

/// Where an [`Announcer`] sends a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    /// This address, a group or a unicast target.
    To(SocketAddrV4),
    /// The directed broadcast address of the interface's current address,
    /// on this port.
    SubnetBroadcast(u16),
}

/// A send-only socket on an interface other than the selected one, which
/// announces this client there and takes nothing in.
///
/// It follows the interface's current address: it rebinds when the address
/// changes, and sends nothing while the interface has none. It is bound to
/// an ephemeral port and never read, so nothing arriving on that interface
/// reaches the registry: the endpoints on the protocol ports drop it, being
/// confined to the selected interface.
#[derive(Debug)]
pub(crate) struct Announcer {
    interface: String,
    bound: Option<(Ipv4Addr, UdpSocket)>,
}

impl Announcer {
    pub(crate) fn new(interface: String) -> Self {
        Self {
            interface,
            bound: None,
        }
    }

    /// The socket for `address`, rebound when the interface's address has
    /// changed since the last send.
    fn socket_for(&mut self, address: Ipv4Addr) -> io::Result<&UdpSocket> {
        if self.bound.as_ref().map(|(at, _)| *at) != Some(address) {
            self.bound = None;
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
            socket.bind(&SockAddr::from(SocketAddrV4::new(address, 0)))?;
            socket.set_broadcast(true)?;
            socket.set_multicast_if_v4(&address)?;
            socket.set_multicast_ttl_v4(MULTICAST_TTL)?;
            self.bound = Some((address, socket.into()));
        }
        match &self.bound {
            Some((_, socket)) => Ok(socket),
            None => unreachable!("bound just above"),
        }
    }

    /// Writes a frame to every place in `reach`, by this interface. Nothing,
    /// and `Ok(0)`, while the interface has no address.
    pub(super) fn send(&mut self, data: &[u8], reach: &[Reach]) -> io::Result<usize> {
        let Some(address) = netif::address_of(&self.interface) else {
            self.bound = None;
            return Ok(0);
        };
        let broadcast = netif::broadcast_address(Some(address))?;
        let socket = self.socket_for(address)?;
        let mut sent = 0;
        for r in reach {
            let to = match *r {
                Reach::To(to) => to,
                Reach::SubnetBroadcast(port) => SocketAddrV4::new(broadcast, port),
            };
            sent = socket.send_to(data, to)?;
        }
        Ok(sent)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame sent to a group came by multicast, one sent to a unicast or
    /// broadcast address by broadcast, whichever endpoint took it; without a
    /// destination the endpoint decides.
    #[test]
    fn the_path_is_the_destination_s() {
        for endpoint in [Path::Multicast, Path::Broadcast] {
            let to = |ip: [u8; 4]| path_to(Some(Ipv4Addr::from(ip)), endpoint);
            assert_eq!(to([224, 8, 8, 8]), Path::Multicast);
            assert_eq!(to([239, 255, 255, 250]), Path::Multicast);
            assert_eq!(to([10, 8, 8, 1]), Path::Broadcast, "unicast");
            assert_eq!(to([10, 8, 8, 255]), Path::Broadcast);
            assert_eq!(to([255, 255, 255, 255]), Path::Broadcast);
            assert_eq!(path_to(None, endpoint), endpoint);
        }
    }

    /// An interface with no address is sent nothing, and that is no error:
    /// it is how an interface waiting on DHCP looks.
    #[test]
    fn an_interface_without_an_address_is_skipped() {
        let mut announcer = Announcer::new("mxr-absent0".to_owned());
        let reach = [Reach::To(SocketAddrV4::new(MULTICAST_IP, MULTICAST_PORT))];
        assert_eq!(announcer.send(b"ping", &reach).ok(), Some(0));
        assert!(announcer.bound.is_none());
    }
}
