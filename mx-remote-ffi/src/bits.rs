// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The names behind the words the API hands over as plain integers.
//!
//! A feature word, a status word and a key code all cross the boundary as the
//! integer the wire carried, because firmware sets bits and sends values this
//! library has no name for and folding those onto a known set would lose them.
//! What a caller needs alongside is the names, so that every consumer does not
//! end up keeping its own copy of each bit position - which is the drift these
//! definitions exist to end.
//!
//! The values are spelled out rather than derived from the core crate's own
//! constants: cbindgen transliterates a constant's expression into the header
//! and cannot evaluate a call, so `DeviceFeature::IR_RX.bits()` would reach C
//! as text it cannot compile. `constants_match_the_core_crate` in
//! `tests/abi.rs` is what holds the two copies to the same values, and
//! `every_core_bit_has_a_name_here` is what stops one being added on one side
//! alone.

use mx_remote::{BayStatus, MxrSignalType};

use crate::abi::guard;

/// The signal format a device reports, packed into one 16-bit word.
///
/// Read it with the `mxr_signal_type_*` functions rather than by shifting it
/// directly: the bit depth in it is an index into a table rather than a depth,
/// and two different encodings both mean "nothing configured".
pub type mxr_signal_type_t = u16;

// ---- what a device says it can do, in `mxr_device_info_t::features` ----

/// Receives infrared.
pub const MXR_FEATURE_IR_RX: u32 = 1 << 0;

/// Transmits infrared.
pub const MXR_FEATURE_IR_TX: u32 = 1 << 1;

/// Speaks CEC.
pub const MXR_FEATURE_CEC: u32 = 1 << 2;

/// Acts as a V2IP stream source.
pub const MXR_FEATURE_V2IP_SOURCE: u32 = 1 << 3;

/// Acts as a V2IP stream sink.
pub const MXR_FEATURE_V2IP_SINK: u32 = 1 << 4;

/// Routes video.
pub const MXR_FEATURE_VIDEO_ROUTING: u32 = 1 << 5;

/// Routes audio.
pub const MXR_FEATURE_AUDIO_ROUTING: u32 = 1 << 6;

/// Controls volume.
pub const MXR_FEATURE_VOLUME_CONTROL: u32 = 1 << 7;

/// Supports audio return.
pub const MXR_FEATURE_AUDIO_RETURN: u32 = 1 << 8;

/// Passes remote-control commands through.
pub const MXR_FEATURE_REMOTE_CONTROL: u32 = 1 << 9;

/// Installer setup has been completed.
pub const MXR_FEATURE_SETUP_COMPLETED: u32 = 1 << 10;

/// Is the master of its mesh.
pub const MXR_FEATURE_MESH_MASTER: u32 = 1 << 11;

/// Has a notification pending.
pub const MXR_FEATURE_STATUS_NOTIFY: u32 = 1 << 12;

/// Has a warning pending.
pub const MXR_FEATURE_STATUS_WARNING: u32 = 1 << 13;

/// Has an error pending.
pub const MXR_FEATURE_STATUS_ERROR: u32 = 1 << 14;

/// Is about to reboot.
pub const MXR_FEATURE_STATUS_REBOOT: u32 = 1 << 15;

/// Is a member of a mesh.
pub const MXR_FEATURE_MESH_MEMBER: u32 = 1 << 16;

/// Is an audio amplifier.
pub const MXR_FEATURE_AUDIO_AMPLIFIER: u32 = 1 << 17;

/// Is still booting.
pub const MXR_FEATURE_BOOTING: u32 = 1 << 18;

/// Is a management client rather than a device.
pub const MXR_FEATURE_MANAGER: u32 = 1 << 19;

/// Is in power-save mode.
pub const MXR_FEATURE_STATUS_POWER_SAVE: u32 = 1 << 20;

/// Supports meshing.
pub const MXR_FEATURE_MESH: u32 = 1 << 21;

