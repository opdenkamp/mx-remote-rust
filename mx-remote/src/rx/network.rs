// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Network port status reports.

use crate::event::Event;
use crate::state::State;
use crate::types::{MacAddress, NetworkPortStatus, UtpCableStatus, UtpLinkErrors, VctStatus};
use crate::wire::{cstr, UtpLinkSpeed};

use super::handlers::{byte, ipv4_at, u32_at};
use super::Rx;

/// The protocol version from which the report uses the current layout.
const MODERN_LAYOUT: u16 = 0x22;

/// Reads the four cable-pair warnings packed into one byte.
fn vct_status(v: u8) -> [VctStatus; 4] {
    let mut out = [VctStatus::Healthy; 4];
    for (pair, slot) in out.iter_mut().enumerate() {
        if v & (1 << pair) != 0 {
            *slot = VctStatus::Warning;
        }
    }
    out
}

/// Reads the per-pair cable measurements at each of the given offsets.
fn cable_pairs(d: &[u8], offsets: &[usize]) -> Vec<UtpCableStatus> {
    let mut out = Vec::with_capacity(offsets.len());
    for &o in offsets {
        if o + 12 > d.len() {
            break;
        }
        out.push(UtpCableStatus {
            polarity: d[o] == 1,
            pair: d[o + 1],
            skew: u32_at(d, o + 4),
            length: u32_at(d, o + 8),
        });
    }
    out
}

fn mac_at(d: &[u8], idx: usize) -> Option<MacAddress> {
    d.get(idx..idx + 6)
        .and_then(|b| <[u8; 6]>::try_from(b).ok())
        .map(MacAddress)
}

/// The stamp from which the report carries the two addresses, and the one from
/// which it carries the MAC.
const ADDRESSES_FROM: u16 = 0x12;
const MAC_FROM: u16 = 0x21;

/// Size of each form of the report sent before protocol 0x22.
///
/// Every field the three share sits at the same offset, so the size is what
/// separates them: the struct grew by appending an address pair and then a MAC,
/// each rounding back out to the 8-byte alignment. A frame is measured against
/// its own form, because a floor set at the largest rejects every older report
/// and one set at the smallest reads absent fields out of whatever follows.
const LEGACY_SIZE: usize = 136;
const LEGACY_SIZE_WITH_ADDRESSES: usize = 144;
const LEGACY_SIZE_WITH_MAC: usize = 152;

/// The size the report has at `protocol`.
fn legacy_size(protocol: u16) -> usize {
    if protocol >= MAC_FROM {
        LEGACY_SIZE_WITH_MAC
    } else if protocol >= ADDRESSES_FROM {
        LEGACY_SIZE_WITH_ADDRESSES
    } else {
        LEGACY_SIZE
    }
}

/// Decodes the report as sent before protocol 0x22.
///
/// The struct grew by appending, so a field exists only from the version that
/// added it: the addresses from 0x12 and the MAC from 0x21, at offsets 132 and
/// 140. Everything ahead of them is common to all three forms - the status word
/// sits at 1 rather than 4 because it is a packed union and carries no
/// alignment of its own.
pub(super) fn parse_legacy(d: &[u8], protocol: u16) -> Option<NetworkPortStatus> {
    if d.len() < legacy_size(protocol) {
        return None;
    }
    let mut status = NetworkPortStatus {
        port: u16::from(d[0]),
        name: cstr(&d[112..128]),
        link_speed: UtpLinkSpeed::from_wire(d[3] & 0x7),
        link_full_duplex: d[3] & (1 << 3) != 0,
        ip: None,
        querier: None,
        mac_address: None,
        errors: Some(UtpLinkErrors::from_wire(d[1])),
        vct_status: Some(vct_status(d[2])),
        cable_status: cable_pairs(d, &[8, 20, 32, 44]),
    };
    if protocol >= ADDRESSES_FROM {
        status.ip = Some(ipv4_at(d, 132));
        status.querier = Some(ipv4_at(d, 136));
    }
    if protocol >= MAC_FROM {
        status.mac_address = mac_at(d, 140);
    }
    Some(status)
}

/// Decodes the report as sent from protocol 0x22.
///
/// This layout reorders the struct rather than extending it: the name moves
/// ahead of the counters, so nothing here shares an offset with the forms
/// [`parse_legacy`] reads.
///
/// The port and the feature word are `u16`. A build exists in which both are
/// `u8`, putting the name at 2 and the MAC at 19, and it is not decoded here:
/// it was superseded five hours after it was written and no release carries
/// it. Separating the two from the payload would cost more than it buys, since
/// the only thing distinguishing them is a zero high byte on the feature word,
/// and that word was widened precisely so it could grow past a byte - so the
/// test would start failing on the firmware it was written to survive.
pub(super) fn parse_modern(d: &[u8]) -> Option<NetworkPortStatus> {
    if d.len() < 39 {
        return None;
    }
    // The flag bits live in the low byte of the feature word.
    let features = d[2];
    let support_status = features & (1 << 0) != 0;
    let support_cable = features & (1 << 1) != 0;
    let support_igmp = features & (1 << 3) != 0;
    let port_uplink = features & (1 << 6) != 0;

    // The name is `mxr_device_name`, one byte longer than the field it names,
    // so unlike the bare 16-byte name fields elsewhere it always has room for
    // a terminator.
    const NAME_OFF: usize = 4;
    const MAC_OFF: usize = 21;
    const IP_OFF: usize = 28;
    const QUERIER_OFF: usize = 32;
    let port = u16::from_le_bytes([d[0], d[1]]);

    let mut status = NetworkPortStatus {
        port,
        name: cstr(d.get(NAME_OFF..NAME_OFF + 17).unwrap_or_default()),
        link_speed: UtpLinkSpeed::from_wire(d[38] & 0x7),
        link_full_duplex: d[38] & (1 << 3) != 0,
        ip: None,
        querier: None,
        mac_address: None,
        errors: None,
        vct_status: None,
        cable_status: Vec::new(),
    };
    if port_uplink && d.len() >= QUERIER_OFF {
        status.mac_address = mac_at(d, MAC_OFF);
        status.ip = Some(ipv4_at(d, IP_OFF));
        if support_igmp && d.len() >= QUERIER_OFF + 4 {
            status.querier = Some(ipv4_at(d, QUERIER_OFF));
        }
    }
    if support_status {
        status.errors = Some(UtpLinkErrors::from_wire(byte(d, 36)));
        status.vct_status = Some(vct_status(byte(d, 37)));
    }
    if support_cable {
        status.cable_status = cable_pairs(d, &[40, 52, 64, 76]);
    }
    Some(status)
}

pub(super) fn network_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let protocol = rx.frame.protocol();
    let parsed = if protocol < MODERN_LAYOUT {
        parse_legacy(rx.frame.payload(), protocol)
    } else {
        parse_modern(rx.frame.payload())
    };
    if let (Some(status), Some(device)) = (parsed, state.device_mut(rx.sender())) {
        device.update_network_status(status, ev);
    }
}
