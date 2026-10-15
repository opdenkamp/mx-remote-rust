// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Overlay sync, as a watcher sees it.

use crate::event::Event;
use crate::state::{Device, State};
use crate::types::{
    OsdCaption, OsdLogo, OsdSync, OsdSyncReport, OsdSyncRequest, OsdSyncSetting, OsdSyncSlot,
    OsdSyncValue,
};
use crate::wire::{
    cstr, DeviceUid, OsdCaptionAlign, OsdCaptionFormat, OsdSyncSize, OsdSyncState, OsdSyncWhat,
};

use super::handlers::{u16_at, u32_at, uid_at};
use super::Rx;

/// The payload version every frame opens with; a unit drops any other.
const OSD_SYNC_VERSION: u8 = 2;

/// Width of the header: version, kind, seq and target.
const HEADER_SIZE: usize = 20;

const KIND_SYNC: u8 = 0;
const KIND_REQUEST: u8 = 1;
const KIND_SETTING: u8 = 2;
const KIND_REPORT: u8 = 3;

/// Width of a sync: the header, what, the HTTP port, source address,
/// records, and four 12-byte slots from 32.
const SYNC_SIZE: usize = 80;
const SYNC_SLOTS_AT: usize = 32;
const SYNC_SLOT_SIZE: usize = 12;

/// The most settings a sync may announce; a unit drops one announcing more.
const SYNC_RECORDS_MAX: u16 = 128;

/// Width of a request without the source behind it, which units on an older
/// module send.
const REQUEST_SIZE: usize = 38;
const REQUEST_SOURCE_AT: usize = 38;

/// Width of a setting's own fields, where its body starts.
const SETTING_BODY_AT: usize = 30;
const SETTING_CAPTION: u8 = 0;
const SETTING_LOGO: u8 = 1;
const SETTING_BACKGROUND: u8 = 2;
/// A caption body: a 32-byte font, then size, x, y, wrap, align, format,
/// colour and the text's length, with the text behind it.
const CAPTION_SIZE: usize = 48;
const LOGO_SIZE: usize = 6;
const BACKGROUND_SIZE: usize = 4;

/// Width of a report: the header, state, written, refused and a 64-byte
/// reason.
const REPORT_SIZE: usize = 90;

/// Reads an overlay sync frame of any kind.
///
/// A frame is dropped where a unit would drop it: shorter than its kind, or
/// of a payload version other than 2. A unit takes a sync, and the settings
/// that complete it, only from a management unit, so this client reads them
/// from no one else either. A request or report names the unit sending it
/// as its target, and is read only when it does.
pub(super) fn osd_sync(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    let sender = rx.sender();
    if p.len() < HEADER_SIZE || p[0] != OSD_SYNC_VERSION {
        return;
    }
    let seq = u16_at(p, 2);
    let target = uid_at(p, 4);
    match p[1] {
        KIND_SYNC if is_management(state, sender) => {
            if let Some(sync) = sync(p, seq, target) {
                if let Some(device) = state.device_mut(sender) {
                    device.set_osd_sync(sync, ev);
                }
            }
        }
        KIND_REQUEST if target == sender && p.len() >= REQUEST_SIZE => {
            ev.push(Event::OsdSyncRequested {
                device: sender,
                request: OsdSyncRequest {
                    seq,
                    what: OsdSyncWhat::from_bits(p[20]),
                    missing: core::array::from_fn(|i| p[22 + i]),
                    source: uid_at(p, REQUEST_SOURCE_AT),
                },
            });
        }
        KIND_SETTING if is_management(state, sender) => {
            if let Some(setting) = setting(p, seq, target) {
                ev.push(Event::OsdSyncSettingReceived {
                    device: sender,
                    setting,
                });
            }
        }
        KIND_REPORT if target == sender && p.len() >= REPORT_SIZE => {
            let report = OsdSyncReport {
                seq,
                state: OsdSyncState::from_wire(p[20]),
                written: u16_at(p, 22),
                refused: u16_at(p, 24),
                reason: cstr(&p[26..REPORT_SIZE]),
            };
            if let Some(device) = state.device_mut(sender) {
                device.set_osd_sync_report(report, ev);
            }
        }
        _ => {}
    }
}

/// Whether a unit counts `uid` as management: the mesh controller, or a
/// device announcing itself as one.
fn is_management(state: &State, uid: DeviceUid) -> bool {
    state.device(uid).is_some_and(Device::is_management)
        || state.devices.values().any(|d| d.mesh_master == uid)
}

fn sync(p: &[u8], seq: u16, target: DeviceUid) -> Option<OsdSync> {
    let records = p.get(..SYNC_SIZE).map(|_| u16_at(p, 28))?;
    if records > SYNC_RECORDS_MAX {
        return None;
    }
    let what = OsdSyncWhat::from_bits(p[20]);
    let slot = |i: usize| {
        let at = SYNC_SLOTS_AT + i * SYNC_SLOT_SIZE;
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
    let body = p.get(SETTING_BODY_AT..)?;
    let index = u16_at(p, 20);
    let records = u16_at(p, 22);
    if index >= records {
        return None;
    }
    let i16_at = |at: usize| u16_at(body, at) as i16;
    let value = match p[24] {
        SETTING_CAPTION if body.len() >= CAPTION_SIZE => {
            let text = &body[CAPTION_SIZE..];
            let len = usize::from(u16_at(body, 46)).min(text.len());
            OsdSyncValue::Caption(OsdCaption {
                font: cstr(&body[..32]),
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
        SETTING_LOGO if body.len() >= LOGO_SIZE => OsdSyncValue::Logo(OsdLogo {
            logo: body[0] as i8,
            x: i16_at(2),
            y: i16_at(4),
        }),
        SETTING_BACKGROUND if body.len() >= BACKGROUND_SIZE => {
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