/// Is a multiviewer.
pub const MXR_FEATURE_MULTIVIEWER: u32 = 1 << 22;

/// Has crashed since it last booted.
pub const MXR_FEATURE_STATUS_CRASHED: u32 = 1 << 23;

/// Supports video walls.
pub const MXR_FEATURE_VIDEO_WALL: u32 = 1 << 24;

/// Initialises the configuration it broadcasts.
///
/// Firmware without this bit sends a device configuration built over
/// uninitialised memory, so fields it did not mean to write carry junk.
pub const MXR_FEATURE_CONFIG_INITIALISED: u32 = 1 << 25;

/// Set while the device is in its boot loader.
pub const MXR_FEATURE_BOOT_BIT: u32 = 1 << 31;

// ---- what a bay is wired for, in `mxr_bay_info_t::features` ----

/// HDMI output.
pub const MXR_BAY_HDMI_OUT: u32 = 1 << 0;

/// HDMI input.
pub const MXR_BAY_HDMI_IN: u32 = 1 << 1;

/// Digital audio output.
pub const MXR_BAY_AUDIO_DIG_OUT: u32 = 1 << 2;

/// Digital audio input.
pub const MXR_BAY_AUDIO_DIG_IN: u32 = 1 << 3;

/// Analogue audio output.
pub const MXR_BAY_AUDIO_ANA_OUT: u32 = 1 << 4;

/// Analogue audio input.
pub const MXR_BAY_AUDIO_ANA_IN: u32 = 1 << 5;

/// Infrared input.
pub const MXR_BAY_IR_IN: u32 = 1 << 6;

/// Infrared output.
pub const MXR_BAY_IR_OUT: u32 = 1 << 7;

/// Amplified audio output.
pub const MXR_BAY_AUDIO_AMP_OUT: u32 = 1 << 8;

/// Remote-control output.
pub const MXR_BAY_RC_OUT: u32 = 1 << 9;

/// Remote-control input.
pub const MXR_BAY_RC_IN: u32 = 1 << 10;

/// Dolby decoding.
pub const MXR_BAY_DOLBY: u32 = 1 << 11;

/// Switches itself off when idle.
pub const MXR_BAY_AUTO_OFF: u32 = 1 << 12;

/// Is a remote V2IP source.
pub const MXR_BAY_V2IP_SOURCE_REMOTE: u32 = 1 << 13;

/// Is a remote V2IP sink.
pub const MXR_BAY_V2IP_SINK_REMOTE: u32 = 1 << 14;

/// Is a local V2IP source.
pub const MXR_BAY_V2IP_SOURCE_LOCAL: u32 = 1 << 15;

/// Is a local V2IP sink.
pub const MXR_BAY_V2IP_SINK_LOCAL: u32 = 1 << 16;

// ---- the live status of a bay, in `mxr_signal_details_t::status` ----
//
// Bits 16-19 and 22-23 are bit-fields rather than flags. Read them with
// `mxr_bay_status_rc_type()` and `mxr_bay_status_hdcp()`.

/// The bay reports a fault.
pub const MXR_BAY_STATUS_FAULT: u32 = 1 << 0;

/// The bay is hidden from the user interface.
pub const MXR_BAY_STATUS_HIDDEN: u32 = 1 << 1;

/// The bay has power.
pub const MXR_BAY_STATUS_POWERED: u32 = 1 << 2;

/// A signal is present.
pub const MXR_BAY_STATUS_SIGNAL_DETECTED: u32 = 1 << 3;

/// Hot-plug detect is asserted.
pub const MXR_BAY_STATUS_HPD_DETECTED: u32 = 1 << 4;

/// The signal is scrambled.
pub const MXR_BAY_STATUS_SIGNAL_SCRAMBLE: u32 = 1 << 5;

/// An HDBaseT link is up.
pub const MXR_BAY_STATUS_HDBT_CONNECTED: u32 = 1 << 6;

