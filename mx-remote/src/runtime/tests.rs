// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The runtime: what drives the announcement timer, and what the receive entry
//! point does with a datagram.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::testing::{bay_config_rec, datagram, fixed_str, uid_n};
use crate::types::MeshOperation;
use crate::wire::{
    op, protocol_for, BayFeatures, BayStatus, DeviceFeature, DeviceUid, Endpoints, Path,
    HEADER_LEN, PROTOCOL_VERSION,
};

use super::schedule::{next_hello_interval, HELLO_BASE, HELLO_JITTER};
use super::*;

/// The address every fixture datagram appears to come from.
const FROM: Ipv4Addr = Ipv4Addr::new(10, 8, 8, 9);

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
/// announced on a clock, so arriving traffic other than a ping addressed to
/// this client may not provoke one.
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

/// A client with one endpoint, or none yet, cannot tell which endpoint a
/// frame arrived by, so it does not claim to know whether a device is reached
/// by multicast.
#[test]
fn reaching_a_device_by_multicast_is_unknown_without_both_endpoints() {
    let (remote, _) = client(205);
    let peer = uid_n(206);
    remote.shared.process_datagram_on(
        &hello_datagram(peer, "Peer", "PR0002"),
        FROM,
        Some(Path::Multicast),
    );
    remote.shared.process_datagram_on(
        &hello_datagram(peer, "Peer", "PR0002"),
        FROM,
        Some(Path::Multicast),
    );
    let device = remote.device(peer).expect("the datagram was not processed");
    assert_eq!(device.multicast, None);
}

