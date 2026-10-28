// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The registry as a unit keeps it: a tree its peers' topology reports
//! shape, listed depth first.

use std::time::{Duration, Instant};

use crate::testing::{hello_payload, poisoned, uid_n};
use crate::wire::{op, DeviceFeature, DeviceUid};

use super::Harness;

/// A topology list naming `uids`, each with a mask of its own and its
/// padding poisoned.
fn topology(uids: &[DeviceUid]) -> Vec<u8> {
    let mut p = poisoned(24 * uids.len());
    for (i, uid) in uids.iter().enumerate() {
        p[i * 24..i * 24 + 16].copy_from_slice(uid.as_bytes());
        p[i * 24 + 16..i * 24 + 20].copy_from_slice(&(0x10 + i as u32).to_le_bytes());
    }
    p
}

/// A registry that has heard `ns` in that order.
fn heard(ns: &[u8]) -> Harness {
    let mut h = Harness::new(ns[0]);
    for &n in ns {
        let hello = hello_payload(0x2A, "ONEIP", "TR0001", "4.8.0", DeviceFeature::MESH);
        h.feed_as(uid_n(n), op::SYS_HELLO, &hello);
    }
    h
}

/// The tree as `(n, depth)`, this client as 0xFE.
fn tree(h: &Harness) -> Vec<(u8, usize)> {
    h.state
        .device_tree()
        .into_iter()
        .map(|(uid, depth)| (uid.as_bytes()[0], depth))
        .collect()
}

fn report(h: &mut Harness, sender: u8, uids: &[u8]) {
    let uids: Vec<DeviceUid> = uids.iter().map(|&n| uid_n(n)).collect();
    h.feed_as(uid_n(sender), op::TOPOLOGY, &topology(&uids));
}

/// This client heads the root, as a unit heads its own registry, and each
/// device joins the end of it.
#[test]
fn devices_join_the_root_in_the_order_heard() {
    let h = heard(&[30, 10, 20]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (30, 0), (10, 0), (20, 0)]);
}

/// A listed device moves to the end of the sender's children, and the walk
/// puts a parent before its children. A child the next list leaves out
/// moves to the end of the root.
#[test]
fn a_topology_list_moves_devices_under_its_sender() {
    let mut h = heard(&[1, 2, 3, 4]);
    report(&mut h, 1, &[4, 2]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (4, 1), (2, 1), (3, 0)]);

    report(&mut h, 1, &[2]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1), (3, 0), (4, 0)]);

    report(&mut h, 3, &[1]);
    assert_eq!(
        tree(&h),
        vec![(0xFE, 0), (3, 0), (1, 1), (2, 2), (4, 0)],
        "a subtree moves whole"
    );
}

/// A device already anywhere below the sender stays where it is.
#[test]
fn a_device_below_the_sender_already_is_not_moved() {
    let mut h = heard(&[1, 2, 3]);
    report(&mut h, 2, &[3]);
    report(&mut h, 1, &[2]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1), (3, 2)]);
    report(&mut h, 1, &[3, 2]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1), (3, 2)]);
}

/// Only a list differing byte for byte from the last changes the tree, so a
/// device registered after it stays put until one does; a change in the
/// padding alone is a difference.
#[test]
fn only_a_different_list_reshapes_the_tree() {
    let mut h = heard(&[1, 2]);
    let list = topology(&[uid_n(2), uid_n(3)]);
    h.feed_as(uid_n(1), op::TOPOLOGY, &list);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1)]);

    let hello = hello_payload(0x2A, "ONEIP", "TR0003", "4.8.0", DeviceFeature::MESH);
    h.feed_as(uid_n(3), op::SYS_HELLO, &hello);
    h.feed_as(uid_n(1), op::TOPOLOGY, &list);
    assert_eq!(
        tree(&h),
        vec![(0xFE, 0), (1, 0), (2, 1), (3, 0)],
        "moved by the same list again"
    );

    let mut padded = list.clone();
    padded[44] ^= 0xFF;
    h.feed_as(uid_n(1), op::TOPOLOGY, &padded);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1), (3, 1)]);
}

/// A device not registered is skipped, and the moves go on past it.
#[test]
fn an_unregistered_device_is_skipped() {
    let mut h = heard(&[1, 2, 3, 4]);
    report(&mut h, 1, &[4]);
    report(&mut h, 1, &[2, 0x77, 3, 4]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (4, 1), (2, 1), (3, 1)]);
}

/// A forgotten device's children move to the end of its parent's.
#[test]
fn a_forgotten_device_leaves_its_children_to_its_parent() {
    let mut h = heard(&[1, 2, 3, 4]);
    report(&mut h, 2, &[3]);
    report(&mut h, 1, &[2, 4]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1), (3, 2), (4, 1)]);

    // Keep every device but 2 alive.
    let later = Instant::now() + Duration::from_secs(200);
    for n in [1, 3, 4] {
        h.state.device_mut(uid_n(n)).expect("registered").last_ping = later;
    }
    h.state.forget_silent(later, &mut Vec::new());
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (4, 1), (3, 1)]);
}

/// A peer's topology report moves this client like any other device.
#[test]
fn a_report_moves_this_client_like_any_device() {
    let mut h = heard(&[1, 2]);
    report(&mut h, 1, &[0xFE]);
    assert_eq!(tree(&h), vec![(1, 0), (0xFE, 1), (2, 0)]);
}

/// A report naming its own sender leaves the sender where it is.
#[test]
fn a_report_naming_its_sender_moves_nothing() {
    let mut h = heard(&[1, 2]);
    report(&mut h, 2, &[1]);
    report(&mut h, 1, &[1]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (2, 0), (1, 1)]);
}

/// A device the sender sits below stays where it is: moving it would take
/// the sender along into a loop. The moves go on past it.
#[test]
fn a_device_above_the_sender_stays_put() {
    let mut h = heard(&[1, 2, 3, 4]);
    report(&mut h, 1, &[2]);
    report(&mut h, 2, &[3]);
    assert_eq!(tree(&h), vec![(0xFE, 0), (1, 0), (2, 1), (3, 2), (4, 0)]);

    report(&mut h, 3, &[1, 2, 4]);
    assert_eq!(
        tree(&h),
        vec![(0xFE, 0), (1, 0), (2, 1), (3, 2), (4, 3)],
        "an ancestor moved, or the moves stopped at it"
    );
    for n in [1, 2, 3, 4] {
        assert!(h.state.device(uid_n(n)).is_some(), "{n} dropped out");
    }
}

/// This client, too, stays put when a report names it from below.
#[test]
fn this_client_above_the_sender_stays_put() {
    let mut h = heard(&[1, 2]);
    report(&mut h, 1, &[0xFE]);
    report(&mut h, 2, &[1]);
    assert_eq!(tree(&h), vec![(2, 0), (1, 1), (0xFE, 2)]);
    report(&mut h, 1, &[0xFE, 2]);
    assert_eq!(tree(&h), vec![(2, 0), (1, 1), (0xFE, 2)]);
}
