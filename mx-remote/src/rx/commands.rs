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
use crate::wire::{
    cstr, BayUid, DeviceUid, EdidProfile, RcAction, RcKey, DEVICE_NAME_LEN, UID_LEN,
};

use super::handlers::{byte, command_subject, ipv4_at, takes_command, u16_at, u32_at, uid_at};
use super::Rx;

/// Runs `body` only when the sender is a device we know.
///
/// Every command handler needs the sender to exist, because the event names it
/// and a caller cannot act on a request from a device it has never seen.
fn from_known_device(state: &State, rx: &Rx<'_>) -> Option<DeviceUid> {
    state.device(rx.sender()).map(|d| d.uid)
}

/// Answers a discover with a hello and reports it, unless it asks another
/// device.
///
/// An empty discover asks every device. One carrying exactly a uid asks only
/// that device; any other length is read as asking every device, as a receiver
/// that predates the uid form reads them all. A unit answers one from a
/// sender it does not know too: a unit that has just started discovers
/// before anyone knows it, and takes frames only from a peer it has heard a
/// hello from.
pub(super) fn discover_request(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if rx.frame.payload().len() == UID_LEN && rx.frame.uid(0) != Some(state.uid) {
        return;
    }
    state.hello_requested = true;
    if let Some(device) = from_known_device(state, rx) {
        ev.push(Event::DiscoverRequest { device });
    }
}

