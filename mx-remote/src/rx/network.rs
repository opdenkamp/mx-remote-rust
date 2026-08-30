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

/// Decodes the report as sent before protocol 0x22.
///
/// The legacy struct grew by appending, so each field exists only from the
/// version that added it: the addresses at 0x12 and the MAC at 0x21. Below
/// those the bytes belong to whatever follows the struct.
pub(super) fn parse_legacy(d: &[u8], protocol: u16) -> Option<NetworkPortStatus> {
    if d.len() < 146 {
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
    if protocol >= 0x12 {
        status.ip = Some(ipv4_at(d, 132));
        status.querier = Some(ipv4_at(d, 136));
    }
    if protocol >= 0x21 {
        status.mac_address = mac_at(d, 140);
    }
    Some(status)
}

/// Reports whether a 0x22-stamped payload uses the later of the two layouts
/// that share the stamp.
///
/// The port and the feature word were widened from `u8` to `u16` without
/// bumping any version. Only the fields ahead of the addresses move - the MAC
/// ends at 24 or 26, and the address type aligns to 4, so the address, the
/// querier and the status block sit at 28, 32 and 36 either way. The ambiguity
/// is confined to bytes 0..27, and both layouts are 144 bytes, so neither the
/// length nor the version can separate them and the payload has to.
///
/// Testing whether the name looks like text does not work: an early-form name
/// of three characters or more puts a printable byte at 4 and reads as late.
/// What separates them is where the zero bytes fall. The later layout widened
/// two small fields, so byte 1 is the high byte of the port and byte 3 the
/// high byte of the feature word; the earlier layout has the features at 1 and
/// a name character at 3.
///
/// That rests on the feature word staying under 0x100. Only bits 0..6 are
/// defined today, leaving nine free. If the field ever grows past a byte, byte
/// 3 stops being zero and every late frame decodes as early - which would
/// present as a decode bug rather than as a widened field, so check this first.
///
/// A single-character early name also leaves byte 3 zero and is genuinely
/// ambiguous; the later layout is the tie-break, being what every device on a
/// live mesh was observed to emit, including units on much older firmware.
fn is_late_layout(d: &[u8]) -> bool {
    d.len() < 4 || (d[1] == 0 && d[3] == 0)
}

/// Decodes the report as sent from protocol 0x22.
pub(super) fn parse_modern(d: &[u8]) -> Option<NetworkPortStatus> {
    if d.len() < 39 {
        return None;
    }
    let late = is_late_layout(d);
    // The feature word is a u16 at 2 in the later layout and a u8 at 1 in the
    // earlier one; the flag bits live in its low byte either way.
    let features = if late { d[2] } else { d[1] };
    let support_status = features & (1 << 0) != 0;
    let support_cable = features & (1 << 1) != 0;
    let support_igmp = features & (1 << 3) != 0;
    let port_uplink = features & (1 << 6) != 0;

    // The name is `mxr_device_name`, one byte longer than the field it names,
    // so unlike the bare 16-byte name fields elsewhere it always has room for
    // a terminator.
    let (name_off, mac_off, port) = if late {
        (4, 21, u16::from_le_bytes([d[0], d[1]]))
    } else {
        (2, 19, u16::from(d[0]))
    };
    const IP_OFF: usize = 28;
    const QUERIER_OFF: usize = 32;

    let mut status = NetworkPortStatus {
        port,
        name: cstr(d.get(name_off..name_off + 17).unwrap_or_default()),
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
        status.mac_address = mac_at(d, mac_off);
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
