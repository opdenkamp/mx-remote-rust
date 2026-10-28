// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The link layer under the UDP endpoints: what a frame's Ethernet and IP
//! headers say that a UDP socket does not, its source hardware address.
//!
//! A unit stores the source MAC address of every hello it takes beside the
//! hello itself. A UDP socket never sees it, so this reads it from a copy of
//! each frame taken below the IP stack. Nothing here sends.

use std::io;

/// The Ethernet type of IPv4.
const ETHERTYPE_IPV4: u16 = 0x0800;

/// The Ethernet types of a VLAN tag, 802.1Q and 802.1ad, either of which
/// can carry the other.
const ETHERTYPE_VLAN: [u16; 2] = [0x8100, 0x88A8];

/// How many VLAN tags a frame is read through.
const VLAN_TAGS_MAX: usize = 2;

/// The IP protocol number of UDP.
const IP_PROTOCOL_UDP: u8 = 17;

/// The bytes of a VLAN tag.
const VLAN_TAG: usize = 4;

/// The bytes of a UDP header.
const UDP_HEADER: usize = 8;

/// One UDP datagram as it was framed on the link.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LinkDatagram<'a> {
    /// The source hardware address of the frame carrying it.
    pub(crate) source: [u8; 6],
    /// The UDP port it was sent to.
    pub(crate) port: u16,
    /// The UDP payload.
    pub(crate) payload: &'a [u8],
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let bytes = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]))
}

/// Reads the UDP datagram an Ethernet frame carries over IPv4, through up to
/// two VLAN tags.
///
/// A fragment is not a datagram: no unit fragments a frame, so one arriving
/// in pieces did not come from one. The payload is bounded by the IP and UDP
/// lengths, so the padding a short frame is sent with is not read as part of
/// it.
pub(crate) fn udp_in_ethernet(frame: &[u8]) -> Option<LinkDatagram<'_>> {
    let source = <[u8; 6]>::try_from(frame.get(6..12)?).ok()?;
    let mut at = 12;
    let mut ethertype = u16_at(frame, at)?;
    for _ in 0..VLAN_TAGS_MAX {
        if !ETHERTYPE_VLAN.contains(&ethertype) {
            break;
        }
        at += VLAN_TAG;
        ethertype = u16_at(frame, at)?;
    }
    if ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = frame.get(at + 2..)?;
    let first = *ip.first()?;
    let header = usize::from(first & 0x0F) * 4;
    if first >> 4 != 4 || header < 20 {
        return None;
    }
    let total = usize::from(u16_at(ip, 2)?);
    let ip = ip.get(..total)?;
    // More fragments, or an offset: a piece of a datagram.
    if u16_at(ip, 6)? & 0x3FFF != 0 || *ip.get(9)? != IP_PROTOCOL_UDP {
        return None;
    }
    let udp = ip.get(header..)?;
    let port = u16_at(udp, 2)?;
    let length = usize::from(u16_at(udp, 4)?);
    let payload = udp.get(UDP_HEADER..length)?;
    Some(LinkDatagram {
        source,
        port,
        payload,
    })
}

/// A copy of every IPv4 frame the host receives or sends, read below the IP
/// stack. Linux only, and it needs `CAP_NET_RAW`.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub(crate) struct Capture {
    socket: socket2::Socket,
}

#[cfg(target_os = "linux")]
impl Capture {
    /// Opens a packet socket taking every IPv4 frame on every interface.
    ///
    /// It reads, and never sends. Each read waits at most as long as a UDP
    /// endpoint's does, so the reading thread can notice it is shutting down.
    pub(crate) fn open() -> io::Result<Self> {
        use socket2::{Domain, Protocol, Socket, Type};
        // The protocol is the Ethernet type in network byte order.
        let protocol = Protocol::from(i32::from(ETHERTYPE_IPV4.to_be()));
        let socket = Socket::new(Domain::PACKET, Type::RAW, Some(protocol))?;
        socket.set_read_timeout(Some(super::conn::READ_TIMEOUT))?;
        Ok(Self { socket })
    }