/// A CEC device answered.
pub const MXR_BAY_STATUS_CEC_DETECTED: u32 = 1 << 7;

/// The attached device was powered on.
pub const MXR_BAY_STATUS_POWERED_ON: u32 = 1 << 8;

/// The attached device was powered off.
pub const MXR_BAY_STATUS_POWERED_OFF: u32 = 1 << 9;

/// Audio return over HDMI is active.
pub const MXR_BAY_STATUS_AUDIO_ARC_HDMI: u32 = 1 << 10;

/// Audio return over optical is active.
pub const MXR_BAY_STATUS_AUDIO_ARC_OPTIC: u32 = 1 << 11;

/// Audio return over analogue is active.
pub const MXR_BAY_STATUS_AUDIO_ARC_ANALOG: u32 = 1 << 12;

/// The bay is offline.
pub const MXR_BAY_STATUS_OFFLINE: u32 = 1 << 13;

/// The V2IP decoder is disabled.
pub const MXR_BAY_STATUS_DECODER_DISABLE: u32 = 1 << 14;

/// The V2IP encoder is disabled.
pub const MXR_BAY_STATUS_ENCODER_DISABLE: u32 = 1 << 15;

/// CEC is switched off for this bay.
pub const MXR_BAY_STATUS_CEC_DISABLED: u32 = 1 << 20;

/// The V2IP encoder reports an error.
pub const MXR_BAY_STATUS_ENCODER_ERROR: u32 = 1 << 21;

// ---- what an audio endpoint can do, in `mxr_audio_endpoint_t::features` ----

/// Accepts audio.
pub const MXR_AUDIO_INPUT: u32 = 1 << 0;

/// Produces audio.
pub const MXR_AUDIO_OUTPUT: u32 = 1 << 1;

/// Sends a V2IP audio stream.
pub const MXR_AUDIO_V2IP_TX: u32 = 1 << 2;

/// Receives a V2IP audio stream.
pub const MXR_AUDIO_V2IP_RX: u32 = 1 << 3;

/// Carries HDMI audio.
pub const MXR_AUDIO_HDMI: u32 = 1 << 4;

/// Is an analogue RCA connector.
pub const MXR_AUDIO_RCA: u32 = 1 << 5;

/// Is an S/PDIF connector.
pub const MXR_AUDIO_SPDIF: u32 = 1 << 6;

/// Drives a trigger output.
pub const MXR_AUDIO_TRIGGER: u32 = 1 << 7;

/// Can be muted.
pub const MXR_AUDIO_MUTE: u32 = 1 << 8;

/// Can be routed to as an input.
pub const MXR_AUDIO_ROUTE_INPUT: u32 = 1 << 9;

/// Can be routed from as an output.
pub const MXR_AUDIO_ROUTE_OUTPUT: u32 = 1 << 10;

/// Accepts "no input" as a route.
pub const MXR_AUDIO_ROUTE_IN_NONE: u32 = 1 << 11;

/// Is an amplifier output.
pub const MXR_AUDIO_AMP_OUTPUT: u32 = 1 << 12;

/// Has a volume control.
pub const MXR_AUDIO_VOLUME_CONTROL: u32 = 1 << 13;

/// Has a gain control.
pub const MXR_AUDIO_GAIN_CONTROL: u32 = 1 << 14;

// ---- remote-control keys, for `mxr_send_key()` ----
//
// A key above the last named here is sent as it is given: the range from
// `MXR_KEY_CUSTOM_CEC` carries a raw CEC user-control code and the one from
// `MXR_KEY_CUSTOM_SKY` a raw Sky code, each as the base plus the code.

/// Digit 0.
pub const MXR_KEY_NUM0: u16 = 0;

/// Digit 1.
pub const MXR_KEY_NUM1: u16 = 1;

/// Digit 2.
pub const MXR_KEY_NUM2: u16 = 2;

/// Digit 3.
pub const MXR_KEY_NUM3: u16 = 3;

