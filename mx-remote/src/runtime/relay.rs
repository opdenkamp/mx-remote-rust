// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The mesh as whole frames: what a caller relaying it elsewhere is handed,
//! and what it puts back.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::raw::{Direction, Endpoint, FrameHeader, RawEndpoints};
use crate::wire::{host_addresses, Endpoints, Path, SendError, Sent};

use super::{lock, Remote, Shared};

/// How long the host's addresses are taken as read before they are read
/// again. An address the host takes on is missed for this long at most.
const OWN_ADDRESSES_FOR: Duration = Duration::from_secs(1);

/// This host's addresses, as last read.
#[derive(Debug, Default)]
pub(super) struct OwnAddresses {
    addresses: Vec<Ipv4Addr>,
    /// Until when they stand, `None` before the first read.
    until: Option<Instant>,
}

impl OwnAddresses {
    /// Whether `ip` is one of this host's addresses, reading them again with
    /// `read` once they are [`OWN_ADDRESSES_FOR`] old.
    fn holds(&mut self, ip: Ipv4Addr, now: Instant, read: impl FnOnce() -> Vec<Ipv4Addr>) -> bool {
        if self.until.map_or(true, |until| now >= until) {
            self.addresses = read();
            self.until = Some(now + OWN_ADDRESSES_FOR);
        }
        self.addresses.contains(&ip)
    }

    /// Takes `addresses` as the host's until `until`.
    #[cfg(test)]
    pub(super) fn set(&mut self, addresses: Vec<Ipv4Addr>, until: Instant) {
        self.addresses = addresses;
        self.until = Some(until);
    }
}

fn endpoint(path: Path) -> Endpoint {
    match path {
        Path::Multicast => Endpoint::Group,
        Path::Broadcast => Endpoint::Broadcast,
    }
}

impl Shared {
    /// Hands a frame this client wrote to the mesh to the handler, once for
    /// each endpoint it went by.
    pub(super) fn tap_sent(&self, sent: &Sent) {
        for path in &sent.paths {
            self.handler
                .on_mesh_frame(&sent.frame, Direction::Sent, endpoint(*path));
        }
    }

    /// Hands a datagram a mesh endpoint received to the handler, when a unit
    /// would take it and no address of this host sent it.
    pub(super) fn tap_received(&self, data: &[u8], from: Ipv4Addr, by: Path) {
        if FrameHeader::parse_accepted(data).is_none() {
            return;
        }
        if lock(&self.own_addresses).holds(from, Instant::now(), host_addresses) {
            return;
        }
        self.handler
            .on_mesh_frame(data, Direction::Received, endpoint(by));
    }
}