    /// Waits for one frame. `Ok(None)` is the timeout expiring.
    pub(crate) fn recv<'a>(&self, buf: &'a mut [u8]) -> io::Result<Option<&'a [u8]>> {
        use std::io::Read;
        match (&self.socket).read(buf) {
            Ok(n) => Ok(buf.get(..n)),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

/// No packet socket off Linux.
#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
pub(crate) struct Capture;

#[cfg(not(target_os = "linux"))]
impl Capture {
    /// Refuses: reading below the IP stack is Linux-only here.
    pub(crate) fn open() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "capturing a hello's source address needs Linux",
        ))
    }

    /// Never called, since no capture opens.
    pub(crate) fn recv<'a>(&self, _buf: &'a mut [u8]) -> io::Result<Option<&'a [u8]>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::poisoned;

    /// An Ethernet frame carrying `payload` from port 0xC350 to `port` over
    /// IPv4, with `tags` VLAN tags, IP options of `options` bytes and poisoned
    /// padding behind it.
    ///
    /// Composed from the Ethernet, IPv4 and UDP layouts alongside the reading
    /// it checks, not captured: it rules out a slip, not a shared misreading.
    fn ethernet(tags: &[u16], options: usize, flags: u16, port: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0x01, 0x00, 0x5E, 0x08, 0x08, 0x08];
        f.extend_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        for (i, tag) in tags.iter().enumerate() {
            f.extend_from_slice(&tag.to_be_bytes());
            f.extend_from_slice(&[0x60, 0x0A + i as u8]);
        }
        f.extend_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
        let header = 20 + options;
        let total = header + UDP_HEADER + payload.len();
        let mut ip = poisoned(header);
        ip[0] = 0x40 | (header / 4) as u8;
        ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        ip[6..8].copy_from_slice(&flags.to_be_bytes());
        ip[9] = IP_PROTOCOL_UDP;
        f.extend_from_slice(&ip);
        f.extend_from_slice(&0xC350u16.to_be_bytes());
        f.extend_from_slice(&port.to_be_bytes());
        f.extend_from_slice(&((UDP_HEADER + payload.len()) as u16).to_be_bytes());
        f.extend_from_slice(&[0xA5, 0x5A]);
        f.extend_from_slice(payload);
        f.extend_from_slice(&poisoned(12));
        f
    }

    const SOURCE: [u8; 6] = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];

    /// The source address, the destination port and the payload are read, up
    /// to the payload's own end, whatever tags and options come before it.
    #[test]
    fn a_datagram_is_read_with_its_source_address() {
        let payload = b"P8 a datagram";
        for (tags, options) in [
            (&[][..], 0),
            (&[0x8100][..], 0),
            (&[0x88A8, 0x8100][..], 0),
            (&[][..], 8),
        ] {
            let frame = ethernet(tags, options, 0x4000, 8812, payload);
            assert_eq!(
                udp_in_ethernet(&frame),
                Some(LinkDatagram {
                    source: SOURCE,
                    port: 8812,
                    payload,
                }),
                "{} tags, {options} bytes of options",
                tags.len()
            );
        }
    }

    /// The UDP length ends the payload, and the IP length ends what the UDP
    /// length may claim: neither reads on into what follows.
    #[test]
    fn the_payload_ends_where_its_lengths_say() {
        let payload = b"P8 a datagram";
        // Behind a 14-byte Ethernet header and a 20-byte IP header.
        let udp_length = 14 + 20 + 4;

        let mut shorter = ethernet(&[], 0, 0, 8812, payload);
        shorter[udp_length..udp_length + 2].copy_from_slice(&(8u16 + 4).to_be_bytes());
        assert_eq!(
            udp_in_ethernet(&shorter).map(|d| d.payload),
            Some(&payload[..4]),
            "read past the UDP length"
        );

        let mut longer = ethernet(&[], 0, 0, 8812, payload);
        let claimed = (8 + payload.len() + 4) as u16;
        longer[udp_length..udp_length + 2].copy_from_slice(&claimed.to_be_bytes());
        assert_eq!(udp_in_ethernet(&longer), None, "read past the IP length");
    }

    /// A frame typed IPv4 whose header says another version is not read.
    #[test]
    fn an_ip_header_of_another_version_is_not_read() {
        let mut frame = ethernet(&[], 0, 0, 8812, b"P8 a datagram");
        frame[14] = 0x65;
        assert_eq!(udp_in_ethernet(&frame), None);
    }

    /// A fragment, a protocol other than UDP or IPv4, or a frame cut short is
    /// not read.
    #[test]
    fn what_is_not_a_whole_udp_datagram_is_not_read() {
        let payload = b"P8 a datagram";
        for flags in [0x2000, 0x0001, 0x6000] {
            let frame = ethernet(&[], 0, flags, 8812, payload);
            assert_eq!(udp_in_ethernet(&frame), None, "flags {flags:#06x}");
        }
        let mut tcp = ethernet(&[], 0, 0, 8812, payload);
        tcp[14 + 9] = 6;
        assert_eq!(udp_in_ethernet(&tcp), None);
        let mut ipv6 = ethernet(&[], 0, 0, 8812, payload);
        ipv6[12..14].copy_from_slice(&0x86DDu16.to_be_bytes());
        assert_eq!(udp_in_ethernet(&ipv6), None);
        let three = ethernet(&[0x8100, 0x8100, 0x8100], 0, 0, 8812, payload);
        assert_eq!(udp_in_ethernet(&three), None, "read through three tags");
        let whole = ethernet(&[], 0, 0, 8812, payload);
        let end = whole.len() - 12;
        for cut in 0..end {
            assert_eq!(udp_in_ethernet(&whole[..cut]), None, "cut at {cut}");
        }
    }
}
