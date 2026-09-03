// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Handlers for the frames that carry a request rather than a report.
//!
//! A request reaches the caller as an event naming who asked. Whether it also
//! writes to the registry is decided by what the rest of the mesh does with the
//! frame, not by the frame being a request: where every device applies an
//! observed request to its record of the addressee, this library applies it
//! too, so that its view agrees with theirs. Where only the addressee acts, the
//! registry waits for that device's own report.

use std::net::Ipv4Addr;

use crate::event::Event;
use crate::state::State;
use crate::types::{
    ActionTransmitRequest, AudioClip, BayNameChange, EdidProfileChange, EdidRecord, EdidRequest,
    FactoryResetRequest, IrCapture, IrMeta, IrTransmitRequest, KeyTransmitRequest, MuteStatus,
    RcSettings, RebootRequest, SetRouteRequest, V2ipBlacklistChange, V2ipPowerSaveRequest,
    V2ipTilingConfig, VideoWallCommand, VideoWallOp, VolumeMuteStatus,
};
use crate::wire::{cstr, BayUid, DeviceUid, EdidProfile, RcAction, RcKey, DEVICE_NAME_LEN};

use super::handlers::{byte, ipv4_at, u16_at, u32_at, uid_at};
use super::Rx;

/// Runs `body` only when the sender is a device we know.
///
/// Every command handler needs the sender to exist, because the event names it
/// and a caller cannot act on a request from a device it has never seen.
fn from_known_device(state: &State, rx: &Rx<'_>) -> Option<DeviceUid> {
    state.device(rx.sender()).map(|d| d.uid)
}

pub(super) fn discover_request(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if let Some(device) = from_known_device(state, rx) {
        ev.push(Event::DiscoverRequest { device });
    }
}

/// Length of one EDID block.
const EDID_SIZE: usize = 256;

/// One reported EDID: an output flag, then the block.
const EDID_RECORD_SIZE: usize = EDID_SIZE + 1;

/// A request: the uid it asks about, then the direction.
const EDID_REQUEST_SIZE: usize = 17;

/// Decodes a `DEV_EDID` frame.
///
/// The output flag leads each record rather than one flag covering a whole
/// reply, so a reply is records concatenated and holds as many as it is long -
/// counted here rather than enumerated, which is what lets a sender append a
/// third. The two forms are told apart by which length they reach, longest
/// first, and a run's trailing bytes past its last whole record are ignored.
pub(super) fn edid(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    match p.len() {
        n if n >= EDID_RECORD_SIZE => {
            for record in p.chunks_exact(EDID_RECORD_SIZE) {
                let output = record[0] != 0;
                let data = record[1..].to_vec();
                // Kept as well as announced: the event carries the bytes past
                // the handler, and a caller that asked for an EDID reads it
                // back rather than having to hold on to one from a callback.
                if let Some(d) = state.device_mut(device) {
                    d.set_edid(output, data.clone());
                }
                ev.push(Event::EdidReceived {
                    device,
                    edid: EdidRecord { output, data },
                });
            }
        }
        n if n >= EDID_REQUEST_SIZE => ev.push(Event::EdidRequested {
            device,
            request: EdidRequest {
                target: uid_at(p, 0),
                output: byte(p, 16) != 0,
            },
        }),
        _ => {}
    }
}

/// Decodes `mxr_routing_change_request`. `mbay_port_id` is a `u16`, so the
/// bays are two bytes each and `no_power_on` follows at 20.
fn routing_request(p: &[u8], audio_only: bool) -> Option<SetRouteRequest> {
    let need = if audio_only { 20 } else { 21 };
    if p.len() < need {
        return None;
    }
    Some(SetRouteRequest {
        serial: cstr(&p[0..16]),
        sink_bay: u16_at(p, 16),
        source_bay: u16_at(p, 18),
        no_power_on: !audio_only && byte(p, 20) != 0,
        audio_only,
    })
}

pub(super) fn set_route(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    route_request(state, rx, ev, false);
}

pub(super) fn audio_set_route(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    route_request(state, rx, ev, true);
}

fn route_request(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>, audio_only: bool) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    if let Some(request) = routing_request(rx.frame.payload(), audio_only) {
        ev.push(Event::SetRouteRequested { device, request });
    }
}

/// Size of `mxr_ir_data` up to the timings it is followed by.
const IR_DATA_SIZE: usize = 24;

