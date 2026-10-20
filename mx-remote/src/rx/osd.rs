// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Overlay sync, as a watcher sees it.

use crate::event::Event;
use crate::state::State;
use crate::types::OSD_SYNC_RECORDS_MAX;
use crate::types::{
    OsdCaption, OsdLogo, OsdMessage, OsdSync, OsdSyncReport, OsdSyncRequest, OsdSyncSetting,
    OsdSyncSlot, OsdSyncValue,
};
use crate::wire::{
    cstr, osd_kind, osd_setting, DeviceFeature, DeviceUid, OsdCaptionAlign, OsdCaptionFormat,
    OsdMessageFlag, OsdMessagePriority, OsdSyncSize, OsdSyncState, OsdSyncWhat,
    OSD_BACKGROUND_SIZE, OSD_CAPTION_SIZE, OSD_FONT_LEN, OSD_HEADER_SIZE, OSD_LOGO_SIZE,
    OSD_MESSAGE_SIZE, OSD_REPORT_SIZE, OSD_REQUEST_SIZE, OSD_REQUEST_SOURCE_AT,
    OSD_SETTING_BODY_AT, OSD_SYNC_SIZE, OSD_SYNC_SLOTS_AT, OSD_SYNC_SLOT_SIZE, OSD_SYNC_VERSION,
};

use super::handlers::{u16_at, u32_at, uid_at};
use super::Rx;

/// The most settings a sync may announce; a unit drops one announcing more.
const SYNC_RECORDS_MAX: u16 = OSD_SYNC_RECORDS_MAX as u16;

/// Reads an overlay sync frame of any kind.
///
/// A frame is dropped where a unit would drop it: shorter than its kind, or
/// of a payload version other than 2. A unit takes a sync, and the settings
/// that complete it, only from a management unit, so this client reads them
/// from no one else either. A request or report names the unit sending it
/// as its target, and is read only when it does. One asking for a new sync,
/// or naming this client as the source, is also queued for the runtime,
/// which answers it while this client holds a sync.
///
/// A unit answers for its own output only, so the message it reports is read
/// only when it names itself. A request for a message, or a write of one,
/// is for a unit with an output, which this client is not.
pub(super) fn osd_sync(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    let sender = rx.sender();
    if p.len() < OSD_HEADER_SIZE || p[0] != OSD_SYNC_VERSION {
        return;
    }
    let seq = u16_at(p, 2);
    let target = uid_at(p, 4);
    match p[1] {
        osd_kind::SYNC if is_management(state, sender) => {
            if let Some(sync) = sync(p, seq, target) {
                if let Some(device) = state.device_mut(sender) {
                    device.set_osd_sync(sync, ev);
                }
            }
        }
        osd_kind::REQUEST if p.len() >= OSD_REQUEST_SIZE => {
            request(state, sender, p, seq, target, ev);
        }
        osd_kind::SETTING if is_management(state, sender) => {
            if let Some(setting) = setting(p, seq, target) {
                ev.push(Event::OsdSyncSettingReceived {
                    device: sender,
                    setting,
                });
            }
        }
        osd_kind::REPORT if target == sender && p.len() >= OSD_REPORT_SIZE => {
            let report = OsdSyncReport {
                seq,
                state: OsdSyncState::from_wire(p[20]),
                written: u16_at(p, 22),
                refused: u16_at(p, 24),
                reason: cstr(&p[26..OSD_REPORT_SIZE]),
            };
            if let Some(device) = state.device_mut(sender) {
                device.set_osd_sync_report(report, ev);
            }
        }
        osd_kind::MESSAGE_STATE if target == sender => {
            if let Some(message) = message(p) {
                if let Some(device) = state.device_mut(sender) {
                    device.set_osd_message(message, ev);
                }
            }
        }
        _ => {}
    }
}

/// Reads a message, which a unit takes only with all the text it counts.
fn message(p: &[u8]) -> Option<OsdMessage> {
    let text = p.get(OSD_MESSAGE_SIZE..)?;
    let len = usize::from(u16_at(p, 24));
    let text = text.get(..len)?;
    Some(OsdMessage {
        flags: OsdMessageFlag::from_bits(p[20]),
        logo: p[21] as i8,
        priority: OsdMessagePriority::from_wire(p[22]),
        text: String::from_utf8_lossy(text).into_owned(),
    })
}