/// Answers a ping addressed to this client with a hello.
///
/// A device on protocol 0x2A pings a peer it suspects is gone, and takes it
/// offline when no frame comes back within about 1.5s - sooner than this
/// client's next scheduled hello. A ping for another device asks nothing of
/// this one.
pub(super) fn ping(state: &mut State, rx: &Rx<'_>, _ev: &mut [Event]) {
    if rx.frame.uid(0) == Some(state.uid) {
        state.hello_requested = true;
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
/// bays are two bytes each and `no_power_on` follows at 20. A unit takes
/// either opcode only at 21 bytes or more, though the audio one does not
/// read the last.
fn routing_request(p: &[u8], audio_only: bool) -> Option<SetRouteRequest> {
    if p.len() < ROUTE_REQUEST_SIZE {
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

/// The shortest routing request a unit takes.
const ROUTE_REQUEST_SIZE: usize = 21;

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

/// The port a unit writes when it names none.
const PORT_NONE: u16 = 0xFFFF;

/// Width of one timing, which is also the shortest tail either infrared frame
/// can carry.
///
/// A receiver measures the struct plus one of these before it reads a field, so
/// a frame that stops at the struct is one nothing on the network acted on.
/// Blasting needs more than that again: the first timing is the gap ahead of
/// the burst rather than part of it, so a receiver replays nothing until a
/// second one arrives.
const IR_TIMING_SIZE: usize = 2;

/// Decodes a captured infrared burst.
///
/// `mxr_ir_data` is not packed, so its `u32` timestamp aligns to 4 and the
/// port's two bytes are followed by padding.
pub(super) fn ir_capture(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    if from_known_device(state, rx).is_none() {
        return;
    }
    let p = rx.frame.payload();
    if p.len() < IR_DATA_SIZE + IR_TIMING_SIZE {
        return;
    }
    let meta = ir_meta(&p[12..21]);
    // The header counts the timings, so the tail is what says how wide they
    // are: a sender from before they were widened describes the same count in
    // half the bytes and falls short of this, which is what keeps its list from
    // reaching a caller as if it were the current width. There is no width to
    // report alongside the blob, so a narrow list read as a wide one would be
    // wrong with nothing able to say so.
    //
    // Taking the counted bytes rather than the rest of the payload also stops
    // anything appended behind the list from arriving as extra timings.
    if meta.nb_timings == 0 {
        return;
    }
    let Some(timings) = ir_timings(p, usize::from(meta.nb_timings)) else {
        return;
    };
    let capture = IrCapture {
        port: u16_at(p, 0),
        timestamp: u32_at(p, 4),
        last_change: u32_at(p, 8),
        meta,
        timings,
    };
    // A unit names no port: it forwards what its own receiver caught while
    // its output shows a source on another unit, so the capture belongs to
    // that output, which on a OneIP is its only one.
    let port = if capture.port == PORT_NONE {
        state
            .device(rx.sender())
            .and_then(|d| d.first_output_port())
    } else {
        Some(capture.port)
    };
    let Some(bay) = port.map(|port| BayUid::new(rx.sender(), port)) else {
        return;
    };
    if state.bay(bay).is_some() {
        ev.push(Event::IrCaptured { bay, capture });
    }
}

/// Reads the counted timings behind an infrared header, as sixteen-bit values.
///
/// Two widths exist on the wire and the header counts timings rather than
/// bytes, so what is left behind the header is what says which arrived: the
/// same count occupies twice as much in the wider form. A sender of the narrow
/// one is widened here rather than refused, so a caller is handed one
/// representation whichever sent it - the values are identical, only the
/// encoding differs.
///
/// Tested widest first. A narrow list is exactly as long as its count, so it
/// can never be mistaken for a wide one, and a frame shorter than its own count
/// is refused rather than truncated.
fn ir_timings(p: &[u8], count: usize) -> Option<Vec<u8>> {
    let tail = p.get(IR_DATA_SIZE..)?;
    if let Some(wide) = count
        .checked_mul(IR_TIMING_SIZE)
        .and_then(|n| tail.get(..n))
    {
        return Some(wide.to_vec());
    }
    let narrow = tail.get(..count)?;
    Some(narrow.iter().flat_map(|&t| [t, 0]).collect())
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
    // A unit blasts nothing from fewer than two timings, the first being a
    // leading slot, or from a frame short of the timings it counts.
    let meta = ir_meta(&p[24..33]);
    let count = usize::from(meta.nb_timings);
    let Some(timings) = p.get(TX_IR_HEADER..TX_IR_HEADER + count * IR_TIMING_SIZE) else {
        return;
    };
    if count < 2 {
        return;
    }
    ev.push(Event::IrTransmitRequested {
        device,
        request: IrTransmitRequest {
            target: uid_at(p, 0),
            local_mode: byte(p, 16),
            local_bay: byte(p, 17),
            timestamp: u32_at(p, 20),
            meta,
            timings: timings.to_vec(),
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
/// The length of a rename: uid, port and name, padded to a multiple of eight.
const BAY_NAME_SIZE: usize = 40;

pub(super) fn change_bay_name(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    // A unit takes the frame with its padding, and keeps 15 bytes of the
    // name; the 16th is where its terminator goes.
    let p = rx.frame.payload();
    if p.len() < BAY_NAME_SIZE {
        return;
    }
    ev.push(Event::BayNameChangeRequested {
        device,
        change: BayNameChange {
            target: uid_at(p, 0),
            port: u16_at(p, 16),
            name: cstr(&p[18..17 + DEVICE_NAME_LEN]),
        },
    });
}

pub(super) fn reboot(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    if let Some(target) = manage_target(rx.frame.payload()) {
        ev.push(Event::RebootRequested {
            device,
            request: RebootRequest {
                all: target.is_none(),
                target,
            },
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
        // Sent as 0 or 1, and read as a unit reads it: anything but 0 is
        // completed.
        device.set_setup_completed(value != 0, ev);
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

/// Decodes the list of source devices filtered out of a sink's picker: the
/// sink's uid followed by zero or more filtered uids. It is a unit's report on
/// its own sink, or management setting a sink's list.
///
/// A unit takes it only as a whole number of uids, the sink's included.
pub(super) fn filter_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() % 16 != 0 {
        return;
    }
    let Some(sink) = command_subject(state, rx, p) else {
        return;
    };
    let filtered: Vec<DeviceUid> = p[16..].chunks_exact(16).map(|c| uid_at(c, 0)).collect();
    let Some(port) = state.device(sink).and_then(|d| d.first_output_port()) else {
        return;
    };
    if let Some(bay) = state.bay_mut(BayUid::new(sink, port)) {
        bay.set_filtered(filtered, ev);
    }
}

/// The one-byte payload a reboot or factory reset carries when it addresses
/// every unit.
const MANAGE_ALL: u8 = 0xFF;

/// Reads whom a reboot or factory reset addresses: `Some(None)` for every unit,
/// `Some(Some(uid))` for one. A unit takes a 16-byte uid, an empty payload, or
/// the single byte 0xFF, and drops any other length.
fn manage_target(p: &[u8]) -> Option<Option<DeviceUid>> {
    match p {
        [] | [MANAGE_ALL] => Some(None),
        _ if p.len() == 16 => Some(Some(uid_at(p, 0))),
        _ => None,
    }
}

pub(super) fn factory_reset(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    let Some(target) = manage_target(rx.frame.payload()) else {
        return;
    };
    let request = FactoryResetRequest {
        all: target.is_none(),
        target,
    };
    ev.push(Event::FactoryResetRequested { device, request });
}

/// Applies a tiling command, which is state for the sink it addresses and a
/// request to everyone else.
pub(super) fn v2ip_tiling(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(device) = from_known_device(state, rx) else {
        return;
    };
    // A unit takes a window only from management.
    let p = rx.frame.payload();
    if p.len() < 24 || !takes_command(state, device, uid_at(p, 0)) {
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
    // A device reports its own settings and only management changes them, so
    // a frame from anyone else is one no device acts on.
    let target = uid_at(p, 0);
    if target != device && !super::handlers::manages(state, device, target) {
        return;
    }
    let flags = p[24];
    let ip = ipv4_at(p, 20);
    let settings = RcSettings {
        target,
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
    // A list of IPv4 groups; a unit takes none from a length that is not a
    // whole number of them.
    let p = rx.frame.payload();
    if p.is_empty() || p.len() % 4 != 0 {
        return;
    }
    let groups = p.chunks_exact(4).map(|g| ipv4_at(g, 0)).collect();
    ev.push(Event::BlacklistChanged {
        device,
        change: V2ipBlacklistChange { groups, registered },
    });
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