/// Width of one timing, which is also the shortest tail either infrared frame
/// can carry.
///
/// A receiver measures the struct plus one of these before it reads a field, so
/// a frame that stops at the struct is one nothing on the network acted on.
/// Blasting needs more than that again: the first timing is the gap ahead of
/// the burst rather than part of it, so a receiver replays nothing until a
/// second one arrives.
const IR_TIMING_SIZE: usize = 2;

/// The protocol version from which infrared captures are reported.
const IR_CAPTURE_PROTOCOL: u16 = 0x19;

/// Decodes a captured infrared burst.
///
/// `mxr_ir_data` is not packed, so its `u32` timestamp aligns to 4 and the
/// port's two bytes are followed by padding.
pub(super) fn ir_capture(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if from_known_device(state, rx).is_none() {
        return;
    }
    let p = rx.frame.payload();
    if p.len() < IR_DATA_SIZE + IR_TIMING_SIZE || rx.frame.protocol() < IR_CAPTURE_PROTOCOL {
        return;
    }
    let capture = IrCapture {
        port: u16_at(p, 0),
        timestamp: u32_at(p, 4),
        last_change: u32_at(p, 8),
        meta: ir_meta(&p[12..21]),
        timings: p[IR_DATA_SIZE..].to_vec(),
    };
    let bay = BayUid::new(rx.sender(), capture.port);
    if state.bay(bay).is_some() {
        ev.push(Event::IrCaptured { bay, capture });
    }
}

fn ir_meta(p: &[u8]) -> IrMeta {
    IrMeta {
        timer_resolution: u16_at(p, 0),
        frequency: u16_at(p, 2),
        nb_timings: u16_at(p, 4),
        repeat_offset: u16_at(p, 6),
        status: byte(p, 8),
    }
}

/// Decodes a request to blast raw infrared.
///
/// `mxr_tx_ir_data` is not packed: two bytes pad before the `u32` timestamp,
/// and the struct's own alignment pads the 9-byte meta block out to 36. The
/// firmware appends the timings at `sizeof`, so they start at 36 - taking them
/// from the end of the last field shifts every `u16` timing by two bytes.
const TX_IR_HEADER: usize = 36;

pub(super) fn ir_transmit(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < TX_IR_HEADER + IR_TIMING_SIZE {
        return;
    }
    ev.push(Event::IrTransmitRequested {
        device,
        request: IrTransmitRequest {
            target: uid_at(p, 0),
            local_mode: byte(p, 16),
            local_bay: byte(p, 17),
            timestamp: u32_at(p, 20),
            meta: ir_meta(&p[24..33]),
            timings: p[TX_IR_HEADER..].to_vec(),
        },
    });
}

pub(super) fn volume_step(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>, up: bool) {
    if from_known_device(state, rx).is_none() {
        return;
    }
    let Some(port) = rx.frame.u8(0) else {
        return;
    };
    let bay = BayUid::new(rx.sender(), u16::from(port));
    if state.bay(bay).is_some() {
        ev.push(Event::VolumeStep { bay, up });
    }
}

/// Decodes `mxr_volume_mute_data`, the notification a device sends when its
/// own volume changed.
///
/// `AUDIO_SET_VOLUME` (0x14) is the request form and names a target device;
/// this one does not.
pub(super) fn volume_mute(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if from_known_device(state, rx).is_none() {
        return;
    }
    let p = rx.frame.payload();
    if p.len() < 4 {
        return;
    }
    let mute = MuteStatus::from_wire(p[3]);
    let volume = VolumeMuteStatus {
        volume_left: Some(p[1]).filter(|v| *v <= 100),
        volume_right: Some(p[2]).filter(|v| *v <= 100),
        muted_left: Some(mute.left()),
        muted_right: Some(mute.right()),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.apply_bay_volume(u16::from(p[0]), volume, ev);
    }
}

pub(super) fn audio_clip(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if from_known_device(state, rx).is_none() {
        return;
    }
    let p = rx.frame.payload();
    if p.len() < 2 {
        return;
    }
    let bay = BayUid::new(rx.sender(), u16::from(p[0]));
    if state.bay(bay).is_some() {
        ev.push(Event::AudioClipped {
            bay,
            clip: AudioClip {
                port: u16::from(p[0]),
                clip: p[1],
            },
        });
    }
}

pub(super) fn v2ip_link_remote(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    if let Some(target) = rx.frame.uid(0) {
        ev.push(Event::V2ipLinkChanged { device, target });
    }
}

