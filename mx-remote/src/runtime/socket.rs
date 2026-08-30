// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! What only a real socket can answer: that two clients on one host both get
//! every frame, that an announcement reaches the wire rather than merely
//! passing the gate, and that what arrives on the wire reaches the registry.
//!
//! A scratch group and port throughout, deliberately not the MX Remote ones, so
//! these never put a frame on a live AV network. Each test gets a group of its
//! own as well as a port: membership is a property of the interface, so a group
//! another test still holds is delivered to every socket bound to the port
//! whether this one joined it or not.
//!
//! Every test here proves the host with sockets this library did not build
//! before it asks anything of one it did. That is what separates "this host
//! does not loop multicast back" from "the library never sent, never joined or
//! never decoded", and the second is the only one these tests exist to find.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use socket2::{Domain, SockAddr, Socket, Type};

use crate::testing::uid_n;
use crate::wire::{op, HEADER_LEN};

use super::schedule::{HELLO_BASE, HELLO_JITTER};
use super::tests::hello_datagram;
use super::*;

/// A group and a port, unique to one test. See the module documentation for
/// why the group cannot be shared.
#[derive(Clone, Copy)]
struct Scratch {
    group: Ipv4Addr,
    port: u16,
}

const FANOUT: Scratch = Scratch {
    group: Ipv4Addr::new(239, 255, 77, 99),
    port: 18812,
};
const TO_THE_WIRE: Scratch = Scratch {
    group: Ipv4Addr::new(239, 255, 77, 98),
    port: 18813,
};
const RE_ARM: Scratch = Scratch {
    group: Ipv4Addr::new(239, 255, 77, 97),
    port: 18814,
};
const FROM_THE_WIRE: Scratch = Scratch {
    group: Ipv4Addr::new(239, 255, 77, 96),
    port: 18815,
};
const UNBLOCKED: Scratch = Scratch {
    group: Ipv4Addr::new(239, 255, 77, 95),
    port: 18816,
};
/// Where the host is proved for the test that then asks something of a group
/// nothing else has joined. Looping multicast back is a property of the
/// interface rather than of one group, so proving it here settles it there.
const LOOPBACK_PROOF: Scratch = Scratch {
    group: Ipv4Addr::new(239, 255, 77, 94),
    port: 18817,
};

/// How long a test waits for a datagram it expects to arrive.
const DELIVERY: Duration = Duration::from_secs(2);

/// What the probe carries. Not a frame: nothing under test should decode it.
const PING: &[u8] = b"ping";

fn conn(at: Scratch) -> Option<Conn> {
    Conn::open(at.group, at.port, None, None).ok()
}

/// Reports that the host could not answer the question, which the harness has
/// no way to say for itself.
///
/// Every early return here is one of these, and every one is about the host.
/// Once the probe has landed, silence is the library's and is asserted on.
fn skipped(why: &str) {
    eprintln!("skipped: {why}");
}

fn opcode_of(frame: &[u8]) -> Option<u16> {
    frame
        .get(20..22)
        .and_then(|b| <[u8; 2]>::try_from(b).ok())
        .map(u16::from_le_bytes)
}

/// A sender and a listener that owe this library nothing.
///
/// The listener joins the group itself, so between them they answer one
/// question: does this host loop multicast back on this interface? A library
/// that sent nothing, joined nothing or decoded nothing would otherwise be able
/// to hide behind a negative answer.
struct Probe {
    sender: UdpSocket,
    listener: UdpSocket,
    group: SocketAddrV4,
}

/// A multicast sender pinned to the interface the library will use, so what it
/// sends reaches the same place the library is listening.
fn sender(at: Scratch) -> Option<(UdpSocket, SocketAddrV4)> {
    let local = crate::wire::default_local_ip().ok()?;
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None).ok()?;
    socket
        .bind(&SockAddr::from(SocketAddrV4::new(local, 0)))
        .ok()?;
    socket.set_multicast_if_v4(&local).ok()?;
    socket.set_multicast_loop_v4(true).ok()?;
    Some((socket.into(), SocketAddrV4::new(at.group, at.port)))
}

