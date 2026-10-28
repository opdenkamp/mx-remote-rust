// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Commands addressed to this client, judged as a unit judges one addressed
//! to itself: taken only from management, which is the controller it follows
//! or a management application.

use crate::event::Event;
use crate::testing::{hello_payload, poisoned, uid_n};
use crate::wire::{op, DeviceFeature, DeviceUid, Opcode};

use super::Harness;

/// A registry with three peers: the controller this client follows, a peer
/// announcing the controller role it does not follow, and a management
/// application.
struct Peers {
    h: Harness,
    followed: DeviceUid,
    pretender: DeviceUid,
    manager: DeviceUid,
}

fn peers(n: u8) -> Peers {
    let mut h = Harness::new(n);
    let hello = |f: DeviceFeature| hello_payload(0x2F, "Peer", "MG0001", "4.8.0", f);
    let (followed, pretender) = (uid_n(0xD1), uid_n(0xD2));
    h.feed_as(followed, op::SYS_HELLO, &hello(DeviceFeature::MESH));
    h.feed_as(
        pretender,
        op::SYS_HELLO,
        &hello(DeviceFeature::MESH | DeviceFeature::MESH_MASTER),
    );
    let manager = h.controller();
    h.state.mesh_controller = followed;
    Peers {
        h,
        followed,
        pretender,
        manager,
    }
}

/// Feeds `payload` under `op` from each peer, and returns which of them it
/// was taken from, as `(pretender, followed, manager)`.
fn taken_from(
    p: &mut Peers,
    op: Opcode,
    payload: &[u8],
    saw: fn(&Event) -> bool,
) -> (bool, bool, bool) {
    let mut one = |sender: DeviceUid| {
        p.h.events.clear();
        p.h.feed_as(sender, op, payload);
        p.h.saw(saw)
    };
    (one(p.pretender), one(p.followed), one(p.manager))
}

/// A uid followed by `rest`, as most addressed commands are laid out.
fn to(uid: DeviceUid, rest: &[u8]) -> Vec<u8> {
    let mut v = uid.as_bytes().to_vec();
    v.extend_from_slice(rest);
    v
}

const ONLY_MANAGEMENT: (bool, bool, bool) = (false, true, true);

#[test]
fn a_reboot_of_this_client_or_every_unit_needs_management() {
    let mut p = peers(90);
    let own = p.h.state.uid;
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::RebootRequested { .. });
    assert_eq!(
        taken_from(&mut p, op::SYS_REBOOT, own.as_bytes(), saw),
        ONLY_MANAGEMENT
    );
    assert_eq!(
        taken_from(&mut p, op::SYS_REBOOT, &[], saw),
        ONLY_MANAGEMENT
    );
    assert_eq!(
        taken_from(&mut p, op::SYS_REBOOT, &[0xFF], saw),
        ONLY_MANAGEMENT
    );
    // About another unit, it is that unit's to judge.
    let other = uid_n(0x33).as_bytes().to_vec();
    assert_eq!(
        taken_from(&mut p, op::SYS_REBOOT, &other, saw),
        (true, true, true)
    );
}

#[test]
fn a_factory_reset_of_this_client_or_every_unit_needs_management() {
    let mut p = peers(91);
    let own = p.h.state.uid;
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::FactoryResetRequested { .. });
    assert_eq!(
        taken_from(&mut p, op::SYS_FACTORY_RESET, own.as_bytes(), saw),
        ONLY_MANAGEMENT
    );
    assert_eq!(
        taken_from(&mut p, op::SYS_FACTORY_RESET, &[], saw),
        ONLY_MANAGEMENT
    );
}

#[test]
fn power_save_for_this_client_or_every_unit_needs_management() {
    let mut p = peers(92);
    let own = p.h.state.uid;
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::PowerSaveRequested { .. });
    assert_eq!(
        taken_from(&mut p, op::V2IP_POWER_SAVE, &to(own, &[1]), saw),
        ONLY_MANAGEMENT
    );
    assert_eq!(
        taken_from(&mut p, op::V2IP_POWER_SAVE, &[1], saw),
        ONLY_MANAGEMENT
    );
}

#[test]
fn an_fpga_upgrade_needs_management() {
    let mut p = peers(93);
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::UpgradeFpgaRequested { .. });
    assert_eq!(
        taken_from(&mut p, op::V2IP_UPGRADE_FPGA, &[], saw),
        ONLY_MANAGEMENT
    );
}

#[test]
fn an_edid_profile_for_this_client_needs_management() {
    let mut p = peers(94);
    let own = p.h.state.uid;
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::EdidProfileChangeRequested { .. });
    let payload = to(own, &[3, 0, 0xA5, 0x5A, 0xA5, 0x5A, 0xA5, 0x5A]);
    assert_eq!(
        taken_from(&mut p, op::BAY_EDID_PROFILE, &payload, saw),
        ONLY_MANAGEMENT
    );
}

#[test]
fn a_unit_command_for_this_client_needs_management() {
    let mut p = peers(95);
    let own = p.h.state.uid;
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::UnitCommandRequested { .. });
    let payload = to(own, &poisoned(4));
    assert_eq!(
        taken_from(&mut p, op::SYS_UNIT_COMMAND, &payload, saw),
        ONLY_MANAGEMENT
    );
}

/// What the frame-by-frame judgement of other commands rests on: a write
/// about this client is judged against the controller it follows, not by
/// who announces the role.
#[test]
fn a_tiling_window_for_this_client_needs_management() {
    let mut p = peers(96);
    let own = p.h.state.uid;
    let saw: fn(&Event) -> bool = |e| matches!(e, Event::TilingChanged { .. });
    let payload = to(own, &poisoned(8));
    assert_eq!(
        taken_from(&mut p, op::V2IP_TILING, &payload, saw),
        ONLY_MANAGEMENT
    );
}