pub(super) fn detect_bays(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if let Some(device) = from_known_device(state, rx) {
        ev.push(Event::DetectBaysRequested { device });
    }
}

/// Decodes `mxr_bay_name_data`: a uid, a `u16` port, then a fixed-width name
/// that carries no terminator when it fills the field.
pub(super) fn change_bay_name(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < 18 + DEVICE_NAME_LEN {
        return;
    }
    ev.push(Event::BayNameChangeRequested {
        device,
        change: BayNameChange {
            target: uid_at(p, 0),
            port: u16_at(p, 16),
            name: cstr(&p[18..18 + DEVICE_NAME_LEN]),
        },
    });
}

pub(super) fn reboot(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    if let Some(target) = rx.frame.uid(0) {
        ev.push(Event::RebootRequested {
            device,
            request: RebootRequest { target },
        });
    }
}

pub(super) fn monitoring_pulse(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if let Some(device) = from_known_device(state, rx) {
        ev.push(Event::MonitoringPulse { device });
    }
}

pub(super) fn upgrade_fpga(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if let Some(device) = from_known_device(state, rx) {
        ev.push(Event::UpgradeFpgaRequested { device });
    }
}

pub(super) fn edid_profile(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < 18 {
        return;
    }
    ev.push(Event::EdidProfileChangeRequested {
        device,
        change: EdidProfileChange {
            target: uid_at(p, 0),
            profile: EdidProfile::from_wire(u16_at(p, 16)),
        },
    });
}

pub(super) fn setup_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(value) = rx.frame.u8(0) else {
        return;
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_setup_completed(value == 1, ev);
    }
}

pub(super) fn set_installer(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(id) = rx.frame.u16(0) else {
        return;
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_installer_id(id, ev);
    }
}

/// Decodes the list of source devices filtered out of a sink's picker: a
/// target uid followed by zero or more filtered uids.
///
/// The list is read to its last whole uid, so trailing bytes that are not one
/// are ignored rather than taken as evidence the frame is malformed.
///
/// A trailing array is the one shape no length test can protect: bytes appended
/// past the last uid are read as another uid once there are sixteen of them.
/// Appending to this payload is a wire break the sender has to announce, not
/// something a receiver can absorb.
pub(super) fn filter_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < 16 {
        return;
    }
    let filtered: Vec<DeviceUid> = p[16..].chunks_exact(16).map(|c| uid_at(c, 0)).collect();
    let Some(port) = state
        .device(rx.sender())
        .and_then(|d| d.first_output_port())
    else {
        return;
    };
    if let Some(bay) = state.bay_mut(BayUid::new(rx.sender(), port)) {
        bay.set_filtered(filtered, ev);
    }
}

/// The payload a factory-reset broadcast carries when it addresses every
/// device rather than one.
const FACTORY_RESET_ALL: u8 = 0xFF;

/// Decodes a factory-reset request: one addressing a named device, one
/// addressing every device, and an empty payload that addresses only the
/// sender.
///
/// The forms are told apart longest first, so a sender that appends to one is
/// still read as that form rather than falling through to the next. A payload
/// that reaches none of them is dropped: the form that carries nothing is
/// empty, so anything shorter than a uid and not marked for every device is
/// unrecognised rather than argument-free, and reading it as the sender
/// resetting itself would aim a destructive request from a frame that was not
/// understood.
pub(super) fn factory_reset(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    let request = match p.len() {
        n if n >= 16 => FactoryResetRequest {
            all: false,
            target: Some(uid_at(p, 0)),
        },
        n if n >= 1 && p[0] == FACTORY_RESET_ALL => FactoryResetRequest {
            all: true,
            target: None,
        },
        0 => FactoryResetRequest::default(),
        _ => return,
    };
    ev.push(Event::FactoryResetRequested { device, request });
}

/// Applies a tiling command, which is state for the sink it addresses and a
/// request to everyone else.
pub(super) fn v2ip_tiling(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < 24 {
        return;
    }
    let tiling = V2ipTilingConfig {
        target: uid_at(p, 0),
        pos_x: u16_at(p, 16),
        pos_y: u16_at(p, 18),
        width: u16_at(p, 20),
        height: u16_at(p, 22),
    };
    if let Some(target) = state.device_mut(tiling.target) {
        target.set_tiling(tiling, ev);
        return;
    }
    ev.push(Event::TilingChanged { device, tiling });
}

