// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The handlers for the mesh module's `MESH_REPLACE` (0x56) and `MESH_CONFIG`
//! (0x57).

use std::time::Instant;

use crate::event::Event;
use crate::state::{MeshConfigTransfer, State};
use crate::types::{
    MeshBays, MeshConfig, MeshConfigDigest, MeshReplaceUnit, MeshReplaceUnits, MESH_BAYS_MAX,
    MESH_CONFIG_MAX, MESH_REPLACE_UNITS_MAX,
};
use crate::wire::{
    config_kind, cstr, decode_controller_hello, decode_mesh_bays, decode_mesh_settings,
    replace_kind, DeviceFeature, DeviceUid, MeshReplaceResult, MESH_BAYS_ENTRY_SIZE,
    MESH_CONFIG_FLAG_COPY, MESH_CONFIG_HEADER_SIZE, MESH_CONFIG_VERSION,
    MESH_REPLACE_FLAG_CONTROLLER, MESH_REPLACE_HEADER_SIZE, MESH_REPLACE_KEPT,
    MESH_REPLACE_NAME_LEN, MESH_REPLACE_TARGET_SIZE, MESH_REPLACE_UNIT_SIZE, MESH_REPLACE_VERSION,
};

use super::handlers::{u16_at, u32_at, uid_at};
use super::Rx;

/// Reads a `MESH_REPLACE` frame of any kind.
///
/// A frame is dropped where a unit would drop it: shorter than its kind, or
/// of a payload version other than 1. A list is read by whole records, and
/// an answer only when it names the unit of this client's last request.
pub(super) fn mesh_replace(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < MESH_REPLACE_HEADER_SIZE || p[0] != MESH_REPLACE_VERSION {
        return;
    }
    let sender = rx.sender();
    match p[1] {
        replace_kind::QUERY => ev.push(Event::MeshReplaceQueried { device: sender }),
        replace_kind::UNITS => units(state, rx, p, ev),
        replace_kind::REQUEST if p.len() >= MESH_REPLACE_TARGET_SIZE => {
            ev.push(Event::MeshReplaceRequested {
                device: sender,
                unit: uid_at(p, MESH_REPLACE_HEADER_SIZE),
            });
        }
        replace_kind::RESULT if p.len() >= MESH_REPLACE_TARGET_SIZE => {
            let unit = uid_at(p, MESH_REPLACE_HEADER_SIZE);
            if state.mesh_replace_pending.map(|(_, asked)| asked) == Some(unit) {
                state.mesh_replace_pending = None;
                ev.push(Event::MeshReplaceAnswered {
                    device: sender,
                    unit,
                    result: MeshReplaceResult::from_wire(p[MESH_REPLACE_HEADER_SIZE + 16]),
                });
            }
        }
        _ => {}
    }
}

/// Whether a list that last grew at `at` is still kept as of `now`.
pub(crate) fn replace_list_kept(at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(at) < MESH_REPLACE_KEPT
}

/// Keeps a frame of the sender's list, as a unit outside a mesh keeps it: a
/// frame starting at the first record replaces what the sender listed
/// before, and a later one adds to it, and no more units are kept of every
/// sender together than a controller lists.
fn units(state: &mut State, rx: &Rx<'_>, p: &[u8], ev: &mut Vec<Event>) {
    let sender = rx.sender();
    let now = rx.timestamp;
    let first = u16_at(p, 2);
    let records = p
        .get(MESH_REPLACE_HEADER_SIZE..)
        .unwrap_or_default()
        .chunks_exact(MESH_REPLACE_UNIT_SIZE)
        .map(record);
    let elsewhere: usize = state
        .devices
        .values()
        .filter(|d| d.uid != sender)
        .filter_map(|d| d.mesh_replace_units.as_ref())
        .filter(|(_, at)| replace_list_kept(*at, now))
        .map(|(list, _)| list.units.len())
        .sum();
    let Some(device) = state.device_mut(sender) else {
        return;
    };
    let mut list = match device.mesh_replace_units.take() {
        Some((list, at)) if first != 0 && replace_list_kept(at, now) => list,
        _ => MeshReplaceUnits::default(),
    };
    list.offline_controller = p[6] & MESH_REPLACE_FLAG_CONTROLLER != 0;
    list.total = u16_at(p, 4);
    let room = MESH_REPLACE_UNITS_MAX.saturating_sub(elsewhere + list.units.len());
    list.units.extend(records.take(room));
    device.mesh_replace_units = Some((list.clone(), now));
    ev.push(Event::MeshReplaceUnitsReceived {
        device: sender,
        units: list,
    });
}

/// Reads one record of a list.
fn record(b: &[u8]) -> MeshReplaceUnit {
    let name = |at: usize| cstr(b.get(at..at + MESH_REPLACE_NAME_LEN).unwrap_or_default());
    MeshReplaceUnit {
        uid: uid_at(b, 0),
        features: DeviceFeature::from_bits(u32_at(b, 16)),
        input: u16_at(b, 20),
        output: u16_at(b, 22),
        input_name: name(24),
        output_name: name(24 + MESH_REPLACE_NAME_LEN),
    }
}