/// Digit 4.
pub const MXR_KEY_NUM4: u16 = 4;

/// Digit 5.
pub const MXR_KEY_NUM5: u16 = 5;

/// Digit 6.
pub const MXR_KEY_NUM6: u16 = 6;

/// Digit 7.
pub const MXR_KEY_NUM7: u16 = 7;

/// Digit 8.
pub const MXR_KEY_NUM8: u16 = 8;

/// Digit 9.
pub const MXR_KEY_NUM9: u16 = 9;

/// Confirm the highlighted item.
pub const MXR_KEY_SELECT: u16 = 10;

/// Go back one step.
pub const MXR_KEY_BACK: u16 = 11;

/// Navigate up.
pub const MXR_KEY_UP: u16 = 12;

/// Navigate down.
pub const MXR_KEY_DOWN: u16 = 13;

/// Navigate left.
pub const MXR_KEY_LEFT: u16 = 14;

/// Navigate right.
pub const MXR_KEY_RIGHT: u16 = 15;

/// Open the main menu.
pub const MXR_KEY_MENU: u16 = 16;

/// Open the content menu.
pub const MXR_KEY_CONTENT_MENU: u16 = 17;

/// Next channel.
pub const MXR_KEY_CHANNEL_UP: u16 = 18;

/// Previous channel.
pub const MXR_KEY_CHANNEL_DOWN: u16 = 19;

/// Start playback.
pub const MXR_KEY_PLAY: u16 = 20;

/// Pause playback.
pub const MXR_KEY_PAUSE: u16 = 21;

/// Stop playback.
pub const MXR_KEY_STOP: u16 = 22;

/// Start recording.
pub const MXR_KEY_RECORD: u16 = 23;

/// Fast forward.
pub const MXR_KEY_FAST_FORWARD: u16 = 24;

/// Rewind.
pub const MXR_KEY_REWIND: u16 = 25;

/// Red colour key.
pub const MXR_KEY_RED: u16 = 26;

/// Green colour key.
pub const MXR_KEY_GREEN: u16 = 27;

/// Yellow colour key.
pub const MXR_KEY_YELLOW: u16 = 28;

/// Blue colour key.
pub const MXR_KEY_BLUE: u16 = 29;

/// Open help.
pub const MXR_KEY_HELP: u16 = 30;

/// Show information.
pub const MXR_KEY_INFORMATION: u16 = 31;

/// Open teletext.
pub const MXR_KEY_TEXT: u16 = 32;

/// Open the programme guide.
pub const MXR_KEY_GUIDE: u16 = 33;

/// Open video on demand.
pub const MXR_KEY_VIDEO_ON_DEMAND: u16 = 34;

/// Return to the previous channel.
pub const MXR_KEY_PREVIOUS_CHANNEL: u16 = 80;

/// Toggle 3D mode.
pub const MXR_KEY_MODE_3D: u16 = 81;

/// Toggle subtitles.
pub const MXR_KEY_SUBTITLE: u16 = 82;

/// Select an audio track.
pub const MXR_KEY_SOUND_SELECT: u16 = 83;

/// Select an input.
pub const MXR_KEY_INPUT_SELECT: u16 = 84;

/// Eject the medium.
pub const MXR_KEY_EJECT: u16 = 85;

/// Next chapter.
pub const MXR_KEY_NEXT_CHAPTER: u16 = 86;

/// Previous chapter.
pub const MXR_KEY_PREV_CHAPTER: u16 = 87;

/// Open interactive services.
pub const MXR_KEY_INTERACTIVE: u16 = 128;

/// Open search.
pub const MXR_KEY_SEARCH: u16 = 129;

/// Sky home key.
pub const MXR_KEY_SKY: u16 = 130;

/// Base of the range carrying a raw CEC user-control code.
pub const MXR_KEY_CUSTOM_CEC: u16 = 1280;

