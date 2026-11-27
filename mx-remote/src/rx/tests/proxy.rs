// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A OneIP controller announcing its driver proxy.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::event::Event;
use crate::rx::process_frame;
use crate::state::PROXY_ANNOUNCE_WINDOW;
use crate::wire::{op, DeviceFeature, DeviceUid};

use super::Harness;

/// The controller's uplink uid in the driver proxy's published fixtures.
const UPLINK: DeviceUid = DeviceUid::from_array([0xC0; 16]);

/// The published `ANNOUNCE`, a whole datagram under [`UPLINK`].
const ANNOUNCE: &str = "50383600c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0590002000001";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// A controller's reduced hello on the customer network: no `MANAGER`, so
/// nothing but the announcement makes it complete.
fn controller() -> Harness {
    let mut h = Harness::new(0);
    h.sender = UPLINK;
    h.hello(
        0x36,
        "OneIP Controller",
        "CTRL0001",
        DeviceFeature::MESH_MEMBER,
    );
    h
}

fn announce(h: &mut Harness, at: Instant) {
    let events = process_frame(
        &mut h.state,
        &hex(ANNOUNCE),
        Some(Ipv4Addr::new(10, 8, 8, 9)),
        at,
    );
    h.events.extend(events);
}

fn changes(h: &Harness) -> Vec<bool> {
    h.events
        .iter()
        .filter_map(|e| match e {
            Event::ProxyControllerChanged { device, announced } if *device == UPLINK => {
                Some(*announced)
            }
            _ => None,
        })
        .collect()
}

/// A hello and an announcement under one uid make a proxy controller, once,
/// complete with no bays.
#[test]
fn an_announcing_device_is_a_proxy_controller() {
    let mut h = controller();
    assert!(
        !h.complete(),
        "a hello with no bays and no MANAGER is complete"
    );
    let now = Instant::now();
    announce(&mut h, now);
    announce(&mut h, now);
    assert_eq!(changes(&h), vec![true]);
    assert!(h.device().proxy_controller);
    assert!(h.complete(), "a proxy controller waits for bays");
}

/// An announcement from a device not yet heard is dropped, as every frame
/// from a stranger but a hello and a discover is.
#[test]
fn a_stranger_announcing_is_nobody() {
    let mut h = Harness::new(0);
    h.sender = UPLINK;
    announce(&mut h, Instant::now());
    assert!(h.state.device(UPLINK).is_none());
    assert_eq!(changes(&h), Vec::<bool>::new());
}

/// Only sub-op 0 with its version byte announces: another sub-op, or one
/// cut short, does not.
#[test]
fn only_an_announcement_marks_a_proxy_controller() {
    let mut h = controller();
    for payload in [&[0u8][..], &[1, 1], &[4, 0, 0, 60]] {
        h.feed(op::PROXY, payload);
    }
    assert_eq!(changes(&h), Vec::<bool>::new());
    assert!(!h.device().proxy_controller);
}

/// The mark holds for 30 s after the last announcement, and then drops.
#[test]
fn the_mark_drops_thirty_seconds_after_the_last_announcement() {
    let mut h = controller();
    let at = Instant::now();
    announce(&mut h, at);
    let mut events = Vec::new();
    let device = h.state.device_mut(UPLINK).expect("registered");
    device.check_proxy(
        at + PROXY_ANNOUNCE_WINDOW - Duration::from_millis(1),
        &mut events,
    );
    assert!(device.proxy_controller, "dropped early");
    device.check_proxy(at + PROXY_ANNOUNCE_WINDOW, &mut events);
    assert!(!device.proxy_controller, "kept past the window");
    h.events.extend(events);
    assert_eq!(changes(&h), vec![true, false]);
}

/// An announcement keeps nothing alive: the hello does.
#[test]
fn an_announcement_is_no_keep_alive() {
    let mut h = controller();
    let before = h.device().last_ping;
    announce(&mut h, Instant::now() + Duration::from_secs(5));
    assert_eq!(h.device().last_ping, before);
}
