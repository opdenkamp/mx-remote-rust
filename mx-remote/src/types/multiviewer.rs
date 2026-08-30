// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The state a OneIP multiviewer reports.

use crate::wire::{
    DeviceUid, MultiviewerAspectRatio, MultiviewerBool, MultiviewerEdidTemplate,
    MultiviewerHdcpMode, MultiviewerItcMode, MultiviewerOutputMode, MultiviewerPipPosition,
    MultiviewerPipSize, MultiviewerSource, MultiviewerViewMode,
};

/// How many windows a multiviewer can show, and how many sources it maps.
pub const MULTIVIEWER_INPUTS: usize = 4;

/// A multiviewer's complete reported state.
///
/// Every enumerated field passes an unrecognised wire value through as it
/// arrived, so a firmware that adds a mode reaches the caller as a value it
/// does not know rather than as whichever mode happens to be zero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiviewerStatus {
    /// The multiviewer this report describes.
    pub uid: DeviceUid,
    /// The source device mapped to each of the four inputs.
    pub mappings: [DeviceUid; MULTIVIEWER_INPUTS],
    /// The MCU firmware version.
    pub mcu_version: String,
    /// The scaler firmware version.
    pub scaler_version: String,
    /// The layout the hardware reports, which the firmware maps to `view_mode`.
    pub hw_view_mode: u8,
    /// The window layout.
    pub view_mode: MultiviewerViewMode,
    /// Where the picture-in-picture window sits.
    pub pip_position: MultiviewerPipPosition,
    /// How large the picture-in-picture window is.
    pub pip_size: MultiviewerPipSize,
    /// The output resolution and refresh rate.
    pub output_mode: MultiviewerOutputMode,
    /// The HDCP version negotiated on the output.
    pub hdcp_mode: MultiviewerHdcpMode,
    /// The IT-content flag set on the output.
    pub output_itc: MultiviewerItcMode,
    /// The EDID template presented to the sources.
    pub edid_template: MultiviewerEdidTemplate,
    /// The aspect ratio the windows are scaled to.
    pub aspect_ratio: MultiviewerAspectRatio,
    /// Whether the multiviewer switches windows on its own.
    pub auto_switch: MultiviewerBool,
    /// The window whose audio is being output.
    pub audio_source: MultiviewerSource,
    /// Output volume as a percentage, or `None` when the device reported a
    /// value outside 0..=100.
    pub audio_volume: Option<u8>,
    /// Whether the output is muted.
    pub audio_muted: MultiviewerBool,
    /// The source shown in each of the four windows.
    pub video_sources: [MultiviewerSource; MULTIVIEWER_INPUTS],
    /// The window receiving remote-control passthrough.
    pub remote_control: MultiviewerSource,
}