/// Base of the range carrying a raw Sky key code.
pub const MXR_KEY_CUSTOM_SKY: u16 = 2048;

// ---- multiviewer settings, for the `mxr_set_multiviewer_*` calls ----
//
// The same values come back in `mxr_multiviewer_status_t`, whose fields are
// the wire's bytes rather than a decoded set: a multiviewer running firmware
// newer than this library reports a mode named here by none of these, and
// passing it through is what lets a caller see it at all.

/// The multiviewer reports no window layout.
pub const MXR_MV_VIEW_MODE_UNKNOWN: u8 = 0;

/// One full-screen window.
pub const MXR_MV_VIEW_MODE_SINGLE: u8 = 1;

/// Picture in picture.
pub const MXR_MV_VIEW_MODE_PIP: u8 = 2;

/// Two windows, large.
pub const MXR_MV_VIEW_MODE_TWO_SCREEN_LARGE: u8 = 3;

/// Two windows, small.
pub const MXR_MV_VIEW_MODE_TWO_SCREEN_SMALL: u8 = 4;

/// Three windows, large.
pub const MXR_MV_VIEW_MODE_THREE_SCREEN_LARGE: u8 = 5;

/// Three windows, small.
pub const MXR_MV_VIEW_MODE_THREE_SCREEN_SMALL: u8 = 6;

/// Four windows, equal size.
pub const MXR_MV_VIEW_MODE_FOUR_SCREEN_EQUAL: u8 = 7;

/// Four windows, small.
pub const MXR_MV_VIEW_MODE_FOUR_SCREEN_SMALL: u8 = 8;

/// The multiviewer reports no picture-in-picture position.
pub const MXR_MV_PIP_POSITION_UNKNOWN: u8 = 0;

/// Top left.
pub const MXR_MV_PIP_POSITION_LEFT_TOP: u8 = 1;

/// Bottom left.
pub const MXR_MV_PIP_POSITION_LEFT_BOTTOM: u8 = 2;

/// Top right.
pub const MXR_MV_PIP_POSITION_RIGHT_TOP: u8 = 3;

/// Bottom right.
pub const MXR_MV_PIP_POSITION_RIGHT_BOTTOM: u8 = 4;

/// The multiviewer reports no picture-in-picture size.
pub const MXR_MV_PIP_SIZE_UNKNOWN: u8 = 0;

/// Small.
pub const MXR_MV_PIP_SIZE_SMALL: u8 = 1;

/// Medium.
pub const MXR_MV_PIP_SIZE_MEDIUM: u8 = 2;

/// Large.
pub const MXR_MV_PIP_SIZE_LARGE: u8 = 3;

/// The multiviewer reports no output mode.
pub const MXR_MV_OUTPUT_UNKNOWN: u8 = 0;

/// 4096x2160p60.
pub const MXR_MV_OUTPUT_DCI4K_P60: u8 = 1;

/// 4096x2160p50.
pub const MXR_MV_OUTPUT_DCI4K_P50: u8 = 2;

/// 3840x2160p60.
pub const MXR_MV_OUTPUT_UHD_P60: u8 = 3;

/// 3840x2160p50.
pub const MXR_MV_OUTPUT_UHD_P50: u8 = 4;

/// 3840x2160p30.
pub const MXR_MV_OUTPUT_UHD_P30: u8 = 5;

/// 3840x2160p25.
pub const MXR_MV_OUTPUT_UHD_P25: u8 = 6;

/// 1920x1200p60, reduced blanking.
pub const MXR_MV_OUTPUT_WUXGA_P60_RB: u8 = 7;

/// 1920x1080p60.
pub const MXR_MV_OUTPUT_HD1080_P60: u8 = 8;

/// 1920x1080p50.
pub const MXR_MV_OUTPUT_HD1080_P50: u8 = 9;

/// 1360x768p60.
pub const MXR_MV_OUTPUT_WXGA_P60: u8 = 10;