impl Remote {
    /// Puts a frame built elsewhere onto the mesh, whole and unchanged, from
    /// this client's mesh endpoints: by both, or by the group alone. Nothing
    /// goes to an announce interface
    /// ([`Config::announce_interfaces`](crate::Config::announce_interfaces)).
    ///
    /// It must be a frame a unit takes
    /// ([`FrameHeader::parse_accepted`](crate::raw::FrameHeader::parse_accepted))
    /// from a sender other than this client, or it is refused with
    /// [`SendError::InvalidFrame`]: a frame under this client's own uid is
    /// built here, behind the checks the rest of this client's sends pass.
    ///
    /// It is not handed to [`on_mesh_frame`](crate::EventHandler::on_mesh_frame).
    /// The copy the host loops back reaches this client's own receive path
    /// as one from its sender would, so the sender becomes known here as it
    /// does on every unit.
    ///
    /// Returns the bytes the first write took.
    pub fn send_raw(&self, frame: &[u8], to: RawEndpoints) -> Result<usize, SendError> {
        let header = FrameHeader::parse_accepted(frame).ok_or(SendError::InvalidFrame)?;
        if header.sender == self.shared.uid {
            return Err(SendError::InvalidFrame);
        }
        let endpoints = match to {
            RawEndpoints::Both => Endpoints::Both,
            RawEndpoints::Group => Endpoints::Multicast,
        };
        let sent = lock(&self.shared.tx).send_raw(frame, endpoints)?;
        Ok(sent.bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::runtime::tests::{client_with, hello_datagram};
    use crate::testing::uid_n;
    use crate::wire::{HEADER_LEN, PROTOCOL_VERSION};
    use crate::EventHandler;

    const HOST: Ipv4Addr = Ipv4Addr::new(10, 8, 8, 2);
    const PEER: Ipv4Addr = Ipv4Addr::new(10, 8, 8, 9);

    /// Records every frame handed on.
    #[derive(Default)]
    struct Frames(Mutex<Vec<(Vec<u8>, Direction, Endpoint)>>);

    impl Frames {
        fn taken(&self) -> Vec<(Vec<u8>, Direction, Endpoint)> {
            self.0.lock().expect("only locked to push or read").clone()
        }
    }

    impl EventHandler for Frames {
        fn on_mesh_frame(&self, frame: &[u8], direction: Direction, endpoint: Endpoint) {
            self.0.lock().expect("only locked to push or read").push((
                frame.to_vec(),
                direction,
                endpoint,
            ));
        }
    }

    /// A client on a host whose one address is [`HOST`], and what it hands
    /// on.
    fn relaying(n: u8) -> (Remote, Arc<Frames>, Arc<crate::runtime::tests::Tap>) {
        let frames = Arc::new(Frames::default());
        let (remote, tap) = client_with(n, Arc::clone(&frames) as Arc<dyn EventHandler>);
        lock(&remote.shared.own_addresses)
            .set(vec![HOST], Instant::now() + Duration::from_secs(3600));
        (remote, frames, tap)
    }

    /// A frame a unit takes is handed on as it arrived, with the endpoint it
    /// came in on.
    #[test]
    fn a_frame_from_the_mesh_is_handed_on_as_received() {
        let (remote, frames, _) = relaying(220);
        let hello = hello_datagram(uid_n(221), "unit", "EP0001");
        remote
            .shared
            .receive_on(&hello, PEER, Path::Multicast, None);
        remote
            .shared
            .receive_on(&hello, PEER, Path::Broadcast, None);
        assert_eq!(
            frames.taken(),
            vec![
                (hello.clone(), Direction::Received, Endpoint::Group),
                (hello, Direction::Received, Endpoint::Broadcast),
            ]
        );
    }

    /// A datagram a unit would not take is not handed on: one padded or
    /// cut short, one not starting `P8`, or one stamped past what a unit
    /// reads.
    #[test]
    fn only_a_frame_a_unit_takes_is_handed_on() {
        let (remote, frames, _) = relaying(222);
        let hello = hello_datagram(uid_n(223), "unit", "EP0001");
        let mut padded = hello.clone();
        padded.push(0);
        let cut = hello[..hello.len() - 1].to_vec();
        let mut magic = hello.clone();
        magic[0] = b'Q';
        let mut newer = hello.clone();
        newer[2..4].copy_from_slice(&(PROTOCOL_VERSION + 1).to_le_bytes());
        for data in [padded, cut, magic, newer] {
            remote.shared.receive_on(&data, PEER, Path::Multicast, None);
        }
        assert_eq!(frames.taken(), vec![]);
    }

    /// A frame from an address of this host is not handed on, but is still
    /// decoded: a frame this client relayed makes its sender known here as
    /// on every unit.
    #[test]
    fn a_frame_from_this_host_is_decoded_but_not_handed_on() {
        let (remote, frames, _) = relaying(224);
        let sender = uid_n(225);
        let hello = hello_datagram(sender, "driver", "EP0002");
        remote
            .shared
            .receive_on(&hello, HOST, Path::Multicast, None);
        assert_eq!(frames.taken(), vec![]);
        assert!(remote.device(sender).is_some(), "the frame was not decoded");
    }

    /// Both copies of a frame sent to both endpoints are handed on, though
    /// only the first is decoded.
    #[test]
    fn both_copies_are_handed_on_before_the_second_is_dropped() {
        let (remote, frames, _) = relaying(226);
        let hello = hello_datagram(uid_n(227), "unit", "EP0003");
        remote
            .shared
            .receive_on(&hello, PEER, Path::Multicast, Some(Path::Multicast));
        remote
            .shared
            .receive_on(&hello, PEER, Path::Broadcast, Some(Path::Broadcast));
        assert_eq!(frames.taken().len(), 2);
        assert_eq!(remote.frames_received(), 1, "the second copy was decoded");
    }

    /// A frame this client could not write is not handed on as sent.
    #[test]
    fn a_frame_not_written_is_not_handed_on() {
        let (remote, frames, tap) = relaying(228);
        assert!(remote.discover().is_err(), "a client with no socket sent");
        assert_eq!(
            tap.opcodes().len(),
            1,
            "the discover never reached the gate"
        );
        assert_eq!(frames.taken(), vec![]);
    }

    /// A raw frame goes to the write unchanged, by the endpoints asked for.
    #[test]
    fn a_raw_frame_is_written_unchanged() {
        let (remote, _, tap) = relaying(229);
        let hello = hello_datagram(uid_n(230), "driver", "EP0004");
        for (to, endpoints) in [
            (RawEndpoints::Both, Endpoints::Both),
            (RawEndpoints::Group, Endpoints::Multicast),
        ] {
            tap.clear();
            lock(&remote.shared.tx).sent_on.clear();
            assert!(matches!(
                remote.send_raw(&hello, to),
                Err(SendError::NotConnected)
            ));
            assert_eq!(tap.frames(), vec![hello.clone()], "{to:?}");
            assert_eq!(lock(&remote.shared.tx).sent_on, vec![endpoints], "{to:?}");
        }
    }

    /// A raw frame a unit would not take, or one under this client's own
    /// uid, is refused before the write.
    #[test]
    fn a_raw_frame_a_unit_would_not_take_is_refused() {
        let (remote, _, tap) = relaying(231);
        let mut padded = hello_datagram(uid_n(232), "driver", "EP0005");
        padded.push(0);
        let own = hello_datagram(uid_n(231), "driver", "EP0005");
        let short = own[..HEADER_LEN - 1].to_vec();
        for frame in [padded, own, short] {
            assert!(matches!(
                remote.send_raw(&frame, RawEndpoints::Both),
                Err(SendError::InvalidFrame)
            ));
        }
        assert_eq!(tap.frames(), Vec::<Vec<u8>>::new());
    }

    /// The host's addresses are read on first use, and again only once they
    /// are a second old.
    #[test]
    fn the_hosts_addresses_are_read_at_most_once_a_second() {
        let mut own = OwnAddresses::default();
        let start = Instant::now();
        let mut reads = 0;
        let mut read = |at: Duration, ip: Ipv4Addr| {
            own.holds(ip, start + at, || {
                reads += 1;
                vec![HOST]
            })
        };
        assert!(read(Duration::ZERO, HOST));
        assert!(!read(
            Duration::from_millis(999),
            Ipv4Addr::new(10, 8, 8, 3)
        ));
        assert!(read(OWN_ADDRESSES_FOR, HOST));
        assert_eq!(reads, 2);
    }

    /// An address the host took on is held once the addresses are read
    /// again, and one it gave up no longer is.
    #[test]
    fn the_hosts_addresses_follow_the_host() {
        let mut own = OwnAddresses::default();
        let start = Instant::now();
        let other = Ipv4Addr::new(10, 8, 8, 3);
        assert!(own.holds(HOST, start, || vec![HOST]));
        assert!(own.holds(other, start + OWN_ADDRESSES_FOR, || vec![other]));
        assert!(!own.holds(HOST, start + OWN_ADDRESSES_FOR, || vec![other]));
    }
}