/// Decodes both power-save forms: a uid-addressed flag for one unit, and a bare
/// flag broadcast to every peer.
///
/// Tested longest first, so a sender that appends to either form is still read
/// as that form: the bare flag would otherwise swallow the addressed one.
pub(super) fn v2ip_power_save(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    let request = match p.len() {
        n if n >= 17 => V2ipPowerSaveRequest {
            target: Some(uid_at(p, 0)),
            enabled: p[16] == 1,
        },
        n if n >= 1 => V2ipPowerSaveRequest {
            target: None,
            enabled: p[0] == 1,
        },
        _ => return,
    };
    ev.push(Event::PowerSaveRequested { device, request });
}

/// Decodes the RC control frame: a target uid then the RC config block, whose
/// control-method and address fields are four bytes each ahead of the flag bits.
///
/// Every meaningful bit sits in byte 24 alone - the four flags in the low
/// nibble, the status in the high one. Byte 25 is dead space in the same
/// bitfield container and 26..27 hold the reserved bits that open a second one,
/// which is what puts the status name at 28. Reading 24..26 as one
/// little-endian `u16` and shifting agrees today and stops agreeing the moment
/// the reserved bits are spent.
pub(super) fn rc_settings(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < 28 {
        return;
    }
    let flags = p[24];
    let ip = ipv4_at(p, 20);
    let settings = RcSettings {
        target: uid_at(p, 0),
        rc_target: p[16],
        ip: (ip != Ipv4Addr::UNSPECIFIED).then_some(ip),
        cec_enabled: flags & (1 << 0) != 0,
        cec_auto_on: flags & (1 << 1) != 0,
        forward_rc: flags & (1 << 2) != 0,
        forward_ir: flags & (1 << 3) != 0,
        rc_status: (flags >> 4) & 0xF,
        // The status name is 15 characters in a 16-byte array, so unlike the
        // device name fields a full-length value here does carry its
        // terminator.
        status_name: p.get(28..44).map(cstr).unwrap_or_default(),
    };
    if let Some(target) = state.device_mut(settings.target) {
        target.set_rc_settings(settings, ev);
        return;
    }
    ev.push(Event::RcSettingsChanged { device, settings });
}

pub(super) fn tx_key(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < 20 {
        return;
    }
    ev.push(Event::KeyTransmitRequested {
        device,
        request: KeyTransmitRequest {
            target: uid_at(p, 0),
            local_bay: u16_at(p, 16),
            key: RcKey::from_wire(u16_at(p, 18)),
        },
    });
}

pub(super) fn tx_action(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < 20 {
        return;
    }
    ev.push(Event::ActionTransmitRequested {
        device,
        request: ActionTransmitRequest {
            target: uid_at(p, 0),
            local_bay: u16_at(p, 16),
            action: RcAction::from_wire(u16_at(p, 18)),
        },
    });
}

pub(super) fn blacklist(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>, registered: bool) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    if let Some(target) = rx.frame.uid(0) {
        ev.push(Event::BlacklistChanged {
            device,
            change: V2ipBlacklistChange { target, registered },
        });
    }
}

/// Decodes `vw_mesh_frame`. The struct is not packed and aligns to 4, so three
/// zeroed bytes trail the op byte.
///
/// This layout is the one decode here not derived from a source tree: the
/// video wall module owns the opcode and is not vendored alongside the
/// firmware, so the offsets came second-hand.
///
/// A shifted geometry field is self-evident - the window lands visibly wrong.
/// The op byte at 28 is the quiet one: read at the wrong offset, a store
/// behaves as a preview and looks entirely correct until the sink restarts and
/// the wall reverts, or a revert reads as a preview and its zeroed window
/// clears a wall that should have been restored. Suspect this layout on a wall
/// that forgets its setting across a reboot, not on one that is visibly
/// misplaced.
/// Payload length of a video-wall command: 29 bytes of fields in a 4-aligned
/// struct, and the receiver requires all 32 and ignores any tail.
const VIDEO_WALL_SIZE: usize = 32;

pub(super) fn video_wall(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let p = rx.frame.payload();
    if p.len() < VIDEO_WALL_SIZE {
        return;
    }
    ev.push(Event::VideoWallCommand {
        device,
        command: VideoWallCommand {
            target: uid_at(p, 0),
            pos_x: u16_at(p, 16),
            pos_y: u16_at(p, 18),
            width: u16_at(p, 20),
            height: u16_at(p, 22),
            raster_w: u16_at(p, 24),
            raster_h: u16_at(p, 26),
            op: VideoWallOp::from_wire(p[28]),
        },
    });
}
