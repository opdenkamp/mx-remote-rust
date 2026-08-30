// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The runtime: what drives the announcement timer, and what the receive entry
//! point does with a datagram.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::testing::{fixed_str, uid_n};
use crate::wire::{op, protocol_for, DeviceFeature, HEADER_LEN};

use super::schedule::{next_hello_interval, HELLO_BASE, HELLO_JITTER};
use super::*;

/// Records every frame that passes the protocol gate.
///
/// Frames are assembled inside the method that sends them, so this is the only
/// way to read back what the client would put on the wire.
#[derive(Default)]
pub(super) struct Tap(Mutex<Vec<Vec<u8>>>);

impl Tap {
    /// The opcodes captured so far.
    pub(super) fn opcodes(&self) -> Vec<u16> {
        self.0
            .lock()
            .expect("the tap is only ever locked to push or read")
            .iter()
            .filter_map(|f| f.get(20..22))
            .filter_map(|b| <[u8; 2]>::try_from(b).ok())
            .map(u16::from_le_bytes)
            .collect()
    }

    pub(super) fn frames(&self) -> Vec<Vec<u8>> {
        self.0
            .lock()
            .expect("the tap is only ever locked to push or read")
            .clone()
    }

    /// Forgets what was captured, so the next call is read on its own.
    pub(super) fn clear(&self) {
        self.0
            .lock()
            .expect("the tap is only ever locked to push or read")
            .clear();
    }
}

/// A client with no socket, so every send fails after passing the gate.
///
/// That is the interesting case for the timer: the announcement must not be
/// recorded when the frame never left.
fn client(n: u8) -> (Remote, Arc<Tap>) {
    client_with(n, Arc::new(()))
}

/// A client as [`client`] builds one, delivering its events to `handler`.
pub(super) fn client_with(n: u8, handler: Arc<dyn EventHandler>) -> (Remote, Arc<Tap>) {
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(n)),
            ..Config::default()
        },
        handler,
    )
    .expect("a client given its own uid reads nothing from disk");
    let tap = Arc::new(Tap::default());
    let sink = Arc::clone(&tap);
    lock(&remote.shared.tx).set_tap(Arc::new(move |frame: &[u8]| {
        sink.0
            .lock()
            .expect("the tap is only ever locked to push or read")
            .push(frame.to_vec())
    }));
    (remote, tap)
}

/// Assembles a hello datagram the way a device does, header and all.
pub(super) fn hello_datagram(sender: DeviceUid, name: &str, serial: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x28u16.to_le_bytes());
    fixed_str(&mut payload, name, 16);
    fixed_str(&mut payload, serial, 16);
    fixed_str(&mut payload, "4.8.0", 16);
    payload.extend_from_slice(&DeviceFeature::VIDEO_ROUTING.bits().to_le_bytes());

    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(b"P8");
    out.extend_from_slice(&protocol_for(op::SYS_HELLO).to_le_bytes());
    out.extend_from_slice(sender.as_bytes());
    out.extend_from_slice(&op::SYS_HELLO.0.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// `process_datagram` is the real receive entry point; every other test enters
/// one level below it. Testing here is what pins the negative half: hello is
/// announced on a clock, so no amount of arriving traffic may provoke one.
#[test]
fn a_datagram_is_decoded_and_does_not_provoke_an_announcement() {
    let (remote, tap) = client(200);
    let peer = uid_n(201);

    remote.shared.process_datagram(
        &hello_datagram(peer, "Peer", "PR0001"),
        Ipv4Addr::new(10, 8, 8, 9),
    );

    let device = remote.device(peer).expect("the datagram was not processed");
    assert_eq!(device.serial, "PR0001");
    assert_eq!(device.address, Some(Ipv4Addr::new(10, 8, 8, 9)));
    assert!(
        !tap.opcodes().contains(&op::SYS_HELLO.0),
        "a received datagram triggered a hello; announcement is a timer, not a reply"
    );
}

/// A client must not decode its own frames back into the registry.
#[test]
fn a_clients_own_frame_is_not_taken_for_a_peers() {
    let (remote, _) = client(205);
    remote.shared.process_datagram(
        &hello_datagram(remote.uid(), "Self", "SF0001"),
        Ipv4Addr::new(10, 8, 8, 9),
    );
    assert!(
        remote.devices().is_empty(),
        "the client registered itself as a device"
    );
}

/// A device announces itself on a schedule whether or not anything is talking
/// to it. A client that only re-announced on arriving traffic went silent on a
/// quiet network and stayed unknown to every peer that started after it.
#[test]
fn the_announcement_is_driven_by_a_timer() {
    let (remote, _) = client(202);
    let now = Instant::now();
    lock(&remote.shared.schedule).set_hello_timer(Some(now), Duration::from_secs(3));

    assert!(
        !remote.shared.announce_due(now + Duration::from_secs(2)),
        "announced early"
    );
    assert!(
        remote.shared.announce_due(now + Duration::from_secs(3)),
        "not due at the interval; a silent network would never announce"
    );
    assert!(
        remote.shared.announce_due(now + Duration::from_secs(3600)),
        "not due long after the interval"
    );

    remote.shared.closing.store(true, Ordering::SeqCst);
    assert!(
        !remote.shared.announce_due(now + Duration::from_secs(3600)),
        "announced while closing"
    );
}

/// The interval is re-drawn on each send, so a mesh full of clients started
/// together does not stay in step.
#[test]
fn the_announcement_interval_is_jittered() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..200 {
        let interval = next_hello_interval();
        assert!(
            (HELLO_BASE..=HELLO_BASE + HELLO_JITTER).contains(&interval),
            "interval {interval:?} outside {HELLO_BASE:?}..{:?}",
            HELLO_BASE + HELLO_JITTER
        );
        seen.insert(interval);
    }
    assert!(
        seen.len() >= 50,
        "only {} distinct intervals in 200 draws; the jitter is not varying",
        seen.len()
    );
}