/// Whether a unit counts `uid` as management: the mesh controller, or a
/// device announcing itself as one.
/// Reads a request. A unit asks about itself; management may also ask for a
/// new sync for another unit. One this client answers is queued for the
/// runtime: one asking for a new sync, and one asking for settings of a sync
/// that names this client as its source or, from an older overlay module,
/// names none. The runtime leaves one naming none to the controller unless
/// this client sent that sync.
fn request(
    state: &mut State,
    sender: DeviceUid,
    p: &[u8],
    seq: u16,
    target: DeviceUid,
    ev: &mut Vec<Event>,
) {
    let by = if seq == 0 {
        target == sender || is_management(state, sender)
    } else {
        target == sender
    };
    if !by {
        return;
    }
    let request = OsdSyncRequest {
        target,
        seq,
        what: OsdSyncWhat::from_bits(p[20]),
        missing: core::array::from_fn(|i| p[22 + i]),
        source: uid_at(p, OSD_REQUEST_SOURCE_AT),
    };
    if seq == 0 || request.source == state.uid || request.source.is_zero() {
        state.osd_requests.push(request.clone());
    }
    ev.push(Event::OsdSyncRequested {
        device: sender,
        request,
    });
}

/// Whether units take an overlay sync from `uid`: it announces
/// [`DeviceFeature::MANAGER`], or it is the controller units follow, or, while
/// no unit has said whom it follows, it announces itself as one.
fn is_management(state: &State, uid: DeviceUid) -> bool {
    let Some(device) = state.device(uid) else {
        return false;
    };
    let followed = state.devices.values().filter(|d| !d.mesh_master.is_zero());
    device.hello.features.has(DeviceFeature::MANAGER)
        || followed.clone().any(|d| d.mesh_master == uid)
        || followed.count() == 0 && device.hello.features.has(DeviceFeature::MESH_MASTER)
}

fn sync(p: &[u8], seq: u16, target: DeviceUid) -> Option<OsdSync> {
    let records = p.get(..OSD_SYNC_SIZE).map(|_| u16_at(p, 28))?;
    if records > SYNC_RECORDS_MAX {
        return None;
    }
    let what = OsdSyncWhat::from_bits(p[20]);
    let slot = |i: usize| {
        let at = OSD_SYNC_SLOTS_AT + i * OSD_SYNC_SLOT_SIZE;
        OsdSyncSlot {
            bytes: u32_at(p, at),
            crc32: u32_at(p, at + 4),
            percent: p[at + 8],
        }
    };
    Some(OsdSync {
        seq,
        target,
        what,
        http_port: u16_at(p, 22),
        source_ip: std::net::Ipv4Addr::new(p[24], p[25], p[26], p[27]),
        records,
        slots: what
            .has(OsdSyncWhat::LOGO_SLOTS)
            .then(|| core::array::from_fn(slot)),
    })
}

fn setting(p: &[u8], seq: u16, target: DeviceUid) -> Option<OsdSyncSetting> {
    let body = p.get(OSD_SETTING_BODY_AT..)?;
    let index = u16_at(p, 20);
    let records = u16_at(p, 22);
    if index >= records {
        return None;
    }
    let i16_at = |at: usize| u16_at(body, at) as i16;
    let value = match p[24] {
        // A unit takes a caption only with all the text it counts.
        osd_setting::CAPTION if body.len() >= OSD_CAPTION_SIZE => {
            let text = &body[OSD_CAPTION_SIZE..];
            let len = usize::from(u16_at(body, 46));
            if text.len() < len {
                return None;
            }
            OsdSyncValue::Caption(OsdCaption {
                font: cstr(&body[..OSD_FONT_LEN]),
                size: u16_at(body, 32),
                x: i16_at(34),
                y: i16_at(36),
                wrap: i16_at(38),
                align: OsdCaptionAlign::from_wire(body[40]),
                format: OsdCaptionFormat::from_wire(body[41]),
                colour: u32_at(body, 42),
                text: String::from_utf8_lossy(&text[..len]).into_owned(),
            })
        }
        osd_setting::LOGO if body.len() >= OSD_LOGO_SIZE => OsdSyncValue::Logo(OsdLogo {
            logo: body[0] as i8,
            x: i16_at(2),
            y: i16_at(4),
        }),
        osd_setting::BACKGROUND if body.len() >= OSD_BACKGROUND_SIZE => {
            OsdSyncValue::Background(u32_at(body, 0))
        }
        _ => return None,
    };
    Some(OsdSyncSetting {
        seq,
        target,
        index,
        records,
        size: OsdSyncSize::from_wire(p[25]),
        layers: u16_at(p, 26),
        set: u16_at(p, 28),
        value,
    })
}