impl Probe {
    fn open(at: Scratch) -> Option<Self> {
        let local = crate::wire::default_local_ip().ok()?;
        let (sender, group) = sender(at)?;

        let listener = Socket::new(Domain::IPV4, Type::DGRAM, None).ok()?;
        listener.set_reuse_address(true).ok()?;
        listener
            .bind(&SockAddr::from(SocketAddrV4::new(
                Ipv4Addr::UNSPECIFIED,
                at.port,
            )))
            .ok()?;
        listener.join_multicast_v4(&at.group, &local).ok()?;
        listener.set_read_timeout(Some(DELIVERY)).ok()?;

        Some(Self {
            sender,
            listener: listener.into(),
            group,
        })
    }

    fn send(&self, data: &[u8]) -> bool {
        self.sender.send_to(data, self.group).is_ok()
    }

    /// Reads one datagram, or `None` once nothing has arrived for [`DELIVERY`].
    fn recv(&self) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; 2048];
        let (n, _) = self.listener.recv_from(&mut buf).ok()?;
        buf.truncate(n);
        Some(buf)
    }

    /// Whether this host loops multicast back on this interface.
    fn host_loops_back(&self) -> bool {
        self.send(PING) && self.recv().is_some()
    }
}

/// Guards the receive path against `SO_REUSEPORT`.
///
/// `SO_REUSEADDR` gives every socket bound to the same address a copy of each
/// datagram. `SO_REUSEPORT` instead puts them in a reuseport group and hashes
/// each datagram to exactly one member, multicast included - so a second client
/// on the same host would silently take half of this one's frames.
#[test]
fn two_clients_on_one_host_each_get_every_frame() {
    const SENT: usize = 20;

    let Some(a) = conn(FANOUT) else {
        return skipped("no usable multicast interface");
    };
    let b = conn(FANOUT).expect("a second client could not bind alongside the first");

    for (which, c) in [("first", &a), ("second", &b)] {
        assert_eq!(
            c.reuse_port().ok(),
            Some(false),
            "the {which} socket is in a reuseport group: receive would be split \
             with any other client on this host"
        );
    }

    let Some(probe) = Probe::open(FANOUT) else {
        return skipped("the probe cannot be pinned to a multicast interface");
    };
    if !probe.host_loops_back() {
        return skipped("no multicast loopback delivery");
    }

    // The probe's own ping is already in both buffers; it is not one of the
    // twenty being counted.
    for c in [&a, &b] {
        while c.recv_within(Duration::from_millis(50)).is_some() {}
    }

    let readers: Vec<_> = [a, b]
        .into_iter()
        .map(|c| {
            std::thread::spawn(move || {
                let mut got = 0;
                while c.recv_within(Duration::from_millis(700)).is_some() {
                    got += 1;
                }
                got
            })
        })
        .collect();

    for _ in 0..SENT {
        assert!(
            probe.send(PING),
            "the probe reached its own listener, so this send must go too"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let counts: Vec<usize> = readers
        .into_iter()
        .map(|r| r.join().expect("a reader thread panicked"))
        .collect();
    assert!(
        counts.iter().all(|&n| n == SENT),
        "receive is partitioned rather than fanned out: {counts:?} of {SENT} sent"
    );
}

/// The last layer of the send path: that an announcement actually reaches the
/// socket.
///
/// The transmit tap sits inside the send, after the protocol gate and before
/// the write, so it proves what would be sent and not that anything was.
/// Between the tap and the wire sit a missing-socket check and the socket call,
/// and "reported success for a frame never written" is a real failure - the
/// Python port had exactly that, from a length comparison that could never
/// match.
#[test]
fn an_announcement_reaches_the_wire() {
    let Some(probe) = Probe::open(TO_THE_WIRE) else {
        return skipped("the probe cannot be pinned to a multicast interface");
    };
    if !probe.host_loops_back() {
        return skipped("no multicast loopback delivery");
    }

    let remote = Remote::new(
        Config {
            uid: Some(uid_n(210)),
            target_ip: Some(TO_THE_WIRE.group),
            port: Some(TO_THE_WIRE.port),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    remote
        .start()
        .expect("the client could not open its socket");

    // Bounded by a deadline rather than by silence: the client keeps
    // announcing and re-discovering, so a run that never sees a hello would
    // otherwise never stop hearing anything either.
    let deadline = Instant::now() + DELIVERY;
    let mut seen = Vec::new();
    while Instant::now() < deadline && !seen.contains(&op::SYS_HELLO.0) {
        let Some(frame) = probe.recv() else { break };
        assert!(
            frame.len() >= HEADER_LEN,
            "received {} bytes, too short for a frame",
            frame.len()
        );
        seen.extend(opcode_of(&frame));
    }
    remote.close();

    assert!(
        seen.contains(&op::SYS_HELLO.0),
        "the client started but nothing it announced reached the socket; saw {seen:x?}"
    );
}

/// The last layer of the receive path: that a datagram on the wire reaches the
/// registry.
///
/// Every other test enters at `process_datagram`, one level below the receive
/// thread. Without this, deleting the decode from that loop - or the group join
/// from the socket - leaves the whole suite green, because the pieces work and
/// the client hears nothing. It is the only test that asks for delivery on a
/// group it has not joined itself, which is what leaves the client's own join
/// as the thing being tested.
#[test]
fn a_frame_on_the_wire_reaches_the_registry() {
    let Some(probe) = Probe::open(LOOPBACK_PROOF) else {
        return skipped("the probe cannot be pinned to a multicast interface");
    };
    if !probe.host_loops_back() {
        return skipped("no multicast loopback delivery");
    }
    // On its own group, where the client's socket is the only member: a
    // listener of this test's own would join the group on the interface and
    // deliver to the client whether the client joined or not.
    let (sender, group) = sender(FROM_THE_WIRE).expect("the probe was pinned, so this pins too");

    let remote = Remote::new(
        Config {
            uid: Some(uid_n(230)),
            target_ip: Some(FROM_THE_WIRE.group),
            port: Some(FROM_THE_WIRE.port),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    remote
        .start()
        .expect("the client could not open its socket");

    let peer = uid_n(231);
    let hello = hello_datagram(peer, "Peer", "WR0001");
    let deadline = Instant::now() + DELIVERY;
    let mut seen = None;
    while Instant::now() < deadline && seen.is_none() {
        sender
            .send_to(&hello, group)
            .expect("the probe reached its own listener, so this send must go too");
        std::thread::sleep(Duration::from_millis(50));
        seen = remote.device(peer);
    }
    remote.close();

    let device = seen.expect(
        "this host loops multicast back, so the frame was either never delivered to \
         the client's socket or never decoded",
    );
    assert_eq!(device.serial, "WR0001");
}

/// The other direction of the re-arm: a send that succeeds must consume the
/// interval, or the probe loop would announce on every tick. Needs a real
/// socket, since the failure path is what a client without one exercises.
#[test]
fn a_successful_announcement_re_arms_the_timer() {
    let Some(conn) = conn(RE_ARM) else {
        return skipped("no usable multicast interface");
    };
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(220)),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    lock(&remote.shared.tx).set_conn(Some(conn));
    lock(&remote.shared.schedule).set_hello_timer(None, Duration::ZERO);

    remote.shared.announce();

    let (last, interval) = lock(&remote.shared.schedule).hello_timer();
    assert!(
        last.is_some_and(|t| t <= Instant::now()),
        "a successful send did not record the announcement"
    );
    assert!(
        (HELLO_BASE..=HELLO_BASE + HELLO_JITTER).contains(&interval),
        "interval after a successful send = {interval:?}, want it re-drawn in range"
    );
}

/// Sending must not wait on the receive thread.
///
/// That thread spends nearly all of its time parked in the kernel, waiting out
/// a read timeout. Were it to hold the transmit lock while it waited, every
/// send would queue behind it for up to that timeout - which no test of what
/// reaches the wire would notice, because everything still arrives, late.
#[test]
fn a_send_does_not_wait_for_the_receive_thread() {
    const SENDS: u32 = 10;
    /// Well under one read timeout, and far under the many that a send
    /// queueing behind each read would cost.
    const BUDGET: Duration = Duration::from_millis(500);

    let Some(conn) = conn(UNBLOCKED) else {
        return skipped("no usable multicast interface");
    };
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(240)),
            target_ip: Some(UNBLOCKED.group),
            port: Some(UNBLOCKED.port),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    lock(&remote.shared.tx).set_conn(Some(conn));
    remote
        .spawn_workers()
        .expect("the worker threads could not start");
    // Long enough for the receive thread to reach the read it parks in.
    std::thread::sleep(Duration::from_millis(50));

    let started = Instant::now();
    for _ in 0..SENDS {
        remote.discover().expect("the socket is open");
    }
    let elapsed = started.elapsed();
    remote.close();

    assert!(
        elapsed < BUDGET,
        "{SENDS} sends took {elapsed:?}, so each one waited on the receive thread"
    );
}