/// 1280x800p60.
pub const MXR_MV_OUTPUT_WXGA800_P60: u8 = 11;

/// 1280x720p60.
pub const MXR_MV_OUTPUT_HD720_P60: u8 = 12;

/// 1280x720p50.
pub const MXR_MV_OUTPUT_HD720_P50: u8 = 13;

/// 1024x768p60.
pub const MXR_MV_OUTPUT_XGA_P60: u8 = 14;

/// The multiviewer reports no HDCP mode.
pub const MXR_MV_HDCP_UNKNOWN: u8 = 0;

/// HDCP 1.4.
pub const MXR_MV_HDCP_V14: u8 = 1;

/// HDCP 2.2.
pub const MXR_MV_HDCP_V22: u8 = 2;

/// Content protection off.
pub const MXR_MV_HDCP_OFF: u8 = 3;

/// The multiviewer reports no EDID template.
pub const MXR_MV_EDID_UNKNOWN: u8 = 0;

/// 4K2K60 4:4:4, stereo 2.0.
pub const MXR_MV_EDID_4K2K60_444_STEREO: u8 = 1;

/// 4K2K60 4:4:4, Dolby/DTS 5.1.
pub const MXR_MV_EDID_4K2K60_444_DOLBY_DTS_51: u8 = 2;

/// 4K2K60 4:4:4, HD audio 7.1.
pub const MXR_MV_EDID_4K2K60_444_HD_AUDIO_71: u8 = 3;

/// 4K2K30 4:4:4, stereo 2.0.
pub const MXR_MV_EDID_4K2K30_444_STEREO: u8 = 4;

/// 4K2K30 4:4:4, Dolby/DTS 5.1.
pub const MXR_MV_EDID_4K2K30_444_DOLBY_DTS_51: u8 = 5;

/// 4K2K30 4:4:4, HD audio 7.1.
pub const MXR_MV_EDID_4K2K30_444_HD_AUDIO_71: u8 = 6;

/// 1080p, stereo 2.0.
pub const MXR_MV_EDID_1080P_STEREO: u8 = 7;

/// 1080p, Dolby/DTS 5.1.
pub const MXR_MV_EDID_1080P_DOLBY_DTS_51: u8 = 8;

/// 1080p, HD audio 7.1.
pub const MXR_MV_EDID_1080P_HD_AUDIO_71: u8 = 9;

/// 1920x1200, stereo 2.0.
pub const MXR_MV_EDID_1920X1200_STEREO: u8 = 10;

/// 1680x1050, stereo 2.0.
pub const MXR_MV_EDID_1680X1050_STEREO: u8 = 11;

/// 1600x1200, stereo 2.0.
pub const MXR_MV_EDID_1600X1200_STEREO: u8 = 12;

/// 1440x900, stereo 2.0.
pub const MXR_MV_EDID_1440X900_STEREO: u8 = 13;

/// 1360x768, stereo 2.0.
pub const MXR_MV_EDID_1360X768_STEREO: u8 = 14;

/// 1280x1024, stereo 2.0.
pub const MXR_MV_EDID_1280X1024_STEREO: u8 = 15;

/// 1024x768, stereo 2.0.
pub const MXR_MV_EDID_1024X768_STEREO: u8 = 16;

/// 720p, stereo 2.0.
pub const MXR_MV_EDID_720P_STEREO: u8 = 17;

/// Whatever the display connected to the HDMI output presents. The template a
/// multiviewer leaves the factory with.
pub const MXR_MV_EDID_COPY_OUTPUT: u8 = 18;

/// The EDID loaded onto the device.
pub const MXR_MV_EDID_CUSTOM: u8 = 19;

/// The multiviewer reports no IT-content mode.
pub const MXR_MV_ITC_UNKNOWN: u8 = 0;

/// Video content.
pub const MXR_MV_ITC_VIDEO: u8 = 1;

/// PC content.
pub const MXR_MV_ITC_PC: u8 = 2;