/// Reads a `MESH_CONFIG` frame of any kind.
///
/// A frame is dropped where a unit would drop it: shorter than the header,
/// or of a payload version other than 1. A hello is read only when it names
/// its sender. Settings are taken once every frame of them has arrived, in
/// order, from one sender, and their crc32 matches; a frame out of order
/// drops what that sender had sent. A set is reported only when it names
/// this client and comes from the controller this client follows, the one
/// sender a unit takes it from.
pub(super) fn mesh_config(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < MESH_CONFIG_HEADER_SIZE || p[0] != MESH_CONFIG_VERSION {
        return;
    }
    let sender = rx.sender();
    let subject = uid_at(p, 16);
    match p[1] {
        config_kind::HELLO if subject == sender => {
            let digest = MeshConfigDigest {
                length: u16_at(p, 6),
                crc32: u32_at(p, 8),
            };
            if let Some(device) = state.device_mut(sender) {
                device.set_mesh_config_digest(digest, ev);
            }
        }
        config_kind::REQUEST => ev.push(Event::MeshConfigRequested {
            device: sender,
            subject,
        }),
        kind @ (config_kind::DUMP | config_kind::SET | config_kind::BAYS) => {
            settings_frame(state, sender, kind, subject, p, ev);
        }
        config_kind::BAYS_REQUEST => ev.push(Event::MeshBaysRequested {
            device: sender,
            subject,
        }),
        _ => {}
    }
}

/// Adds one frame of settings to what its sender has sent, and acts on the
/// settings once they are whole.
fn settings_frame(
    state: &mut State,
    sender: DeviceUid,
    kind: u8,
    subject: DeviceUid,
    p: &[u8],
    ev: &mut Vec<Event>,
) {
    let flags = p[2];
    let offset = usize::from(u16_at(p, 4));
    let total = u16_at(p, 6);
    let crc = u32_at(p, 8);
    let chunk = p.get(MESH_CONFIG_HEADER_SIZE..).unwrap_or_default();
    let earlier = state.mesh_config_transfers.remove(&sender);
    let mut transfer = match earlier {
        _ if offset == 0 => MeshConfigTransfer {
            kind,
            flags,
            subject,
            total,
            crc,
            bytes: Vec::new(),
        },
        Some(t)
            if (t.kind, t.flags, t.subject, t.total, t.crc)
                == (kind, flags, subject, total, crc)
                && t.bytes.len() == offset =>
        {
            t
        }
        _ => return,
    };
    let total = usize::from(total);
    let most = if kind == config_kind::BAYS {
        MESH_BAYS_MAX * MESH_BAYS_ENTRY_SIZE
    } else {
        MESH_CONFIG_MAX
    };
    if total > most {
        return;
    }
    transfer.bytes.extend_from_slice(chunk);
    if transfer.bytes.len() < total {
        state.mesh_config_transfers.insert(sender, transfer);
        return;
    }
    // Bytes past the length fail the crc32 like any other wrong bytes.
    if crate::wire::crc32(&transfer.bytes) != crc {
        return;
    }
    if kind == config_kind::BAYS {
        let Some(entries) = decode_mesh_bays(&transfer.bytes) else {
            return;
        };
        let bays = MeshBays {
            subject,
            holder: sender,
            copy: flags & MESH_CONFIG_FLAG_COPY != 0,
            bays: entries,
        };
        state.mesh_bays.insert(subject, bays.clone());
        ev.push(Event::MeshBaysReceived {
            device: sender,
            bays,
        });
        return;
    }
    let Some(settings) = decode_mesh_settings(&transfer.bytes) else {
        return;
    };
    match kind {
        config_kind::DUMP => {
            let config = MeshConfig {
                subject,
                holder: sender,
                copy: flags & MESH_CONFIG_FLAG_COPY != 0,
                settings,
            };
            state.mesh_configs.insert(subject, config.clone());
            ev.push(Event::MeshConfigReceived {
                device: sender,
                config,
            });
        }
        _ if subject == state.uid && sender == state.mesh_controller => {
            ev.push(Event::MeshConfigSetRequested {
                device: sender,
                settings,
            });
        }
        _ => {}
    }
}

/// Reads a `CONTROLLER_STATE` frame. A hello (version 1, kind 0, at least 40
/// bytes) marks its sender, which a frame only reaches here from once it is
/// known, as a dedicated controller for as long as it is known, as a unit
/// from protocol 0x34 on does. Pulling a controller's state with a request
/// and its data is not taken yet.
pub(super) fn controller_state(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(hello) = decode_controller_hello(rx.frame.payload()) else {
        return;
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_controller_hello(hello, ev);
    }
}
