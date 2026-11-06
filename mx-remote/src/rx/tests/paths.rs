// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Which endpoint a frame arrived by, and its second copy by the other.

use std::time::{Duration, Instant};

use crate::rx::process_frame_on;
use crate::testing::{datagram, hello_payload, uid_n};
use crate::wire::{op, DeviceFeature, DeviceUid, Path};

use super::Harness;

/// A hello from `sender` announcing `features`, as it would be put on the
/// wire.
fn hello(sender: DeviceUid, serial: &str, features: DeviceFeature) -> Vec<u8> {
    datagram(
        sender,
        op::SYS_HELLO,
        0x2F,
        &hello_payload(0x2F, "ONEIP", serial, "4.8.0", features),
    )
}

/// The same datagram by the other endpoint within 100ms is the frame sent to
/// both, and is acted on once; it still proves its path. Later, or arriving
/// by no known endpoint, it is a frame of its own.
#[test]
fn a_second_copy_is_dropped_but_proves_its_path() {
    let mut h = Harness::new(60);
    let data = hello(h.sender, "PA0001", DeviceFeature::MESH);
    let t0 = Instant::now();

    process_frame_on(&mut h.state, &data, None, Some(Path::Multicast), t0);
    assert_eq!(h.state.frames_received, 1);
    assert_eq!(h.device().heard_multicast, None, "unknown until its hello");

    let t1 = t0 + Duration::from_millis(10);
    let ev = process_frame_on(&mut h.state, &data, None, Some(Path::Broadcast), t1);
    assert!(ev.is_empty(), "the second copy was acted on");
    assert_eq!(h.state.frames_received, 1, "the second copy was counted");
    assert_eq!(
        h.device().heard_broadcast,
        Some(t1),
        "the second copy did not prove its path"
    );

    let t2 = t0 + Duration::from_millis(150);
    process_frame_on(&mut h.state, &data, None, Some(Path::Multicast), t2);
    assert_eq!(h.state.frames_received, 2, "a repeat after the window");
    assert_eq!(h.device().heard_multicast, Some(t2));

    process_frame_on(&mut h.state, &data, None, None, t2);
    process_frame_on(&mut h.state, &data, None, None, t2);
    assert_eq!(
        h.state.frames_received, 4,
        "a single endpoint has no second copies to drop"
    );
}

/// A live peer not heard by multicast for two minutes has to be reached by
/// broadcast too, and one heard by broadcast meanwhile shows this client's
/// multicast fails, unless it is about to reboot. A peer reporting its own
/// multicast fault has to be reached by broadcast however it is heard.
#[test]
fn a_peer_heard_only_by_broadcast_needs_broadcast() {
    let mut h = Harness::new(61);
    let t0 = Instant::now();
    let first = hello(h.sender, "PB0001", DeviceFeature::MESH);
    process_frame_on(&mut h.state, &first, None, Some(Path::Multicast), t0);
    let again = hello(
        h.sender,
        "PB0001",
        DeviceFeature::MESH | DeviceFeature::MESH_MEMBER,
    );
    process_frame_on(
        &mut h.state,
        &again,
        None,
        Some(Path::Multicast),
        t0 + Duration::from_secs(1),
    );

    let t = t0 + Duration::from_secs(2);
    assert!(!h.state.needs_broadcast(t));
    assert!(!h.state.multicast_fault(t));

    let t = t0 + Duration::from_secs(110);
    process_frame_on(&mut h.state, &first, None, Some(Path::Broadcast), t);
    let t = t0 + Duration::from_secs(125);
    assert!(h.state.needs_broadcast(t), "two minutes without multicast");
    assert!(h.state.multicast_fault(t), "heard by broadcast only");

    let rebooting = hello(
        h.sender,
        "PB0001",
        DeviceFeature::MESH | DeviceFeature::STATUS_REBOOT,
    );
    process_frame_on(&mut h.state, &rebooting, None, Some(Path::Broadcast), t);
    assert!(h.state.needs_broadcast(t));
    assert!(
        !h.state.multicast_fault(t),
        "a peer about to reboot proved a fault"
    );

    let mut other = Harness::new(62);
    let faulty = hello(
        other.sender,
        "PB0002",
        DeviceFeature::MESH | DeviceFeature::STATUS_MCAST_FAULT,
    );
    process_frame_on(&mut other.state, &faulty, None, Some(Path::Multicast), t0);
    let fresh = hello(
        other.sender,
        "PB0002",
        DeviceFeature::MESH | DeviceFeature::STATUS_MCAST_FAULT | DeviceFeature::MESH_MEMBER,
    );
    process_frame_on(
        &mut other.state,
        &fresh,
        None,
        Some(Path::Multicast),
        t0 + Duration::from_secs(1),
    );
    assert!(
        other.state.needs_broadcast(t0 + Duration::from_secs(2)),
        "a peer whose multicast fails was not reached by broadcast"
    );
    assert!(!other.state.multicast_fault(t0 + Duration::from_secs(2)));

    let mut gone = Harness::new(63);
    let quiet = hello(uid_n(63), "PB0003", DeviceFeature::MESH);
    process_frame_on(&mut gone.state, &quiet, None, Some(Path::Broadcast), t0);
    // Gone long enough to be forgotten, which is what drops it from the count.
    let long_gone = t0 + Duration::from_secs(600);
    gone.state.forget_silent(long_gone, &mut Vec::new());
    assert!(
        !gone.state.needs_broadcast(long_gone),
        "a peer long gone still needed broadcast"
    );
}

/// Every registered peer counts towards broadcast and the multicast fault,
/// one gone silent but not yet forgotten included: a unit judges by every
/// peer it knows.
#[test]
fn a_silent_peer_still_counts_for_broadcast() {
    let mut h = Harness::new(62);
    let t0 = Instant::now();
    let first = hello(h.sender, "PS0001", DeviceFeature::MESH);
    process_frame_on(&mut h.state, &first, None, Some(Path::Broadcast), t0);
    let again = hello(h.sender, "PS0002", DeviceFeature::MESH);
    let t1 = t0 + Duration::from_secs(1);
    process_frame_on(&mut h.state, &again, None, Some(Path::Broadcast), t1);
    // The protocol 0x2F hello gives a 60 s offline window.
    let later = t1 + Duration::from_secs(61);
    assert!(!h.device().is_online(later), "still online");
    assert!(h.state.needs_broadcast(later), "a silent peer was left out");
    assert!(h.state.multicast_fault(later), "a silent peer was left out");
}

/// A peer counts towards a multicast fault only after its 60 s grace, from
/// when it is first heard and, for every peer, from when this client opens
/// its endpoints: its broadcast copies can arrive before its multicast.
#[test]
fn a_peer_counts_towards_a_fault_after_its_grace() {
    let mut h = Harness::new(64);
    let t0 = Instant::now();
    let first = hello(h.sender, "PG0001", DeviceFeature::MESH);
    process_frame_on(&mut h.state, &first, None, Some(Path::Broadcast), t0);
    let t = t0 + Duration::from_secs(59);
    process_frame_on(&mut h.state, &first, None, Some(Path::Broadcast), t);
    assert!(h.state.needs_broadcast(t));
    assert!(!h.state.multicast_fault(t), "a peer in its grace");
    assert!(h.state.multicast_fault(t0 + Duration::from_secs(60)));

    h.state.endpoints_opened = Some(t0 + Duration::from_secs(70));
    assert!(
        !h.state.multicast_fault(t0 + Duration::from_secs(129)),
        "opening the endpoints did not start the grace again"
    );
    assert!(h.state.multicast_fault(t0 + Duration::from_secs(130)));
}
