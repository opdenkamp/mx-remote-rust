// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The fixed-size bay descriptor carried by the bay-config frames.

use super::enums::{BayFeatures, BayStatus, EdidProfile, MxrSignalType, RcType};
use super::frame::cstr;

/// Width of one bay descriptor on the wire.
pub(crate) const BAY_CONFIG_SIZE: usize = 61;

/// A single 61-byte bay descriptor from a bay-config frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BayConfig {
    pub(crate) port: u8,
    pub(crate) modenum: u8,
    pub(crate) bay: u8,
    pub(crate) video_source: u8,
    pub(crate) audio_source: u8,
    /// The same two bytes as `video_source` and `audio_source`, read as the
    /// 12-bit EDID profile an input bay reports there.
    pub(crate) edid_profile: EdidProfile,
    /// The upper nibble of `audio_source`, read as the remote-control type an
    /// input bay reports there.
    pub(crate) rc_type: RcType,
    pub(crate) bay_name: String,
    pub(crate) user_name: String,
    pub(crate) signal_type: String,
    pub(crate) signal_mode: MxrSignalType,
    pub(crate) status: BayStatus,
    pub(crate) features: BayFeatures,
}

/// Reads one descriptor, or `None` when fewer than [`BAY_CONFIG_SIZE`] bytes
/// are available.
pub(crate) fn parse_bay_config(p: &[u8]) -> Option<BayConfig> {
    let p = p.get(..BAY_CONFIG_SIZE)?;
    let u8_at = |i: usize| p.get(i).copied().unwrap_or(0);
    let u16_at = |i: usize| {
        p.get(i..i + 2)
            .and_then(|b| <[u8; 2]>::try_from(b).ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0)
    };
    let u32_at = |i: usize| {
        p.get(i..i + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0)
    };
    let str_at = |from: usize, to: usize| cstr(p.get(from..to).unwrap_or_default());

    let video_source = u8_at(3);
    let audio_source = u8_at(4);

    Some(BayConfig {
        port: u8_at(0),
        modenum: u8_at(1),
        bay: u8_at(2),
        video_source,
        audio_source,
        edid_profile: EdidProfile::from_wire(
            (u16::from(audio_source & 0x0F) << 8) | u16::from(video_source),
        ),
        rc_type: RcType::from_wire((audio_source >> 4) & 0x0F),
        bay_name: str_at(5, 21),
        user_name: str_at(21, 37),
        // mxr_cfg_signal is a 14-byte description followed by a 2-byte
        // mxr_signal_type, not a 16-byte string: a description filling its
        // field would otherwise run into the type bytes.
        signal_type: str_at(37, 51),
        signal_mode: MxrSignalType::from_wire(u16_at(51)),
        status: BayStatus::from_bits(u32_at(53)),
        features: BayFeatures::from_bits(u32_at(57)),
    })
}
