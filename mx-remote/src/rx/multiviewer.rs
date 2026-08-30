// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The V2IP multiviewer sub-protocol.

use crate::event::Event;
use crate::state::State;
use crate::types::{MultiviewerCommand, MultiviewerStatus, MULTIVIEWER_INPUTS};
use crate::wire::{
    DeviceUid, MultiviewerAspectRatio, MultiviewerBool, MultiviewerEdidTemplate,
    MultiviewerHdcpMode, MultiviewerItcMode, MultiviewerOutputMode, MultiviewerPipPosition,
    MultiviewerPipSize, MultiviewerSource, MultiviewerViewMode,
};

use crate::wire::mv_sub as sub;

use super::Rx;

/// Where the parameter block of a command starts, behind the envelope of
/// target uid, sub-opcode and seven padding bytes.
const PARAMS_OFFSET: usize = 24;

/// Decodes a status report into the multiviewer's complete state.
fn status(rx: &Rx<'_>) -> MultiviewerStatus {
    let f = &rx.frame;
    let u8_at = |idx: usize| f.u8(idx).unwrap_or(0);
    let uid_at = |idx: usize| f.uid(idx).unwrap_or(DeviceUid::ZERO);
    let str_at = |idx: usize, len: usize| f.str(idx, len).unwrap_or_default();

    let mut mappings = [DeviceUid::ZERO; MULTIVIEWER_INPUTS];
    let mut video_sources = [MultiviewerSource::UNKNOWN; MULTIVIEWER_INPUTS];
    for (index, (mapping, source)) in mappings.iter_mut().zip(&mut video_sources).enumerate() {
        *mapping = uid_at(40 + index * 16);
        *source = MultiviewerSource::from_wire(u8_at(182 + index));
    }

    MultiviewerStatus {
        uid: uid_at(24),
        mappings,
        mcu_version: str_at(40 + 4 * 16, 32),
        scaler_version: str_at(40 + 6 * 16, 32),
        hw_view_mode: u8_at(168),
        view_mode: MultiviewerViewMode::from_wire(u8_at(169)),
        pip_position: MultiviewerPipPosition::from_wire(u8_at(170)),
        pip_size: MultiviewerPipSize::from_wire(u8_at(171)),
        output_mode: MultiviewerOutputMode::from_wire(u8_at(172)),
        hdcp_mode: MultiviewerHdcpMode::from_wire(u8_at(173)),
        output_itc: MultiviewerItcMode::from_wire(u8_at(174)),
        edid_template: MultiviewerEdidTemplate::from_wire(u8_at(175)),
        aspect_ratio: MultiviewerAspectRatio::from_wire(u8_at(177)),
        auto_switch: MultiviewerBool::from_wire_tristate(u8_at(178)),
        audio_source: MultiviewerSource::from_zero_based(u8_at(179)),
        // A volume is a percentage rather than an enumeration, so a value
        // outside its range is not reported rather than passed through.
        audio_volume: Some(u8_at(180)).filter(|v| *v <= 100),
        audio_muted: MultiviewerBool::from_wire_tristate(u8_at(181)),
        video_sources,
        remote_control: MultiviewerSource::from_zero_based(u8_at(186)),
    }
}

pub(super) fn multiviewer(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let Some(op) = rx.frame.u8(16) else {
        return;
    };
    if state.device(rx.sender()).is_none() {
        return;
    }
    if op == sub::STATUS {
        let status = status(rx);
        if let Some(device) = state.device_mut(rx.sender()) {
            device.set_multiviewer_status(status, ev);
        }
    }

    // The parameters are passed through as raw bytes. The opcode is owned by
    // the multiviewer module rather than by MatrixOS, so beyond the envelope
    // there is no firmware source here to pin per-sub-command field semantics
    // against.
    let params = rx
        .frame
        .payload()
        .get(PARAMS_OFFSET..)
        .unwrap_or_default()
        .to_vec();
    ev.push(Event::MultiviewerCommand {
        device: rx.sender(),
        command: MultiviewerCommand {
            target: rx.uid_or_zero(0),
            op,
            params,
        },
    });
}
