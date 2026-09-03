// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The V2IP audio sub-protocol: the endpoint tree, its links, and the
//! per-endpoint notifications.

use crate::event::Event;
use crate::state::State;
use crate::types::{
    AudioChangeSource, AudioEndpoint, AudioEndpoints, AudioFeatures, AudioLink, StreamKind,
};

use crate::wire::audio_sub as sub;

use super::handlers::stream_source;
use super::Rx;

/// The entry kinds in a features report. One endpoint is described by several
/// entries, each naming the endpoint id it belongs to.
mod entry {
    pub(super) const ENDPOINT: u8 = 1;
    pub(super) const ADDRESS: u8 = 2;
    pub(super) const ROUTE_IN: u8 = 3;
    pub(super) const PARENT: u8 = 5;
}

/// Where the entry list starts, and how wide each entry is.
const ENTRY_BASE: usize = 36;
const ENTRY_SIZE: usize = 16;

fn entry_at(index: usize) -> usize {
    ENTRY_BASE + index * ENTRY_SIZE
}

/// Decodes a `SELECT_INPUT` body, which follows the 20-byte audio command
/// header: sink uid, source uid, then the sink and source endpoint ids.
///
/// Read from the body alone. The header names whichever device is addressed for
/// the hop that carried this frame, which is the sink only when the route has
/// one hop, so taking the sink from there would mislabel every longer route.
///
/// The receiving struct calls the first uid `source` and the second `target`,
/// the reverse of what they hold, and the module reads them accordingly. The
/// field names are the trap here: nothing on the wire tells a swapped reading
/// from a correct one.
fn change_source(rx: &Rx<'_>) -> Option<AudioChangeSource> {
    let f = &rx.frame;
    Some(AudioChangeSource {
        source_uid: f.uid(36)?,
        source_id: f.u16(54)?,
        target_uid: f.uid(20)?,
        target_id: f.u16(52)?,
    })
}

/// Decodes the endpoint tree from a features report.
///
/// Two passes: the first creates every endpoint so the second can attach
/// addresses, parents and routes to endpoints declared after them.
fn endpoints(rx: &Rx<'_>) -> AudioEndpoints {
    let f = &rx.frame;
    let mut eps = AudioEndpoints::default();
    let Some(count) = f.u16(28) else {
        return eps;
    };

    for index in 0..usize::from(count) {
        let base = entry_at(index);
        let Some(id) = f.u8(base) else {
            break;
        };
        if f.u8(base + 1) == Some(entry::ENDPOINT) {
            eps.add(AudioEndpoint {
                id,
                features: AudioFeatures::from_bits(f.u32(base + 8).unwrap_or(0)),
                ..AudioEndpoint::default()
            });
        }
    }

    for index in 0..usize::from(count) {
        let base = entry_at(index);
        let Some(id) = f.u8(base) else {
            break;
        };
        if eps.get(id).is_none() {
            continue;
        }
        match f.u8(base + 1) {
            Some(entry::ADDRESS) => {
                if let Some(bytes) = f.bytes_from(base + 8).filter(|b| b.len() >= 6) {
                    let address = stream_source(StreamKind::Audio, bytes, 0);
                    if let Some(ep) = eps.get_mut(id) {
                        ep.address = Some(address);
                    }
                }
            }
            Some(entry::PARENT) => {
                if let Some(parent) = f.u8(base + 8) {
                    if let Some(ep) = eps.get_mut(id) {
                        ep.parent = Some(parent);
                    }
                    if let Some(ep) = eps.get_mut(parent) {
                        ep.children.push(id);
                    }
                }
            }
            Some(entry::ROUTE_IN) => {
                if let (Some(available), Some(routed)) = (f.u32(base + 8), f.u32(base + 12)) {
                    if let Some(ep) = eps.get_mut(id) {
                        ep.inputs_available = Some(available);
                        ep.inputs_routed = Some(routed);
                    }
                }
            }
            _ => {}
        }
    }
    eps
}

/// Whether a features report carries a link block behind its entry list.
fn has_links(rx: &Rx<'_>, count: u16) -> bool {
    rx.frame.payload().len() > entry_at(usize::from(count))
}

/// Width of one entry in a link block.
const LINK_ENTRY: usize = 20;

fn links(rx: &Rx<'_>, idx: usize) -> Vec<AudioLink> {
    let f = &rx.frame;
    let Some(count) = f.u16(idx) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..usize::from(count) {
        let base = 4 + idx + index * LINK_ENTRY;
        let Some(endpoint) = f.u8(base) else {
            break;
        };
        let linked_device = f.uid(base + 4).unwrap_or(crate::wire::DeviceUid::ZERO);
        if linked_device.is_zero() {
            continue;
        }
        out.push(AudioLink {
            endpoint,
            linked_endpoint: f.u8(base + 1).unwrap_or(0),
            linked_device,
        });
    }
    out
}

pub(super) fn audio(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(op) = rx.frame.u16(0) else {
        return;
    };
    match op {
        sub::FEATURES => {
            let eps = endpoints(rx);
            let count = rx.frame.u16(28).unwrap_or(0);
            let links = has_links(rx, count).then(|| links(rx, entry_at(usize::from(count))));
            if let Some(device) = state.device_mut(rx.sender()) {
                device.set_audio_endpoints(eps, ev);
                if let Some(links) = links {
                    device.apply_audio_links(&links);
                }
            }
        }
        sub::LINKS => {
            let links = links(rx, 0);
            if let Some(device) = state.device_mut(rx.sender()) {
                device.apply_audio_links(&links);
            }
        }
        sub::SELECT_INPUT => {
            let Some(change) = change_source(rx) else {
                return;
            };
            if let Some(device) = state.device_mut(rx.sender()) {
                device.set_audio_select_input(change, ev);
            }
        }
        sub::MUTE | sub::TRIGGER | sub::VOLUME => {
            let (Some(endpoint), Some(value)) = (rx.frame.u16(20), rx.frame.u32(24)) else {
                return;
            };
            if state.device(rx.sender()).is_none() {
                return;
            }
            // Mute and trigger carry a boolean in the low bit of the same u32
            // that volume uses for a level.
            let device = rx.sender();
            ev.push(match op {
                sub::MUTE => Event::AudioEndpointMute {
                    device,
                    endpoint,
                    muted: value != 0,
                },
                sub::TRIGGER => Event::AudioEndpointTrigger {
                    device,
                    endpoint,
                    active: value != 0,
                },
                _ => Event::AudioEndpointVolume {
                    device,
                    endpoint,
                    volume: value,
                },
            });
        }
        _ => {}
    }
}