/// A send that fails must not consume the interval: the firmware re-arms only
/// inside the branch where the transmit succeeded, so a failure is retried on
/// the next tick rather than costing a whole interval of silence. This client
/// has no socket, so the send fails.
#[test]
fn a_failed_announcement_does_not_consume_the_interval() {
    let (remote, tap) = client(203);
    lock(&remote.shared.schedule).set_hello_timer(None, Duration::ZERO);

    remote.shared.announce();

    assert_eq!(
        tap.opcodes(),
        vec![op::SYS_HELLO.0],
        "the frame did not reach the gate at all, so the timer proves nothing"
    );
    let (last, interval) = lock(&remote.shared.schedule).hello_timer();
    assert_eq!(
        (last, interval),
        (None, Duration::ZERO),
        "a failed send re-armed the timer; it should retry on the next tick"
    );
}

/// The decision and the send are tested above; this drives the loop that joins
/// them. Without it, deleting the call from the probe leaves every other
/// announcement test green - the pieces work and nothing announces.
#[test]
fn the_probe_loop_announces() {
    let (remote, tap) = client(204);
    lock(&remote.shared.schedule).set_hello_timer(Some(Instant::now()), Duration::from_millis(1));

    remote
        .spawn_workers()
        .expect("the worker threads could not start");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !tap.opcodes().contains(&op::SYS_HELLO.0) {
        std::thread::sleep(Duration::from_millis(50));
    }
    remote.close();

    assert!(
        tap.opcodes().contains(&op::SYS_HELLO.0),
        "the probe loop never announced, though the announcement was overdue"
    );
}

/// What the client says about itself has to parse as a hello, or no peer will
/// know it is there.
#[test]
fn the_announcement_describes_this_client() {
    let (remote, tap) = client(206);
    remote.shared.announce();

    let frame = tap.frames().pop().expect("nothing was announced");
    let peer = Remote::new(
        Config {
            uid: Some(uid_n(207)),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    // A peer is what reads this frame; the sender drops its own as an echo.
    peer.shared
        .process_datagram(&frame, Ipv4Addr::new(10, 8, 8, 1));

    let seen = peer
        .device(remote.uid())
        .expect("a peer could not decode this client's announcement");
    assert_eq!(seen.name, "MXR Rust");
    assert_eq!(seen.serial, CLIENT_SERIAL);
    assert_eq!(seen.version, VERSION);
    assert_eq!(seen.supported_protocol, PROTOCOL_VERSION);
    assert!(seen.features.has(DeviceFeature::MANAGER));
    peer.close();
}

/// An event reaches the handler after the state lock is released, so a handler
/// may read the state the event describes without deadlocking.
#[test]
fn a_handler_may_read_the_state_its_event_describes() {
    struct Reentrant {
        remote: Mutex<Option<Arc<Remote>>>,
        seen: AtomicUsize,
    }

    impl EventHandler for Reentrant {
        fn on_device_update(&self, device: DeviceUid) {
            let remote = self.remote.lock().expect("set before any frame arrives");
            let remote = remote.as_ref().expect("set before any frame arrives");
            assert!(
                remote.device(device).is_some(),
                "the handler could not read the device its own event named"
            );
            self.seen.fetch_add(1, Ordering::SeqCst);
        }
    }

    let handler = Arc::new(Reentrant {
        remote: Mutex::new(None),
        seen: AtomicUsize::new(0),
    });
    let remote = Arc::new(
        Remote::new(
            Config {
                uid: Some(uid_n(208)),
                ..Config::default()
            },
            Arc::clone(&handler) as Arc<dyn EventHandler>,
        )
        .expect("a client given its own uid reads nothing from disk"),
    );
    *handler.remote.lock().expect("nothing else holds this") = Some(Arc::clone(&remote));

    // A device is not announced when it is first heard from: the hello it was
    // built from is the hello being applied, so nothing about it changed. The
    // second one renames it, which is a change.
    let peer = uid_n(209);
    remote.shared.process_datagram(
        &hello_datagram(peer, "Peer", "PR0002"),
        Ipv4Addr::new(10, 8, 8, 9),
    );
    assert_eq!(handler.seen.load(Ordering::SeqCst), 0);
    remote.shared.process_datagram(
        &hello_datagram(peer, "Renamed", "PR0002"),
        Ipv4Addr::new(10, 8, 8, 9),
    );
    assert!(
        handler.seen.load(Ordering::SeqCst) > 0,
        "no event reached the handler"
    );
}