/// The multiviewer reports no aspect ratio.
pub const MXR_MV_ASPECT_UNKNOWN: u8 = 0;

/// Fill the window.
pub const MXR_MV_ASPECT_FULL: u8 = 1;

/// 16:9.
pub const MXR_MV_ASPECT_RATIO_16_9: u8 = 2;

/// Off.
pub const MXR_MV_BOOL_OFF: u8 = 0;

/// On.
pub const MXR_MV_BOOL_ON: u8 = 1;

/// The multiviewer reports no value.
pub const MXR_MV_BOOL_UNKNOWN: u8 = 255;

/// The multiviewer reports no source.
pub const MXR_MV_SOURCE_UNKNOWN: u8 = 0;

/// Input 1.
pub const MXR_MV_SOURCE_INPUT_1: u8 = 1;

/// Input 2.
pub const MXR_MV_SOURCE_INPUT_2: u8 = 2;

/// Input 3.
pub const MXR_MV_SOURCE_INPUT_3: u8 = 3;

/// Input 4.
pub const MXR_MV_SOURCE_INPUT_4: u8 = 4;

// ---- reading the packed words ----

/// The remote-control type a bay status word carries in bits 16-19.
#[no_mangle]
pub extern "C" fn mxr_bay_status_rc_type(status: u32) -> u8 {
    guard(0, || BayStatus::from_bits(status).rc_type().to_wire())
}

/// The HDCP version a bay status word carries in bits 22-23.
#[no_mangle]
pub extern "C" fn mxr_bay_status_hdcp(status: u32) -> u8 {
    guard(0, || BayStatus::from_bits(status).hdcp())
}

/// The CTA-861 short video descriptor, zero when the signal is not HDMI.
#[no_mangle]
pub extern "C" fn mxr_signal_type_svd(signal_type: mxr_signal_type_t) -> u8 {
    guard(0, || MxrSignalType::from_wire(signal_type).svd())
}

/// The colour space: 0 RGB, 1 4:4:4, 2 4:2:2, 3 4:2:0.
#[no_mangle]
pub extern "C" fn mxr_signal_type_colour_space(signal_type: mxr_signal_type_t) -> u8 {
    guard(0, || MxrSignalType::from_wire(signal_type).colour_space())
}

/// Whether the frame rate carries a 1000/1001 clock.
#[no_mangle]
pub extern "C" fn mxr_signal_type_non_integer(signal_type: mxr_signal_type_t) -> bool {
    guard(false, || {
        MxrSignalType::from_wire(signal_type).is_non_integer()
    })
}

/// The bit depth in bits per component, zero where the word names none.
///
/// The field on the wire is an index into a table of four depths, so it is not
/// the depth: a signal at 12 bits reads 3 there. This converts it.
#[no_mangle]
pub extern "C" fn mxr_signal_type_bpp(signal_type: mxr_signal_type_t) -> u8 {
    guard(0, || {
        MxrSignalType::from_wire(signal_type).bpp().unwrap_or(0)
    })
}

/// The raw bit-depth index, for a caller that wants the wire value.
///
/// See `mxr_signal_type_bpp()` for the depth it stands for.
#[no_mangle]
pub extern "C" fn mxr_signal_type_bpp_index(signal_type: mxr_signal_type_t) -> u8 {
    guard(0, || MxrSignalType::from_wire(signal_type).bpp_index())
}

/// Whether the word names a signal format at all.
///
/// A bay with nothing configured says so two ways: a sender that zeroes the
/// word and stamps the unset bit-depth index, and one that writes a plain
/// zero. Neither is a format, and the svd and colour space beside them are not
/// answers either - both read as zero, which is what this word says for "not
/// HDMI" and "RGB" when it is set.
#[no_mangle]
pub extern "C" fn mxr_signal_type_is_set(signal_type: mxr_signal_type_t) -> bool {
    guard(false, || MxrSignalType::from_wire(signal_type).is_set())
}