/// A ping addressed to this client from a known device is answered with a
/// hello at once: a device that hears nothing back within about 1.5s takes
/// this client offline, and the scheduled hello can be seconds away. A unit
/// drops a ping from a sender it has not registered, as it does every frame
/// but a hello and a discover.
#[test]
fn a_ping_for_this_client_is_answered_with_a_hello() {
    let (remote, tap) = client(202);
    let peer = uid_n(203);
    let ping = |target: DeviceUid| datagram(peer, op::SYS_PING, 0x2A, target.as_bytes());

    remote.shared.process_datagram(&ping(uid_n(202)), FROM);
    assert!(tap.opcodes().is_empty(), "answered a stranger");

    remote
        .shared
        .process_datagram(&hello_datagram(peer, "Peer", "PR0002"), FROM);
    tap.clear();
    remote.shared.process_datagram(&ping(uid_n(204)), FROM);
    assert!(tap.opcodes().is_empty(), "a ping for another device");

    remote.shared.process_datagram(&ping(uid_n(202)), FROM);
    assert_eq!(tap.opcodes(), vec![op::SYS_HELLO.0], "from a known device");
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
        .spawn_workers(None)
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

/// A dedicated controller follows its 54-byte hello with a `CONTROLLER_STATE`
/// hello, which marks it as one to a peer; any other client sends the hello
/// alone (matrixos e958c49).
#[test]
fn a_dedicated_controller_marks_itself_with_a_controller_hello() {
    for dedicated in [false, true] {
        let remote = Remote::new(
            Config {
                uid: Some(uid_n(208)),
                dedicated_controller: dedicated,
                ..Config::default()
            },
            Arc::new(()),
        )
        .expect("a client given its own uid reads nothing from disk");
        let tap = Arc::new(Tap::default());
        let sink = Arc::clone(&tap);
        lock(&remote.shared.tx).set_tap(Arc::new(move |frame: &[u8]| {
            sink.0.lock().expect("tap").push(frame.to_vec())
        }));
        remote.shared.announce();
        let frames = tap.frames();
        assert_eq!(frames.len(), if dedicated { 2 } else { 1 });
        assert_eq!(frames[0].len() - HEADER_LEN, 54, "the hello");
        assert_eq!(u16::from_le_bytes([frames[0][20], frames[0][21]]), 0x00);
        let (peer, _) = client(209);
        for frame in &frames {
            peer.shared
                .process_datagram(frame, Ipv4Addr::new(10, 8, 8, 1));
        }
        let seen = peer.device(remote.uid()).expect("a peer decodes it");
        assert_eq!(seen.dedicated_controller, dedicated);
        assert_eq!(
            peer.controller_hello(remote.uid()),
            dedicated.then(ControllerHello::default)
        );
        peer.close();
        remote.close();
    }
}

/// The controller hello follows a hello at most every 10 s, and goes at once
/// when the application changes it, but not when it sets the same one again.
#[test]
fn a_controller_hello_goes_every_10_s_and_on_a_change() {
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(210)),
            dedicated_controller: true,
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    let tap = Arc::new(Tap::default());
    let sink = Arc::clone(&tap);
    lock(&remote.shared.tx).set_tap(Arc::new(move |frame: &[u8]| {
        sink.0.lock().expect("tap").push(frame.to_vec())
    }));
    let state_frames = |tap: &Tap| {
        tap.frames()
            .iter()
            .filter(|f| u16::from_le_bytes([f[20], f[21]]) == 0x58)
            .count()
    };
    let t0 = Instant::now();
    remote.shared.announce();
    assert_eq!(state_frames(&tap), 1, "after the first hello");
    remote.shared.announce();
    assert_eq!(state_frames(&tap), 1, "within 10 s of the last");
    lock(&remote.shared.controller_hello).1 = Some(t0 - Duration::from_secs(10));
    remote.shared.announce();
    assert_eq!(state_frames(&tap), 2, "10 s after the last");
    let hello = ControllerHello {
        role: crate::ControllerRole::PRIMARY,
        generation: 7,
        primary: uid_n(210),
        parts: vec![crate::ControllerPart {
            part: 0,
            length: 100,
            crc: 0xDEAD_BEEF,
        }],
    };
    remote
        .set_controller_hello(hello.clone())
        .expect("a dedicated controller sets its hello");
    assert_eq!(state_frames(&tap), 3, "at once on a change");
    remote
        .set_controller_hello(hello)
        .expect("a dedicated controller sets its hello");
    assert_eq!(state_frames(&tap), 3, "the same one again");
    remote.close();

    let plain = Remote::new(
        Config {
            uid: Some(uid_n(211)),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    assert!(plain
        .set_controller_hello(ControllerHello::default())
        .is_err());
    plain.close();
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
    assert!(
        seen.features.has(DeviceFeature::CONFIG_INITIALISED),
        "a unit ignores the scaling in a 0x3C write without it"
    );
    peer.close();
}

/// A controller replacing a unit has to be told apart in the device list by
/// a serial and version of its own. A mesh device announces mesh support
/// rather than management, membership while it follows a controller, and the
/// controller role while that controller is itself.
#[test]
fn the_announcement_carries_the_identity_and_role_it_was_given() {
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(209)),
            serial: Some("P9SN66660600".to_owned()),
            version: Some("1.2.3".to_owned()),
            mesh_device: true,
            ..Config::default()
        },
        Arc::new(()),
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
    let peer = Remote::new(
        Config {
            uid: Some(uid_n(210)),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    let heard = |peer: &Remote| {
        let frame = tap.frames().pop().expect("nothing was announced");
        peer.shared
            .process_datagram(&frame, Ipv4Addr::new(10, 8, 8, 1));
        peer.device(remote.uid())
            .expect("a peer could not decode this client's announcement")
    };
    let role = |f: DeviceFeature| {
        (
            f.has(DeviceFeature::MESH_MEMBER),
            f.has(DeviceFeature::MESH_MASTER),
        )
    };

    remote.shared.announce();
    let seen = heard(&peer);
    assert_eq!(seen.serial, "P9SN66660600");
    assert_eq!(seen.version, "1.2.3");
    assert!(seen.features.has(DeviceFeature::MESH));
    assert!(
        !seen.features.has(DeviceFeature::MANAGER),
        "a mesh device announced management"
    );
    assert!(seen.features.has(DeviceFeature::CONFIG_INITIALISED));
    assert_eq!(role(seen.features), (false, false));

    tap.clear();
    remote.set_mesh_controller(uid_n(211));
    assert_eq!(remote.mesh_controller(), uid_n(211));
    assert_eq!(
        role(heard(&peer).features),
        (true, false),
        "following another controller"
    );

    tap.clear();
    remote.set_mesh_controller(remote.uid());
    assert_eq!(
        role(heard(&peer).features),
        (true, false),
        "the role was announced with no bays mapped"
    );

    tap.clear();
    remote.set_mesh_bays_mapped(true);
    assert_eq!(
        role(heard(&peer).features),
        (true, true),
        "taking the role was not announced"
    );
    tap.clear();
    remote.set_mesh_bays_mapped(true);
    assert!(tap.frames().is_empty(), "unchanged bays were re-announced");

    tap.clear();
    remote.set_mesh_controller(remote.uid());
    assert!(
        tap.frames().is_empty(),
        "an unchanged controller was re-announced"
    );

    remote.set_mesh_controller(DeviceUid::ZERO);
    assert_eq!(role(heard(&peer).features), (false, false));
    peer.close();
}

/// What every unit sends to both endpoints goes to both; anything else goes
/// to both only while some peer needs broadcast, which is judged afresh as
/// frames arrive by each path.
#[test]
fn a_frame_goes_to_both_endpoints_when_a_unit_would_send_it_there() {
    let (remote, _tap) = client(212);
    let last = |remote: &Remote| {
        *lock(&remote.shared.tx)
            .sent_on
            .last()
            .expect("nothing sent")
            == Endpoints::Both
    };

    remote.shared.announce();
    assert!(last(&remote), "a hello goes to both endpoints");
    let _ = remote.send_mesh_operation(MeshOperation::report_membership(DeviceUid::ZERO));
    assert!(!last(&remote), "nothing needs broadcast yet");

    let peer = uid_n(213);
    let heard = |remote: &Remote, serial: &str, path: Path| {
        remote
            .shared
            .process_datagram_on(&hello_datagram(peer, "Peer", serial), FROM, Some(path));
    };
    heard(&remote, "PC0001", Path::Broadcast);
    heard(&remote, "PC0002", Path::Broadcast);
    let _ = remote.send_mesh_operation(MeshOperation::report_membership(DeviceUid::ZERO));
    assert!(
        last(&remote),
        "a peer heard only by broadcast was not reached by it"
    );

    heard(&remote, "PC0003", Path::Multicast);
    let _ = remote.send_mesh_operation(MeshOperation::report_membership(DeviceUid::ZERO));
    assert!(
        !last(&remote),
        "a peer heard by multicast again still got broadcast"
    );
}

/// A peer whose multicast goes quiet needs broadcast from the next probe
/// pass on, with no frame arriving to say so.
#[test]
fn the_probe_pass_judges_a_path_that_went_quiet() {
    let (remote, _tap) = client(216);
    let peer = uid_n(217);
    remote.shared.process_datagram_on(
        &hello_datagram(peer, "Peer", "PD0001"),
        FROM,
        Some(Path::Multicast),
    );
    remote.shared.process_datagram_on(
        &hello_datagram(peer, "Peer", "PD0002"),
        FROM,
        Some(Path::Multicast),
    );
    assert!(!remote
        .shared
        .broadcast_needed
        .load(std::sync::atomic::Ordering::SeqCst));

    let now = Instant::now();
    if let Some(device) = lock(&remote.shared.state).device_mut(peer) {
        device.heard_multicast = now.checked_sub(Duration::from_secs(125));
    }
    remote.shared.probe_once(now);
    assert!(
        remote
            .shared
            .broadcast_needed
            .load(std::sync::atomic::Ordering::SeqCst),
        "the probe pass did not notice the multicast went quiet"
    );
}

/// A client that hears a peer by broadcast but not by multicast announces
/// that its own multicast fails, as a unit does.
#[test]
fn a_multicast_fault_is_announced() {
    let (remote, tap) = client(214);
    let peer = Remote::new(
        Config {
            uid: Some(uid_n(215)),
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    let fault = |remote: &Remote| {
        tap.clear();
        remote.shared.announce();
        let frame = tap.frames().pop().expect("nothing was announced");
        peer.shared
            .process_datagram(&frame, Ipv4Addr::new(10, 8, 8, 1));
        peer.device(remote.uid())
            .expect("not decoded")
            .features
            .has(DeviceFeature::STATUS_MCAST_FAULT)
    };
    assert!(!fault(&remote));

    remote
        .shared
        .multicast_fault
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(fault(&remote), "the fault was not announced");
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

/// Starting introduces this client before the caller can command anything.
///
/// A device drops every frame from a uid it has no record of, hello and
/// discover excepted, so a command sent before the hello is discarded by each
/// peer that has not met us - silently, since nothing answers a frame it
/// dropped and the send reports the bytes it wrote. Leaving this to the probe
/// loop would put the hello a tick late, which is invisible to a caller that
/// holds the client open for days and fatal to a script that starts and
/// immediately commands.
///
/// The discover matters as much: it is the other opcode a stranger may send,
/// and it makes every unit answer at once rather than at its own announcement
/// interval.
#[test]
fn starting_announces_and_solicits_before_it_returns() {
    let (remote, tap) = client(205);
    if remote.start().is_err() {
        // No usable multicast interface here; the ordering is untested rather
        // than shown to hold.
        return;
    }
    let opcodes = tap.opcodes();
    remote.close();

    assert_eq!(
        opcodes.first().copied(),
        Some(op::SYS_HELLO.0),
        "start returned without introducing this client"
    );
    assert!(
        opcodes.contains(&op::SYS_DISCOVER.0),
        "start returned without asking the network to describe itself"
    );
}

/// Collects the device events a probe pass produces.
#[derive(Default)]
struct Watching {
    completed: Mutex<Vec<DeviceUid>>,
    offline: Mutex<Vec<DeviceUid>>,
}

impl EventHandler for Watching {
    fn on_device_config_complete(&self, device: DeviceUid) {
        self.completed.lock().expect("test handler").push(device);
    }

    fn on_device_online_changed(&self, device: DeviceUid, online: bool) {
        if !online {
            self.offline.lock().expect("test handler").push(device);
        }
    }
}

/// Announces `peer` to `remote` as a video matrix, and gives it one bay.
///
/// A matrix is behind the link-configuration gate, so what it still owes after
/// this is its links and nothing else.
fn matrix_with_one_bay(remote: &Remote, peer: DeviceUid) {
    remote
        .shared
        .process_datagram(&hello_datagram(peer, "FF88", "PB0001"), FROM);
    remote.shared.process_datagram(
        &datagram(
            peer,
            op::SYS_BAY_CONFIG,
            PROTOCOL_VERSION,
            &bay_config_rec(
                1,
                1,
                0,
                "Output 1",
                "TV",
                BayStatus::NONE,
                BayFeatures::HDMI_OUT,
            ),
        ),
        FROM,
    );
}

/// Moves `peer`'s registration `age` into the past.
fn age(remote: &Remote, peer: DeviceUid, age: Duration) {
    remote.shared.mutate(|state, _| {
        let device = state.device_mut(peer).expect("peer not registered");
        device.first_seen = device
            .first_seen
            .checked_sub(age)
            .expect("the test clock cannot predate the process");
    });
}

/// A device that stops waiting for its links is announced from the probe pass.
///
/// Nothing arrives when a window closes, so no frame can carry the news: the
/// pass that re-tests completion is the whole of the announcement path. Driven
/// through that pass rather than through the device method, so the two cannot
/// come apart.
#[test]
fn the_probe_pass_announces_a_device_its_window_completed() {
    let seen = Arc::new(Watching::default());
    let (remote, _) = client_with(210, Arc::clone(&seen) as Arc<dyn EventHandler>);
    let peer = uid_n(211);
    matrix_with_one_bay(&remote, peer);

    remote.shared.probe_once(Instant::now());
    assert!(
        seen.completed.lock().expect("test handler").is_empty(),
        "inside the window the pass announced a device still owing its links"
    );

    age(&remote, peer, CONFIG_GRACE + Duration::from_secs(1));
    remote.shared.probe_once(Instant::now());
    assert_eq!(
        *seen.completed.lock().expect("test handler"),
        vec![peer],
        "the pass did not announce a device no further frame will complete"
    );
}

/// A device that drops off is noticed whether or not anything else finished.
///
/// Nothing on the wire marks a device gone - it stops pinging, and comparing
/// its last ping against the clock is the only thing that can tell. A device
/// drops off in whatever state it reached, including before it ever described
/// itself, so a client whose devices are all in that state is precisely the one
/// that must still be checking.
#[test]
fn a_device_that_never_described_itself_is_still_reported_gone() {
    let seen = Arc::new(Watching::default());
    let (remote, _) = client_with(212, Arc::clone(&seen) as Arc<dyn EventHandler>);
    let peer = uid_n(213);
    remote
        .shared
        .process_datagram(&hello_datagram(peer, "FF88", "PB0002"), FROM);

    // Long enough for the silence to count, and nothing has completed: the
    // device sent a hello and never a bay.
    remote
        .shared
        .probe_once(Instant::now() + Duration::from_secs(600));

    assert_eq!(
        *seen.offline.lock().expect("test handler"),
        vec![peer],
        "a device that stopped answering was never reported gone"
    );
}

/// A management client on the mesh does not keep this client discovering.
///
/// A manager - another controller, or a second instance of this library - sends
/// a hello and nothing else: it has no bays or links to report. Waiting for
/// them never ends, and a device still undescribed past its window is asked
/// for the rest again.
#[test]
fn a_management_client_does_not_keep_discovery_going() {
    let (remote, tap) = client(214);
    let matrix = uid_n(215);
    matrix_with_one_bay(&remote, matrix);

    let manager = uid_n(216);
    let hello = crate::testing::hello_payload(
        0x2A,
        "Controller",
        "CTRL0001",
        "1.0.0",
        DeviceFeature::MANAGER,
    );
    remote.shared.process_datagram(
        &datagram(manager, op::SYS_HELLO, protocol_for(op::SYS_HELLO), &hello),
        FROM,
    );

    let past_window = CONFIG_GRACE + Duration::from_secs(1);
    age(&remote, matrix, past_window);
    age(&remote, manager, past_window);
    tap.clear();
    remote.shared.probe_once(Instant::now());

    assert!(
        !tap.opcodes().contains(&op::SYS_DISCOVER.0),
        "a manager that has nothing more to send kept discovery going"
    );
}

/// Announces `peer` to `remote` with the given features, as a hello datagram.
fn hello_with(remote: &Remote, peer: DeviceUid, features: DeviceFeature) {
    let hello = crate::testing::hello_payload(0x2A, "ONEIP", "SR0001", "1.0.0", features);
    remote.shared.process_datagram(
        &datagram(peer, op::SYS_HELLO, protocol_for(op::SYS_HELLO), &hello),
        FROM,
    );
}

/// Moves the last time `peer` was heard far enough back to count as offline,
/// and not so far that it is forgotten.
fn silence(remote: &Remote, peer: DeviceUid) {
    remote.shared.mutate(|state, _| {
        let device = state.device_mut(peer).expect("peer not registered");
        device.last_ping = device
            .last_ping
            .checked_sub(Duration::from_secs(90))
            .expect("the test clock cannot predate the process");
    });
}

/// The discovers captured, as their payloads.
fn discovers(tap: &Tap) -> Vec<Vec<u8>> {
    tap.frames()
        .into_iter()
        .filter(|f| f.get(20..22) == Some(&op::SYS_DISCOVER.0.to_le_bytes()[..]))
        .map(|f| f[HEADER_LEN..].to_vec())
        .collect()
}

/// A device that comes back is asked for its state by a discover naming it.
///
/// A device repeats a frame only for a short while after it changes, so what
/// it sent while this client could not hear it may never come again.
#[test]
fn a_device_back_from_offline_is_asked_for_its_state() {
    let (remote, tap) = client(220);
    let peer = uid_n(221);
    let features = DeviceFeature::VIDEO_ROUTING;

    hello_with(&remote, peer, features);
    assert_eq!(
        discovers(&tap),
        vec![peer.as_bytes().to_vec()],
        "a device this client first heard"
    );

    tap.clear();
    hello_with(&remote, peer, features);
    assert!(discovers(&tap).is_empty(), "a device that never went away");

    silence(&remote, peer);
    hello_with(&remote, peer, features);
    assert_eq!(
        discovers(&tap),
        vec![peer.as_bytes().to_vec()],
        "a device back from offline"
    );
}

/// A device that rebooted sends everything it holds unasked, and one about to
/// reboot is about to; a management client holds nothing to send.
#[test]
fn a_device_with_nothing_missed_is_not_asked_for_its_state() {
    let (remote, tap) = client(222);
    let features = DeviceFeature::VIDEO_ROUTING;

    let rebooted = uid_n(223);
    hello_with(&remote, rebooted, features);
    silence(&remote, rebooted);
    tap.clear();
    hello_with(&remote, rebooted, features | DeviceFeature::BOOT_BIT);
    assert!(
        discovers(&tap).is_empty(),
        "a device whose reboot toggle flipped"
    );

    let rebooting = uid_n(224);
    hello_with(&remote, rebooting, features | DeviceFeature::STATUS_REBOOT);
    assert!(discovers(&tap).is_empty(), "a device about to reboot");

    let manager = uid_n(225);
    hello_with(&remote, manager, DeviceFeature::MANAGER);
    assert!(discovers(&tap).is_empty(), "a management client");
}

/// A discover to every device already asked each one for everything.
#[test]
fn a_recent_discover_covers_a_device_that_appears() {
    let (remote, tap) = client(226);
    let _ = remote.discover();
    tap.clear();

    hello_with(&remote, uid_n(227), DeviceFeature::VIDEO_ROUTING);
    assert!(discovers(&tap).is_empty(), "asked again inside the window");

    lock(&remote.shared.schedule)
        .discovered(Instant::now() - DISCOVER_COVERS - Duration::from_secs(1));
    hello_with(&remote, uid_n(228), DeviceFeature::VIDEO_ROUTING);
    assert_eq!(
        discovers(&tap),
        vec![uid_n(228).as_bytes().to_vec()],
        "not asked once the window had passed"
    );
}

/// Keeps `peer` counted alive at `at`, as a device pinging away would.
fn keep_alive(remote: &Remote, peer: DeviceUid, at: Instant) {
    remote.shared.mutate(|state, _| {
        state
            .device_mut(peer)
            .expect("peer not registered")
            .last_ping = at;
    });
}

/// Runs the probe pass once a second for `minutes` from `start`, keeping
/// `peer` alive, and returns how many discovers had named it by the end of
/// each minute.
fn asks_by_minute(
    remote: &Remote,
    tap: &Tap,
    peer: DeviceUid,
    start: Instant,
    minutes: u64,
) -> Vec<usize> {
    let mut counts = Vec::new();
    for second in 0..minutes * 60 {
        let at = start + Duration::from_secs(second);
        keep_alive(remote, peer, at);
        remote.shared.probe_once(at);
        if second % 60 == 59 {
            let named = discovers(tap)
                .iter()
                .filter(|p| p.as_slice() == peer.as_bytes())
                .count();
            counts.push(named);
        }
    }
    counts
}

/// A device still owing its description is asked by name, at most once a
/// minute and three times in all.
///
/// Each unit answers a discover with everything it holds and nothing limits
/// how often, so asking on every pass keeps a whole mesh broadcasting. A unit
/// also broadcasts everything every five minutes unasked, which is what still
/// reaches a device that answered none of the asks.
#[test]
fn a_device_owing_its_description_is_asked_a_few_times() {
    let (remote, tap) = client(230);
    let peer = uid_n(231);
    hello_with(&remote, peer, DeviceFeature::VIDEO_ROUTING);
    age(&remote, peer, CONFIG_GRACE + Duration::from_secs(1));
    tap.clear();

    assert_eq!(
        asks_by_minute(&remote, &tap, peer, Instant::now(), 5),
        vec![1, 2, 3, 3, 3]
    );
    assert!(
        discovers(&tap).iter().all(|p| !p.is_empty()),
        "a discover to every device went while one was known"
    );
}

/// An ask a recent discover to every device stood in for is not counted.
#[test]
fn an_ask_covered_by_a_discover_is_not_counted() {
    let (remote, tap) = client(232);
    let peer = uid_n(233);
    hello_with(&remote, peer, DeviceFeature::VIDEO_ROUTING);
    age(&remote, peer, CONFIG_GRACE + Duration::from_secs(1));
    let start = Instant::now();
    let _ = remote.discover();
    tap.clear();

    assert_eq!(
        asks_by_minute(&remote, &tap, peer, start, 5),
        vec![0, 1, 2, 3, 3]
    );
}

/// A device that has gone offline is not asked: nothing would answer.
#[test]
fn an_offline_device_is_not_asked() {
    let (remote, tap) = client(234);
    let peer = uid_n(235);
    hello_with(&remote, peer, DeviceFeature::VIDEO_ROUTING);
    age(&remote, peer, CONFIG_GRACE + Duration::from_secs(1));
    silence(&remote, peer);
    tap.clear();

    remote.shared.probe_once(Instant::now());
    assert!(discovers(&tap).is_empty());
}

/// A device that described itself and then dropped back is asked afresh.
///
/// Going offline or rebooting has it describe itself again, and the asks
/// spent on its first description say nothing about this one.
#[test]
fn the_asks_start_over_once_a_device_is_described() {
    let (remote, tap) = client(236);
    let peer = uid_n(237);
    hello_with(&remote, peer, DeviceFeature::VIDEO_ROUTING);
    age(&remote, peer, CONFIG_GRACE + Duration::from_secs(1));
    tap.clear();
    let start = Instant::now();
    assert_eq!(asks_by_minute(&remote, &tap, peer, start, 3), vec![1, 2, 3]);

    matrix_with_one_bay(&remote, peer);
    keep_alive(&remote, peer, start + Duration::from_secs(180));
    remote.shared.probe_once(start + Duration::from_secs(180));
    assert!(remote
        .shared
        .mutate(|state, _| state.devices[&peer].have_config));
    // Silent long enough to go offline before the hello below, which arrives
    // now, and not long enough to be forgotten.
    let silent = start
        .checked_sub(Duration::from_secs(61))
        .expect("the test clock cannot predate the process");
    keep_alive(&remote, peer, silent);
    remote.shared.probe_once(start);
    hello_with(&remote, peer, DeviceFeature::VIDEO_ROUTING);
    tap.clear();

    let later = start + Duration::from_secs(181);
    assert_eq!(asks_by_minute(&remote, &tap, peer, later, 1), vec![1]);
}

/// A discover to every device goes only while none is known, and at most once
/// a minute.
#[test]
fn every_device_is_discovered_only_while_none_is_known() {
    let (remote, tap) = client(238);
    let start = Instant::now();
    let empty = |tap: &Tap| discovers(tap).iter().filter(|p| p.is_empty()).count();

    for second in 0..120 {
        remote
            .shared
            .probe_once(start + Duration::from_secs(second));
    }
    assert_eq!(empty(&tap), 2, "once a minute while none is known");

    let peer = uid_n(239);
    hello_with(&remote, peer, DeviceFeature::VIDEO_ROUTING);
    tap.clear();
    for second in 120..300 {
        let at = start + Duration::from_secs(second);
        keep_alive(&remote, peer, at);
        remote.shared.probe_once(at);
    }
    assert_eq!(empty(&tap), 0, "while a device is known");
}

/// Collects the devices a client forgets.
#[derive(Default)]
struct Forgetting(Mutex<Vec<DeviceUid>>);

impl EventHandler for Forgetting {
    fn on_device_unregistered(&self, device: DeviceUid) {
        self.0.lock().expect("test handler").push(device);
    }
}

/// Announces `peer` with the given hello protocol, which sets its offline
/// window.
fn hello_at(remote: &Remote, peer: DeviceUid, protocol: u16) {
    let hello =
        crate::testing::hello_payload(protocol, "ONEIP", "FG0001", "1.0.0", DeviceFeature::MESH);
    remote.shared.process_datagram(
        &datagram(peer, op::SYS_HELLO, protocol_for(op::SYS_HELLO), &hello),
        FROM,
    );
}

/// Devices are listed in the order this client first heard them, however
/// their identifiers sort, and hearing one again does not move it.
#[test]
fn devices_are_listed_in_the_order_first_heard() {
    let (remote, _) = client(240);
    let order: Vec<DeviceUid> = [37, 12, 250, 3, 99, 41, 180, 7].map(uid_n).to_vec();
    for &peer in &order {
        hello_at(&remote, peer, 0x2A);
    }
    hello_at(&remote, order[0], 0x2A);
    hello_at(&remote, order[3], 0x2A);
    assert_eq!(remote.devices(), order);
}

/// A device silent for three times its offline window is forgotten by the
/// probe pass, and not a moment before: 60s at protocol 0x2A and later, 15s
/// from 0x20, 180s below that. One heard again is registered anew, at the
/// end of the list.
#[test]
fn a_device_silent_for_three_offline_windows_is_forgotten() {
    let seen = Arc::new(Forgetting::default());
    let (remote, _) = client_with(241, Arc::clone(&seen) as Arc<dyn EventHandler>);
    let peers = [
        (uid_n(242), 0x29, 45),
        (uid_n(243), 0x2A, 180),
        (uid_n(244), 0x1F, 540),
    ];
    let t0 = Instant::now();
    for (peer, protocol, _) in peers {
        hello_at(&remote, peer, protocol);
    }
    for (peer, _, forget_s) in peers {
        remote
            .shared
            .probe_once(t0 + Duration::from_secs(forget_s - 1));
        assert!(
            remote.device(peer).is_some(),
            "forgotten inside three offline windows"
        );
        remote
            .shared
            .probe_once(Instant::now() + Duration::from_secs(forget_s));
        assert!(
            remote.device(peer).is_none(),
            "kept past three offline windows"
        );
        assert!(!remote.devices().contains(&peer), "still listed");
        assert_eq!(seen.0.lock().expect("test handler").last(), Some(&peer));
    }

    hello_at(&remote, uid_n(245), 0x2A);
    hello_at(&remote, uid_n(242), 0x2A);
    assert_eq!(remote.devices(), vec![uid_n(245), uid_n(242)]);
}

/// A forgotten device's link configuration goes with it, so the same link
/// reported after it is heard again is a new link.
#[test]
fn a_forgotten_device_takes_its_links_with_it() {
    let linked = Arc::new(Linking::default());
    let (remote, _) = client_with(246, Arc::clone(&linked) as Arc<dyn EventHandler>);
    let peer = uid_n(247);
    let link = datagram(
        peer,
        op::SYS_LINKS,
        PROTOCOL_VERSION,
        &crate::testing::link_rec(1, "AMP00001", "Zone 1", 0),
    );
    matrix_with_one_bay(&remote, peer);
    remote.shared.process_datagram(&link, FROM);
    assert_eq!(*linked.0.lock().expect("test handler"), 1);

    remote
        .shared
        .probe_once(Instant::now() + Duration::from_secs(600));
    assert!(remote.device(peer).is_none());
    matrix_with_one_bay(&remote, peer);
    remote.shared.process_datagram(&link, FROM);
    assert_eq!(
        *linked.0.lock().expect("test handler"),
        2,
        "the link of a forgotten device was remembered"
    );
}

/// Counts the links reported.
#[derive(Default)]
struct Linking(Mutex<usize>);

impl EventHandler for Linking {
    fn on_bay_linked(
        &self,
        _bay: crate::wire::BayUid,
        _linked_serial: String,
        _bay_name: String,
        _features: crate::wire::LinkFeature,
    ) {
        *self.0.lock().expect("test handler") += 1;
    }
}

/// The features this client reports announcing are those its hello carries,
/// whatever role, overlay sync or multicast fault it is announcing.
#[test]
fn the_reported_features_are_those_the_hello_carries() {
    let (remote, tap) = client(248);
    let announced = |remote: &Remote| {
        tap.clear();
        remote.shared.announce();
        let frame = tap.frames().pop().expect("nothing was announced");
        let bits = frame
            .get(HEADER_LEN + 50..HEADER_LEN + 54)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
            .expect("a hello carries its features");
        DeviceFeature::from_bits(bits)
    };
    assert_eq!(remote.features(), announced(&remote));
    assert!(remote.features().has(DeviceFeature::MANAGER));

    remote.set_mesh_controller(remote.uid());
    remote.set_mesh_bays_mapped(true);
    remote.shared.multicast_fault.store(true, Ordering::SeqCst);
    let features = remote.features();
    assert_eq!(features, announced(&remote));
    for bit in [
        DeviceFeature::MESH_MASTER,
        DeviceFeature::MESH_MEMBER,
        DeviceFeature::STATUS_MCAST_FAULT,
    ] {
        assert!(features.has(bit), "{bit:?} not reported");
    }
}

/// The source address a hello was captured from, a moment before or after
/// its UDP copy registered the device, is the device's address. A frame to
/// another port, a frame that is not a hello and a hello shorter than a unit
/// takes leave it alone. A forgotten device's address goes with it.
#[test]
fn a_hello_is_captured_with_its_source_address() {
    let (remote, _) = client(249);
    let peer = uid_n(250);
    let mac = [0x02, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E];
    let other = [0x02, 0x99, 0x88, 0x77, 0x66, 0x55];
    let hello = hello_datagram(peer, "Peer", "MA0001");
    let ports = [MULTICAST_PORT, 8811];

    remote
        .shared
        .note_link_frame_on(&crate::testing::ethernet(mac, 8811, &hello), &ports);
    remote.shared.process_datagram(&hello, FROM);
    let seen = |remote: &Remote| remote.device(peer).expect("registered").mac;
    assert_eq!(
        seen(&remote),
        Some(MacAddress(mac)),
        "captured before it registered"
    );

    let bays = datagram(peer, op::SYS_BAY_CONFIG, 1, &crate::testing::poisoned(61));
    let mut short = hello.clone();
    short.pop();
    short[22..24].copy_from_slice(&53u16.to_le_bytes());
    for (port, frame) in [(8813, &hello), (MULTICAST_PORT, &bays), (8811, &short)] {
        remote
            .shared
            .note_link_frame_on(&crate::testing::ethernet(other, port, frame), &ports);
    }
    assert_eq!(seen(&remote), Some(MacAddress(mac)));

    remote.shared.note_link_frame_on(
        &crate::testing::ethernet(other, MULTICAST_PORT, &hello),
        &ports,
    );
    assert_eq!(seen(&remote), Some(MacAddress(other)), "a later hello");

    remote
        .shared
        .probe_once(Instant::now() + Duration::from_secs(600));
    remote.shared.process_datagram(&hello, FROM);
    assert_eq!(seen(&remote), None, "kept for a forgotten device");
}

/// A client asked to capture what it cannot fails to start, and opens no
/// endpoint first.
#[cfg(not(target_os = "linux"))]
#[test]
fn capturing_off_linux_fails_to_start() {
    let remote = Remote::new(
        Config {
            uid: Some(uid_n(251)),
            capture_hello_mac: true,
            ..Config::default()
        },
        Arc::new(()),
    )
    .expect("a client given its own uid reads nothing from disk");
    let err = remote.start().expect_err("started without a capture");
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(remote.target().is_none(), "an endpoint was opened");
}

/// The tree a client reports heads with itself, and nests a device under
/// the one whose topology report lists it.
#[test]
fn the_device_tree_nests_by_topology() {
    let (remote, _) = client(252);
    let (a, b) = (uid_n(253), uid_n(254));
    hello_at(&remote, a, 0x2A);
    hello_at(&remote, b, 0x2A);
    let mut list = crate::testing::poisoned(24);
    list[..16].copy_from_slice(a.as_bytes());
    remote
        .shared
        .process_datagram(&datagram(b, op::TOPOLOGY, 0x06, &list), FROM);
    let tree: Vec<(DeviceUid, usize)> = remote
        .device_tree()
        .into_iter()
        .map(|e| (e.uid, e.depth))
        .collect();
    assert_eq!(tree, vec![(remote.uid(), 0), (b, 0), (a, 1)]);
    assert_eq!(remote.devices(), vec![b, a]);
}

/// The controller this client follows is what its receive path judges
/// management by.
#[test]
fn the_followed_controller_reaches_the_receive_path() {
    let (remote, _) = client(255);
    remote.set_mesh_controller(uid_n(3));
    assert_eq!(lock(&remote.shared.state).mesh_controller, uid_n(3));
    remote.set_mesh_controller(DeviceUid::ZERO);
    assert_eq!(lock(&remote.shared.state).mesh_controller, DeviceUid::ZERO);
}

/// A client always runs both endpoints a unit has; only a test or
/// development setup naming an address of its own runs one, on that address.
#[test]
fn a_client_always_runs_both_endpoints() {
    let plan = |config: Config| {
        let remote = Remote::new(config, Arc::new(())).expect("a client given its own uid");
        let plan = lock(&remote.shared.network).plan();
        plan
    };
    let uid = Some(uid_n(247));
    assert_eq!(
        plan(Config {
            uid,
            ..Config::default()
        }),
        Plan::Both
    );
    let group = Ipv4Addr::new(239, 255, 77, 90);
    assert_eq!(
        plan(Config {
            uid,
            target_ip: Some(group),
            port: Some(18890),
            ..Config::default()
        }),
        Plan::One(group, 18890)
    );
    assert_eq!(
        plan(Config {
            uid,
            target_ip: Some(group),
            ..Config::default()
        }),
        Plan::One(group, MULTICAST_PORT)
    );
}

/// The hello goes to both endpoints when it is the first since the sockets
/// opened or the last on broadcast went 30 s ago or more, and otherwise to the
/// group alone, unless some peer needs broadcast.
#[test]
fn the_hello_reaches_broadcast_at_least_every_30_seconds() {
    let (remote, _) = client(248);
    let on = |remote: &Remote| {
        remote.shared.announce();
        *lock(&remote.shared.tx)
            .sent_on
            .last()
            .expect("nothing sent")
    };
    assert_eq!(on(&remote), Endpoints::Both, "the first hello");
    assert_eq!(on(&remote), Endpoints::Multicast, "a hello inside 30 s");

    let back = |secs| {
        Instant::now()
            .checked_sub(Duration::from_secs(secs))
            .expect("the test clock cannot predate the process")
    };
    *lock(&remote.shared.hello_on_broadcast) = Some(back(29));
    assert_eq!(on(&remote), Endpoints::Multicast, "29 s after the last");
    *lock(&remote.shared.hello_on_broadcast) = Some(back(30));
    assert_eq!(on(&remote), Endpoints::Both, "30 s after the last");
    assert_eq!(on(&remote), Endpoints::Multicast);

    remote
        .shared
        .broadcast_needed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(on(&remote), Endpoints::Both, "a peer needing broadcast");
}

/// A ping goes by the one endpoint its device is heard by.
#[test]
fn a_ping_goes_by_the_endpoint_its_device_is_heard_by() {
    let (remote, _) = client(249);
    let peer = uid_n(250);
    // A peer on protocol 0x2A, the first that takes a ping.
    let heard = |serial: &str, path: Path| {
        let hello =
            crate::testing::hello_payload(0x2A, "Peer", serial, "1.0.0", DeviceFeature::MESH);
        remote.shared.process_datagram_on(
            &datagram(peer, op::SYS_HELLO, protocol_for(op::SYS_HELLO), &hello),
            FROM,
            Some(path),
        );
    };
    heard("PG0001", Path::Broadcast);
    heard("PG0002", Path::Broadcast);
    let on = |remote: &Remote| {
        let _ = remote.ping(peer);
        *lock(&remote.shared.tx)
            .sent_on
            .last()
            .expect("nothing sent")
    };
    assert_eq!(
        on(&remote),
        Endpoints::Broadcast,
        "heard by broadcast alone"
    );
    heard("PG0003", Path::Multicast);
    assert_eq!(on(&remote), Endpoints::Multicast, "heard by multicast");
}

/// A client that comes to hear a peer only by broadcast announces its
/// multicast fault at once, as a unit announces a change to its features.
#[test]
fn a_multicast_fault_is_announced_as_it_appears() {
    let (remote, tap) = client(251);
    let peer = uid_n(252);
    remote.shared.process_datagram_on(
        &hello_datagram(peer, "Peer", "MF0001"),
        FROM,
        Some(Path::Broadcast),
    );
    tap.clear();
    remote.shared.process_datagram_on(
        &hello_datagram(peer, "Peer", "MF0002"),
        FROM,
        Some(Path::Broadcast),
    );
    assert!(
        tap.opcodes().contains(&op::SYS_HELLO.0),
        "the fault was not announced when it appeared"
    );
    assert!(remote.features().has(DeviceFeature::STATUS_MCAST_FAULT));
}

/// The hello is due on broadcast 30 s after the last one there, to the
/// moment, and at once when none has gone since the sockets opened.
#[test]
fn the_hello_is_due_on_broadcast_30_seconds_after_the_last() {
    let last = Instant::now();
    let edge = last + Duration::from_secs(30);
    assert!(super::hello_due_on_broadcast(None, last));
    assert!(!super::hello_due_on_broadcast(
        Some(last),
        edge - Duration::from_nanos(1)
    ));
    assert!(super::hello_due_on_broadcast(Some(last), edge));
}
