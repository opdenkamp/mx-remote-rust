// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A unit's status report, and the commands sent to one unit.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::state::State;
use crate::types::{UnitCommand, UnitNetConfig, UnitStatus, UNIT_MAX_TEMPERATURES};
use crate::wire::{cstr, UnitCommandFlag, UnitCommandKind, UnitFlag, UID_LEN};

use super::handlers::{u16_at, u32_at, uid_at};
use super::Rx;

/// Width of `mxr_unit_status`. Its fields are naturally aligned, so it has no
/// padding.
const UNIT_STATUS_SIZE: usize = 104;

/// Where the temperatures start, behind their count at 49.
const UNIT_TEMPERATURES_AT: usize = 50;

/// Where the status message starts, behind the temperatures.
const UNIT_MESSAGE_AT: usize = UNIT_TEMPERATURES_AT + UNIT_MAX_TEMPERATURES;

/// Width of `mxr_unit_command`: the target uid, the command and its flags,
/// and two reserved bytes.
const UNIT_COMMAND_SIZE: usize = 20;

/// Width of the `mxr_unit_net_config` behind a command: five addresses.
const UNIT_NET_CONFIG_SIZE: usize = 20;

/// Applies a unit's status report to the unit that sent it.
///
/// The report grows at the back, so a frame is read as far as it goes and the
/// fields it does not reach read as zero.
pub(super) fn unit_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let mut p = rx.frame.payload().to_vec();
    p.resize(p.len().max(UNIT_STATUS_SIZE), 0);
    let count = usize::from(p[49]).min(UNIT_MAX_TEMPERATURES);
    let status = UnitStatus {
        flags: UnitFlag::from_bits(u32_at(&p, 0)),
        uptime: u32_at(&p, 4),
        boot_count: u32_at(&p, 8),
        reboot_count: u32_at(&p, 12),
        encoder_health: u32_at(&p, 16),
        decoder_health: u32_at(&p, 20),
        network: net_config(&p[24..44]),
        fan_rpm: u16_at(&p, 44),
        fan_duty: p[46],
        status: p[47],
        net_revert_s: p[48],
        temperatures: p[UNIT_TEMPERATURES_AT..UNIT_TEMPERATURES_AT + count]
            .iter()
            .map(|t| *t as i8)
            .collect(),
        status_message: cstr(&p[UNIT_MESSAGE_AT..UNIT_STATUS_SIZE]),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_unit_status(status, ev);
    }
}

/// Reports a command a peer sent to one unit.
pub(super) fn unit_command(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < UNIT_COMMAND_SIZE || state.device(rx.sender()).is_none() {
        return;
    }
    let command = UnitCommand {
        target: uid_at(p, 0),
        command: UnitCommandKind::from_wire(p[UID_LEN]),
        flags: UnitCommandFlag::from_bits(p[UID_LEN + 1]),
        network: p
            .get(UNIT_COMMAND_SIZE..UNIT_COMMAND_SIZE + UNIT_NET_CONFIG_SIZE)
            .map(net_config),
    };
    ev.push(Event::UnitCommandRequested {
        device: rx.sender(),
        command,
    });
}

/// Five addresses in network byte order: address, netmask, gateway and the
/// two DNS servers.
fn net_config(p: &[u8]) -> UnitNetConfig {
    let at = |i: usize| Ipv4Addr::new(p[i], p[i + 1], p[i + 2], p[i + 3]);
    UnitNetConfig {
        ip: at(0),
        netmask: at(4),
        gateway: at(8),
        dns1: at(12),
        dns2: at(16),
    }
}
