// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The control surface: what a caller can ask a device to do.
//!
//! Every call here returns `MXR_OK` only when a frame left the socket. There
//! is nothing further to wait for and nothing to acknowledge: a device answers
//! a command by reporting its new state a moment later, through the callbacks,
//! so a caller that needs confirmation waits for the event rather than for the
//! return.
//!
//! A device that speaks a protocol older than a command requires is refused
//! with `MXR_ERR_PROTOCOL_TOO_OLD` and nothing is sent, because such a device
//! discards the frame without answering and a send would report a success that
//! changed nothing.

use std::ffi::c_char;
use std::net::Ipv4Addr;
use std::time::{Duration, UNIX_EPOCH};

use mx_remote::{
    ControlError, DeviceUid, EdidProfile, MultiviewerAspectRatio, MultiviewerEdidTemplate,
    MultiviewerHdcpMode, MultiviewerItcMode, MultiviewerOutputMode, MultiviewerPipPosition,
    MultiviewerPipSize, MultiviewerSource, MultiviewerViewMode, RcAction, RcKey, UnitNetConfig,
    V2ipAudioFormat, V2ipColourSpace, V2ipDeviceSetting, V2ipDeviceSettings, V2ipOutputMode,
    V2ipPowerSaveLevel, V2ipPowerSaveSchedule, V2ipPtpMode, V2ipRoute, V2ipRouteTarget,
    V2ipTestPattern, V2ipVlan, V2ipVlanFlag, VideoWallWindow,
};
use mx_remote::{
    OsdCaption, OsdCaptionAlign, OsdCaptionFormat, OsdLogo, OsdMessagePriority, OsdSyncContent,
    OsdSyncEntry, OsdSyncImage, OsdSyncSize, OsdSyncValue, OsdSyncWhat,
};

use crate::abi::{
    fail, from_control, mxr_bay_uid_t, mxr_result_t, mxr_tribool_t, mxr_uid_t, opt_str, req_str,
};
use crate::bits::{
    MXR_OSD_SYNC_TYPE_BACKGROUND, MXR_OSD_SYNC_TYPE_CAPTION, MXR_OSD_SYNC_TYPE_LOGO,
};
use crate::info::mxr_amp_zone_settings_t;
use crate::remote::{mxr_remote_t, with};
use crate::subsystems::{
    mxr_v2ip_device_settings_t, mxr_v2ip_downmix_t, mxr_v2ip_power_save_t, mxr_v2ip_ptp_messages_t,
    mxr_v2ip_ptp_settings_t, mxr_v2ip_test_sync_t, mxr_v2ip_test_tone_t, mxr_v2ip_vlan_t,
    MXR_OSD_SYNC_SLOTS,
};

/// A stream's sample rate and channel count.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_audio_format_t {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u8,
}

impl From<mxr_audio_format_t> for V2ipAudioFormat {
    fn from(f: mxr_audio_format_t) -> Self {
        Self {
            sample_rate: f.sample_rate,
            channels: f.channels,
        }
    }
}

// ---- routing ----

/// Routes a OneIP sink's video to the stream a source port advertises.
///
/// The sink's audio then follows the video: a separate audio route is
/// dropped, so route audio after video to keep them apart. A unit below
/// protocol 0x27 is routed the source's audio stream instead, as some of
/// those route no audio for "follow the video".
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_select_video_source(
    remote: *const mxr_remote_t,
    sink: mxr_bay_uid_t,
    source_port: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.select_video_source(sink.into(), source_port))
    })
}

/// Routes a OneIP sink's audio to the stream a source port advertises,
/// leaving its video where it is.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_select_audio_source(
    remote: *const mxr_remote_t,
    sink: mxr_bay_uid_t,
    source_port: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.select_audio_source(sink.into(), source_port))
    })
}

/// Routes a OneIP sink's video to the source bay with this user-assigned name.
///
/// # Safety
///
/// `remote` is null or a live handle, and `name` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mxr_select_video_source_by_name(
    remote: *const mxr_remote_t,
    sink: mxr_bay_uid_t,
    name: *const c_char,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        match unsafe { req_str(name) } {
            Ok(name) => from_control(r.remote.select_video_source_by_name(sink.into(), name)),
            Err(code) => code,
        }
    })
}

/// Routes a OneIP sink's audio to the source bay with this user-assigned name.
///
/// `format` may be null to leave the sink's audio format alone.
///
/// # Safety
///
/// `remote` is null or a live handle, `name` is a NUL-terminated string, and
/// `format` is null or points at an initialised [`mxr_audio_format_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_select_audio_source_by_name(
    remote: *const mxr_remote_t,
    sink: mxr_bay_uid_t,
    name: *const c_char,
    format: *const mxr_audio_format_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        let name = match unsafe { req_str(name) } {
            Ok(name) => name,
            Err(code) => return code,
        };
        // SAFETY: the caller guarantees an initialised struct or null.
        let format = unsafe { format.as_ref() }.map(|f| (*f).into());
        from_control(
            r.remote
                .select_audio_source_by_name(sink.into(), name, format),
        )
    })
}

/// Routes a OneIP sink's audio to a multicast group directly, for a source this
/// client has not heard advertise it.
///
/// `audio_port` may be zero for the default, and `format` may be null to leave
/// the sink's audio format alone.
///
/// # Safety
///
/// `remote` is null or a live handle, `audio_ip` is a NUL-terminated dotted
/// quad, and `format` is null or points at an initialised
/// [`mxr_audio_format_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_select_audio_source_addr(
    remote: *const mxr_remote_t,
    sink: mxr_bay_uid_t,
    audio_ip: *const c_char,
    audio_port: u16,
    format: *const mxr_audio_format_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        let text = match unsafe { req_str(audio_ip) } {
            Ok(text) => text,
            Err(code) => return code,
        };
        let Ok(ip) = text.parse() else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                &format!("audio_ip is not an IPv4 address: {text:?}"),
            );
        };
        // SAFETY: the caller guarantees an initialised struct or null.
        let format = unsafe { format.as_ref() }.map(|f| (*f).into());
        from_control(r.remote.select_audio_source_addr(
            sink.into(),
            ip,
            // Zero is not a port a stream can arrive on, so it is how the
            // caller declines to name one.
            (audio_port != 0).then_some(audio_port),
            format,
        ))
    })
}

/// One stream of a route the caller assembles.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_stream_addr_t {
    /// The multicast group, as a dotted quad. Null or empty sends the slot
    /// zeroed, naming no group for that stream.
    ///
    /// It is not a way to leave one stream alone. The firmware decides
    /// whether a sink has a manual route at all by reading the video and
    /// ancillary slots, so an empty one of those disqualifies the whole
    /// route rather than preserving anything - see
    /// `mxr_select_source_addr()`.
    pub ip: *const c_char,
    /// The destination UDP port. Zero means the standard port for the stream
    /// this slot names.
    pub port: u16,
}

/// The three streams a manual route points a OneIP sink at.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_route_t {
    /// The video stream, at port 50020 unless the port says otherwise.
    pub video: mxr_stream_addr_t,
    /// The audio stream, at port 50022 unless the port says otherwise.
    pub audio: mxr_stream_addr_t,
    /// The ancillary-data stream, at port 50021 unless the port says
    /// otherwise.
    pub anc: mxr_stream_addr_t,
}

/// Reads one route slot, where a null or empty address means "not set".
///
/// # Safety
///
/// `slot.ip` is null or a NUL-terminated string.
unsafe fn to_target(slot: mxr_stream_addr_t, what: &str) -> Result<V2ipRouteTarget, mxr_result_t> {
    // SAFETY: the caller guarantees a NUL-terminated string or null.
    let text = unsafe { opt_str(slot.ip) }?.unwrap_or_default();
    if text.is_empty() {
        return Ok(V2ipRouteTarget::default());
    }
    match text.parse() {
        Ok(ip) => Ok(V2ipRouteTarget {
            ip,
            port: slot.port,
        }),
        Err(_) => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            &format!("{what} is not an IPv4 address: {text:?}"),
        )),
    }
}

/// Routes a OneIP sink's video, audio and ancillary streams to multicast groups
/// the caller names.
///
/// This is the only way to reach a stream no device on the mesh advertises,
/// such as one the calling program is transmitting itself; the routes by
/// source port and by name can only name a stream some bay has announced.
///
/// Set all three groups. The firmware decides whether a sink has a manual
/// route by looking at the video and ancillary groups, so a route that leaves
/// either unset does not register as one and the sink falls back to the audio
/// source its mesh picks.
///
/// A null `format` sends 48kHz stereo rather than omitting the field. The
/// firmware stores whatever the frame carries and hands it to the FPGA
/// unexamined, so a frame without a format leaves a zero sample rate there,
/// which the FPGA rejects and which takes the switch down with it.
///
/// # Safety
///
/// `remote` is null or a live handle, `route` points at an initialised
/// [`mxr_v2ip_route_t`] whose addresses are null or NUL-terminated strings,
/// and `format` is null or points at an initialised [`mxr_audio_format_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_select_source_addr(
    remote: *const mxr_remote_t,
    sink: mxr_bay_uid_t,
    route: *const mxr_v2ip_route_t,
    format: *const mxr_audio_format_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(route) = (unsafe { route.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the route pointer is null",
            );
        };
        // SAFETY: the caller guarantees NUL-terminated strings or null.
        let route = unsafe {
            match (
                to_target(route.video, "video ip"),
                to_target(route.audio, "audio ip"),
                to_target(route.anc, "anc ip"),
            ) {
                (Ok(video), Ok(audio), Ok(anc)) => V2ipRoute { video, audio, anc },
                (Err(code), _, _) | (_, Err(code), _) | (_, _, Err(code)) => return code,
            }
        };
        // SAFETY: the caller guarantees an initialised struct or null.
        let format = unsafe { format.as_ref() }.map(|f| (*f).into());
        from_control(r.remote.select_source_addr(sink.into(), route, format))
    })
}

// ---- bays ----

/// Renames a bay. The device stores the first 16 bytes.
///
/// # Safety
///
/// `remote` is null or a live handle, and `name` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_bay_name(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    name: *const c_char,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string.
        match unsafe { req_str(name) } {
            Ok(name) => from_control(r.remote.set_bay_name(bay.into(), name)),
            Err(code) => code,
        }
    })
}

/// Hides a bay from the installation's user interface, or shows it again.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_bay_hidden(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    hidden: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_bay_hidden(bay.into(), hidden))
    })
}

/// Switches an input bay's EDID profile.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_select_edid_profile(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    profile: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .select_edid_profile(bay.into(), EdidProfile::from_wire(profile)),
        )
    })
}

/// Sends a remote-control action to whatever is attached to a bay.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_action(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    action: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .send_action(bay.into(), RcAction::from_wire(action)),
        )
    })
}

/// Sends a remote-control key press to whatever is attached to a bay.
///
/// The device forwards it over CEC, infrared or IP, whichever that bay is
/// configured for; the caller does not choose. `key` is one of the `MXR_KEY_*`
/// values, or a raw code above `MXR_KEY_CUSTOM_CEC` or `MXR_KEY_CUSTOM_SKY`.
/// A value this library has no name for is sent as it was given.
///
/// `mxr_send_action()` names an outcome instead, and lets the device decide
/// which keys reach it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_key(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    key: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.send_key(bay.into(), RcKey::from_wire(key)))
    })
}

/// Powers on what is attached to a bay.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_power_on(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.power_on(bay.into())))
}

/// Powers off what is attached to a bay.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_power_off(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.power_off(bay.into())))
}

// ---- volume ----

/// Sets a bay's volume percentage, and its mute state when `muted` is not
/// `MXR_UNKNOWN`.
///
/// A bay with no volume control of its own is set through its `linked_bay`,
/// so an output wired to an amplifier zone reaches that zone.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_volume(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    volume: u8,
    muted: mxr_tribool_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_volume(
            bay.into(),
            volume,
            match muted {
                mxr_tribool_t::MXR_UNKNOWN => None,
                mxr_tribool_t::MXR_FALSE => Some(false),
                mxr_tribool_t::MXR_TRUE => Some(true),
            },
        ))
    })
}

/// Asks a bay to step its volume up.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_volume_up(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.volume_up(bay.into())))
}

/// Asks a bay to step its volume down.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_volume_down(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.volume_down(bay.into())))
}

/// Mutes or unmutes a bay, leaving its volume alone.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_muted(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    muted: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_muted(bay.into(), muted))
    })
}

/// Writes an amplifier zone's gain, delay, tone and power settings.
///
/// This replaces every setting at once, so a caller changing one reads the
/// current set with `mxr_bay_amp_settings()`
/// first.
///
/// # Safety
///
/// `remote` is null or a live handle, and `settings` points at an initialised
/// [`mxr_amp_zone_settings_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_set_amp_zone_settings(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    settings: *const mxr_amp_zone_settings_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(settings) = (unsafe { settings.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the amp zone settings pointer is null",
            );
        };
        from_control(
            r.remote
                .set_amp_zone_settings(bay.into(), (*settings).into()),
        )
    })
}

// ---- audio endpoints ----

/// Mutes or unmutes one of a device's audio endpoints.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it. Refused with
/// `MXR_ERR_UNSUPPORTED` unless the endpoint reports the MUTE feature.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_audio_endpoint_muted(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    endpoint: u16,
    muted: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_audio_endpoint_muted(device.into(), endpoint, muted),
        )
    })
}

/// Locks or unlocks an audio endpoint's source: while it is locked, a video
/// route change leaves the endpoint's audio source alone.
///
/// `MXR_ERR_NOT_REPORTED` before the device has reported its audio endpoints,
/// and `MXR_ERR_UNSUPPORTED` for an endpoint without `MXR_AUDIO_AUDIO_LOCK`,
/// sending nothing in either case. The device reports its endpoints again
/// once the lock has changed; `mxr_audio_endpoint_status()` reads it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_audio_endpoint_locked(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    endpoint: u8,
    locked: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_audio_endpoint_locked(device.into(), endpoint, locked),
        )
    })
}

/// Activates or clears an audio endpoint's trigger.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it. Refused with
/// `MXR_ERR_UNSUPPORTED` unless the endpoint reports the TRIGGER feature.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_audio_endpoint_trigger(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    endpoint: u16,
    active: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_audio_endpoint_trigger(device.into(), endpoint, active),
        )
    })
}

/// Sets an audio endpoint's volume. Always `MXR_ERR_UNSUPPORTED`.
///
/// No unit acts on this command: the audio module defines it but neither
/// sends nor handles it, and a unit on released firmware reads the same
/// sub-opcode as a request to select an output.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_audio_endpoint_volume(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    endpoint: u16,
    volume: u32,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_audio_endpoint_volume(device.into(), endpoint, volume),
        )
    })
}

/// Points one device's audio endpoint at another device's.
///
/// `sink` is the end doing the listening and `source` the end being
/// listened to.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// A unit on released firmware up to 4.4.2 routes only between its own
/// endpoints: `MXR_ERR_UNSUPPORTED` for a route from another unit to one.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_select_audio_endpoint_input(
    remote: *const mxr_remote_t,
    sink: mxr_uid_t,
    sink_endpoint: u16,
    source: mxr_uid_t,
    source_endpoint: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.select_audio_endpoint_input(
            sink.into(),
            sink_endpoint,
            source.into(),
            source_endpoint,
        ))
    })
}

// ---- devices ----

/// Asks a device for an EDID: the one the display on its output publishes, or
/// the one the device presents to the source on its input.
///
/// The device answers a moment later. The bytes reach `on_edid_received` and
/// stay readable through `mxr_device_edid()`.
///
/// Only OneIP hardware handles this opcode. A matrix or an amplifier accepts
/// the frame and answers nothing, at any protocol version, so the silence that
/// follows is permanent rather than a reply still to come. `MXR_OK` here means
/// the frame was sent, and nothing more; a caller polling for an EDID should
/// ask a device that can answer rather than wait on one that cannot.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_edid(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    output: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_edid(device.into(), output))
    })
}

/// Asks for a detailed signal report from every bay of one device, or - with
/// the zero uid - from every bay on the network.
///
/// Devices report on their own when a signal changes, so this is what a client
/// that has just started needs: without it, a bay that has been showing the
/// same picture for an hour says nothing until it changes.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_signal_status(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let uid = DeviceUid::from(device);
        let target = (uid != DeviceUid::ZERO).then_some(uid);
        from_control(r.remote.request_signal_status(target))
    })
}

/// Subscribes to, or unsubscribes from, a device's OneIP statistics.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_subscribe_v2ip_stats(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    subscribe: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.subscribe_v2ip_stats(device.into(), subscribe))
    })
}

/// Reboots a device.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_reboot(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.reboot(device.into())))
}

/// Asks a device to announce itself now, so a device suspected to be gone is
/// confirmed or ruled out within a second or two.
///
/// `MXR_ERR_PROTOCOL_TOO_OLD` for a device below protocol 0x2A, which does not
/// answer one.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_ping(remote: *const mxr_remote_t, device: mxr_uid_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.ping(device.into())))
}

/// Hands a OneIP source's stream addresses back to automatic assignment,
/// undoing addresses that were set on it by hand. The device's next
/// configuration report carries the addresses it ends up with.
///
/// `MXR_ERR_UNSUPPORTED` for a device that is not a OneIP source, and
/// `MXR_ERR_PROTOCOL_TOO_OLD` for one below protocol 0x2B, which ignores it,
/// sending nothing in either case.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_auto_assign_v2ip_source_addresses(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.auto_assign_v2ip_source_addresses(device.into()))
    })
}

/// Sets the time zone of every device that hears it: `zone` an IANA name such
/// as `Europe/Amsterdam`, `rule` the POSIX TZ rule the devices keep time by.
///
/// `MXR_ERR_INVALID_ARGUMENT` for an empty string, or one of
/// `MXR_TIME_ZONE_NAME_LEN` or `MXR_TIME_ZONE_RULE_LEN` bytes or more. The
/// mesh controller takes it too and announces it from then on.
///
/// # Safety
///
/// `remote` is null or a live handle, and `zone` and `rule` are
/// NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_mesh_time_zone(
    remote: *const mxr_remote_t,
    zone: *const c_char,
    rule: *const c_char,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees NUL-terminated strings.
        match unsafe { (req_str(zone), req_str(rule)) } {
            (Ok(zone), Ok(rule)) => from_control(r.remote.set_mesh_time_zone(zone, rule)),
            (Err(code), _) | (_, Err(code)) => code,
        }
    })
}

/// Clears the time zone of every device that hears it, which then keeps UTC:
/// `mxr_set_mesh_time_zone()` with an empty name and rule, which that call
/// refuses.
///
/// # Safety
///
/// `remote` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn mxr_clear_mesh_time_zone(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.clear_mesh_time_zone()))
}

/// Sets the clock of every device that hears it to `utc`, in seconds since
/// 1970 UTC. A device keeps its own clock where that is within 2s.
///
/// `MXR_ERR_INVALID_ARGUMENT` for a time past what 32 bits of seconds hold.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_mesh_time(remote: *const mxr_remote_t, utc: u64) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let time = UNIX_EPOCH.checked_add(Duration::from_secs(utc));
        match time {
            Some(time) => from_control(r.remote.set_mesh_time(time)),
            None => fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the time does not fit the frame",
            ),
        }
    })
}

/// Tells every unit to check its update server for new firmware now.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_monitoring_pulse(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| from_control(r.remote.send_monitoring_pulse()))
}

// ---- OneIP scaling ----

/// The colour space a OneIP sink scales its output to.
pub const MXR_V2IP_COLOUR_RGB: u8 = 0;
/// YCbCr 4:4:4. See `MXR_V2IP_COLOUR_RGB`.
pub const MXR_V2IP_COLOUR_YCBCR444: u8 = 1;
/// YCbCr 4:2:2. See `MXR_V2IP_COLOUR_RGB`.
pub const MXR_V2IP_COLOUR_YCBCR422: u8 = 2;
/// YCbCr 4:2:0. See `MXR_V2IP_COLOUR_RGB`.
pub const MXR_V2IP_COLOUR_YCBCR420: u8 = 3;

/// Lowest refresh rate a OneIP output stage accepts, in Hz.
pub const MXR_V2IP_SCALING_REFRESH_MIN: u16 = 24;

/// Highest refresh rate a OneIP output stage accepts, in Hz.
pub const MXR_V2IP_SCALING_REFRESH_MAX: u16 = 120;

/// The output format to scale a OneIP sink to.
///
/// Given as a depth and a colour space rather than as a packed signal-type
/// word, so a caller cannot send the "no depth" index a sink reports while it
/// has none configured - a value a sink decodes cleanly and then drops.
///
/// `svd` must name a known video descriptor and may not be zero, `depth` must
/// be 8, 10 or 12, `colour` one of the `MXR_V2IP_COLOUR_*` values, and
/// `refresh` between `MXR_V2IP_SCALING_REFRESH_MIN` and
/// `MXR_V2IP_SCALING_REFRESH_MAX`. Each is checked before anything is sent.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_v2ip_output_mode_t {
    /// The CTA-861 short video descriptor to output.
    pub svd: u8,
    /// Bit depth: 8, 10 or 12.
    pub depth: u8,
    /// One of the `MXR_V2IP_COLOUR_*` values.
    pub colour: u8,
    /// Refresh rate in Hz.
    pub refresh: u16,
}

impl From<mxr_v2ip_output_mode_t> for V2ipOutputMode {
    fn from(m: mxr_v2ip_output_mode_t) -> Self {
        Self {
            svd: m.svd,
            depth: m.depth,
            colour: V2ipColourSpace::from_wire(m.colour),
            refresh: m.refresh,
        }
    }
}

/// Reads a mode argument, refusing a null pointer.
unsafe fn output_mode(mode: *const mxr_v2ip_output_mode_t) -> Result<V2ipOutputMode, mxr_result_t> {
    // SAFETY: the caller guarantees an initialised struct or null.
    match unsafe { mode.as_ref() } {
        Some(m) => Ok((*m).into()),
        None => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "the mode pointer is null",
        )),
    }
}

/// Turns a OneIP sink's automatic scaling on or off.
///
/// Automatic scaling and a configured output mode are separate reasons for a
/// sink to scale, and this moves only the first: a sink with a mode configured
/// goes on scaling to it with automatic scaling off. Turning both off is this
/// call plus `mxr_clear_v2ip_output_mode()`.
///
/// Nothing acknowledges the frame, so `MXR_OK` means it was sent. Read the sink
/// back with `mxr_v2ip_details()`, and trust the scaling fields only where the
/// device reports `MXR_FEATURE_CONFIG_INITIALISED`.
///
/// **Read any route you still need before writing.** The sink rebuilds and
/// rebroadcasts its subscription in response, and the addresses in
/// `mxr_v2ip_details()` can read as zero for up to a minute afterwards.
///
/// `MXR_ERR_UNSUPPORTED` for a unit below protocol 0x2F: released firmware
/// drops a scaling write from management.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_auto_scaling(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    enabled: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_auto_scaling(device.into(), enabled))
    })
}

/// Switches on/off settings of a OneIP device, all to the same value.
///
/// `setting` is one or more `MXR_V2IP_SETTING_*` on/off bits, each one the
/// device has reported. Returns `MXR_ERR_NOT_REPORTED` before the device has
/// reported its settings, `MXR_ERR_UNSUPPORTED` for a setting it does not have
/// and `MXR_ERR_INVALID_ARGUMENT` for a bit that is not an on/off setting,
/// sending nothing in each case: the device ignores such a write. Until the
/// device reports back, `mxr_v2ip_device_settings()` reads what was written.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_device_setting(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    setting: u32,
    enabled: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_device_setting(
            device.into(),
            V2ipDeviceSetting::from_bits(setting),
            enabled,
        ))
    })
}

/// Sets the infrared profile of a OneIP device's global infrared port.
///
/// `profile` is from 0 up to, not including, `MXR_V2IP_IR_PROFILE_MAX`.
/// Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_ir_profile(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    profile: i8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_ir_profile(device.into(), profile))
    })
}

/// Sets how many idle minutes a OneIP device waits before it powers down by
/// itself, 0 for never. Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_auto_power_save(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    minutes: u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_auto_power_save(device.into(), minutes))
    })
}

/// Sets a OneIP device's daily power save windows from `schedule`'s `start`
/// and `end`; its `auto_minutes` is not sent.
///
/// `MXR_ERR_INVALID_ARGUMENT` for a time not below
/// `MXR_V2IP_MINUTES_PER_DAY`. Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `schedule`
/// is null or points at an initialised `mxr_v2ip_power_save_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_power_save_schedule(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    schedule: *const mxr_v2ip_power_save_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(schedule) = (unsafe { schedule.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the schedule pointer is null",
            );
        };
        from_control(r.remote.set_v2ip_power_save_schedule(
            device.into(),
            V2ipPowerSaveSchedule {
                start: schedule.start,
                end: schedule.end,
            },
        ))
    })
}

/// Changes settings on every OneIP device of the mesh with one frame.
///
/// `settings` carries the settings behind their `MXR_V2IP_SETTING_*` bits in
/// `valid`, as `mxr_v2ip_device_settings()` reports them; its `ir_profiles`
/// is not sent. `power_save` carries the idle minutes and the schedule, and
/// may be null when `valid` has neither. Each device applies the settings it
/// has and ignores the rest.
///
/// `MXR_ERR_INVALID_ARGUMENT`, sending nothing, when no setting is carried,
/// when one only a device reports is, for a PTP setting, which
/// `mxr_set_all_v2ip_ptp_settings()` carries, for the power save level, which
/// `mxr_set_all_v2ip_power_save_level()` carries, for a PTP message, SAP
/// timing, stream TTL or rotation setting, which
/// `mxr_set_all_v2ip_ptp_messages()` carries, for a profile out of range or a schedule time not below
/// `MXR_V2IP_MINUTES_PER_DAY`. Nothing is cached: a device that applies a
/// change reports it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, `settings` is
/// null or points at an initialised `mxr_v2ip_device_settings_t`, and
/// `power_save` is null or points at an initialised `mxr_v2ip_power_save_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_all_v2ip_device_settings(
    remote: *const mxr_remote_t,
    settings: *const mxr_v2ip_device_settings_t,
    power_save: *const mxr_v2ip_power_save_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(settings) = (unsafe { settings.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the settings pointer is null",
            );
        };
        let valid = V2ipDeviceSetting::from_bits(settings.valid);
        if !(valid & V2ipDeviceSetting::PTP_SETTINGS).is_empty() {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "PTP settings go through mxr_set_all_v2ip_ptp_settings()",
            );
        }
        if valid.has(V2ipDeviceSetting::POWER_SAVE_LEVEL) {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the power save level goes through mxr_set_all_v2ip_power_save_level()",
            );
        }
        if !(valid & V2ipDeviceSetting::SETTINGS_BLOCK).is_empty() {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "PTP message, SAP timing, stream TTL and rotation settings go through mxr_set_all_v2ip_ptp_messages()",
            );
        }
        let needs_power_save = valid.has(V2ipDeviceSetting::AUTO_POWER_SAVE)
            || valid.has(V2ipDeviceSetting::POWER_SAVE_SCHEDULE);
        // SAFETY: the caller guarantees an initialised struct or null.
        let power_save = match unsafe { power_save.as_ref() } {
            Some(p) => *p,
            None if needs_power_save => {
                return fail(
                    mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                    "the power save pointer is null",
                )
            }
            None => mxr_v2ip_power_save_t {
                auto_minutes: 0,
                start: [0; 7],
                end: [0; 7],
            },
        };
        from_control(r.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            valid,
            flags: V2ipDeviceSetting::from_bits(settings.flags),
            ir_profiles: 0,
            ir_profile: settings.ir_profile,
            ir_profile_sink: settings.ir_profile_sink,
            auto_power_save: power_save.auto_minutes,
            power_save: V2ipPowerSaveSchedule {
                start: power_save.start,
                end: power_save.end,
            },
            ..V2ipDeviceSettings::default()
        }))
    })
}

/// Sets the level a OneIP device powers down to in power save,
/// `MXR_V2IP_POWER_SAVE_LEVEL_*`. `MXR_ERR_INVALID_ARGUMENT` for a level with no
/// name. `mxr_unit_power_save()` says which levels the unit offers. It also
/// switches `MXR_V2IP_SETTING_POWER_SAVE_DISABLED` off. Otherwise as
/// `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_power_save_level(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    level: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_v2ip_power_save_level(device.into(), V2ipPowerSaveLevel::from_wire(level)),
        )
    })
}

/// Sets the power save level of every OneIP device of the mesh with one frame,
/// `MXR_V2IP_POWER_SAVE_LEVEL_*`. `MXR_ERR_INVALID_ARGUMENT`, sending nothing,
/// for a level with no name. Each device that takes the level also switches
/// `MXR_V2IP_SETTING_POWER_SAVE_DISABLED` off. Otherwise as
/// `mxr_set_all_v2ip_device_settings()`,
/// which carries the other settings.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_all_v2ip_power_save_level(
    remote: *const mxr_remote_t,
    level: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            valid: V2ipDeviceSetting::POWER_SAVE_LEVEL,
            power_save_level: V2ipPowerSaveLevel::from_wire(level),
            ..V2ipDeviceSettings::default()
        }))
    })
}

/// Sets how a OneIP device paces its PTP messages, its SAP timings, its
/// streams' TTL and its output's rotation: the values of `messages` behind
/// the `MXR_V2IP_SETTING_PTP_*` message bits, `MXR_V2IP_SETTING_SAP_EXPIRE`
/// and `_SAP_INTERVAL`, `MXR_V2IP_SETTING_STREAM_TTL` and
/// `MXR_V2IP_SETTING_SINK_ROTATION` in `valid`. `MXR_ERR_INVALID_ARGUMENT`,
/// sending nothing, for a null `messages`, for no such bit or any other bit in
/// `valid`, for a DSCP above 63, for a stream TTL of 0, or for a rotation that
/// is not 0 or a multiple of the `sink_rotation_step` the device reports.
/// Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `messages`
/// is null or points at an initialised `mxr_v2ip_ptp_messages_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_ptp_messages(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    valid: u32,
    messages: *const mxr_v2ip_ptp_messages_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(messages) = (unsafe { messages.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the PTP messages pointer is null",
            );
        };
        from_control(r.remote.set_v2ip_ptp_messages(
            device.into(),
            V2ipDeviceSetting::from_bits(valid),
            (*messages).into(),
        ))
    })
}

/// Sets how a OneIP device's output mixes 5.1 and 7.1 PCM down to stereo: the
/// `mode`, `level` and `lfe` of `downmix`, which the device applies together;
/// `state` and `channels` are ignored. `MXR_ERR_INVALID_ARGUMENT`, sending
/// nothing, for a null `downmix` or a mode, level or LFE percent out of range,
/// and `MXR_ERR_UNSUPPORTED` for a device that reports no downmix, as
/// `mxr_v2ip_downmix()` reads it. Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `downmix`
/// is null or points at an initialised `mxr_v2ip_downmix_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_downmix(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    downmix: *const mxr_v2ip_downmix_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(downmix) = (unsafe { downmix.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the downmix pointer is null",
            );
        };
        from_control(r.remote.set_v2ip_downmix(device.into(), (*downmix).into()))
    })
}

/// Changes how every OneIP device of the mesh paces its PTP messages, with one
/// frame. As `mxr_set_v2ip_ptp_messages()` for its arguments, except that a
/// rotation is refused only above `MXR_V2IP_SINK_ROTATION_MAX`, and each
/// device takes it only by its own step. Otherwise as
/// `mxr_set_all_v2ip_device_settings()`, which carries the other settings.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `messages`
/// is null or points at an initialised `mxr_v2ip_ptp_messages_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_all_v2ip_ptp_messages(
    remote: *const mxr_remote_t,
    valid: u32,
    messages: *const mxr_v2ip_ptp_messages_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(messages) = (unsafe { messages.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the PTP messages pointer is null",
            );
        };
        let valid = V2ipDeviceSetting::from_bits(valid);
        if valid.is_empty() || !V2ipDeviceSetting::SETTINGS_BLOCK.has(valid) {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "only PTP message, SAP timing, stream TTL and rotation settings are carried here",
            );
        }
        from_control(r.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            valid,
            ptp_messages: (*messages).into(),
            ..V2ipDeviceSettings::default()
        }))
    })
}

/// Sets the PTP mode a OneIP device runs, `MXR_V2IP_PTP_MODE_*`.
/// `MXR_ERR_INVALID_ARGUMENT` for a mode with no name. Otherwise as
/// `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_ptp_mode(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    mode: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_v2ip_ptp_mode(device.into(), V2ipPtpMode::from_wire(mode)),
        )
    })
}

/// Sets the PTP domain a OneIP device runs in. `MXR_ERR_INVALID_ARGUMENT` above
/// `MXR_V2IP_PTP_DOMAIN_MAX`. Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_ptp_domain(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    domain: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_ptp_domain(device.into(), domain))
    })
}

/// Sets the priority1 a OneIP device announces as a PTP grandmaster, lower
/// winning. Otherwise as `mxr_set_v2ip_device_setting()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_ptp_priority1(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    priority1: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_ptp_priority1(device.into(), priority1))
    })
}

/// Changes PTP settings on every OneIP device of the mesh with one frame.
///
/// `valid` names the `MXR_V2IP_SETTING_PTP_*` bits to send, and `ptp` carries
/// their values. `MXR_ERR_INVALID_ARGUMENT`, sending nothing, for a null `ptp`,
/// for no PTP bit or any other bit in `valid`, or for a mode or domain out of
/// range. Otherwise as `mxr_set_all_v2ip_device_settings()`, which carries the
/// other settings.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `ptp` is
/// null or points at an initialised `mxr_v2ip_ptp_settings_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_all_v2ip_ptp_settings(
    remote: *const mxr_remote_t,
    valid: u32,
    ptp: *const mxr_v2ip_ptp_settings_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(ptp) = (unsafe { ptp.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the PTP settings pointer is null",
            );
        };
        let valid = V2ipDeviceSetting::from_bits(valid);
        if valid.is_empty() || !V2ipDeviceSetting::PTP_SETTINGS.has(valid) {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "only PTP settings are carried here",
            );
        }
        from_control(r.remote.set_all_v2ip_device_settings(V2ipDeviceSettings {
            valid,
            ptp_mode: V2ipPtpMode::from_wire(ptp.mode),
            ptp_domain: ptp.domain,
            ptp_priority1: ptp.priority1,
            ..V2ipDeviceSettings::default()
        }))
    })
}

/// Has a OneIP output play the stream a session description describes.
///
/// The description goes out in pages of up to 1 KB, and the output plays it
/// once the last page is in. It takes them only from its mesh controller or a
/// management application, which this client announces itself as. Nothing
/// acknowledges the pages: `mxr_v2ip_sink_sdp()` reads what the output
/// reports next. `MXR_ERR_INVALID_ARGUMENT` for a null or empty description or
/// one longer than `MXR_V2IP_SDP_MAX` bytes, `MXR_ERR_UNSUPPORTED` for a device
/// that is not a OneIP output and `MXR_ERR_PROTOCOL_TOO_OLD` for one below
/// protocol 0x2E, sending nothing in each case.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `sdp` is
/// null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mxr_play_v2ip_sdp(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    sdp: *const c_char,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string or null.
        let text = match unsafe { req_str(sdp) } {
            Ok(text) => text,
            Err(code) => return code,
        };
        from_control(r.remote.play_v2ip_sdp(device.into(), text))
    })
}

/// Stops the stream a OneIP output plays from a session description. As
/// `mxr_play_v2ip_sdp()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_stop_v2ip_sdp(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.stop_v2ip_sdp(device.into()))
    })
}

/// Sets what a OneIP output shows while it plays audio alone. As
/// `mxr_play_v2ip_sdp()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_sdp_screen(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    screen: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_sdp_screen(device.into(), screen))
    })
}

/// Asks a OneIP output what it plays from a session description now. As
/// `mxr_play_v2ip_sdp()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_v2ip_sink_sdp(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_v2ip_sink_sdp(device.into()))
    })
}

/// Asks a OneIP source unit how its streams are encrypted now, rather than at
/// its next report; `mxr_v2ip_privacy()` reads the answer.
/// `MXR_ERR_UNSUPPORTED` for a device that is not a OneIP source, and
/// `MXR_ERR_PROTOCOL_TOO_OLD` for one below protocol 0x30.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_v2ip_privacy(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_v2ip_privacy(device.into()))
    })
}

/// Asks every OneIP source unit of the mesh how its streams are encrypted, as
/// `mxr_request_v2ip_privacy()` asks one. A unit below protocol 0x30 ignores
/// it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_all_v2ip_privacy(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_all_v2ip_privacy())
    })
}

/// Asks a unit for the message on its output; `mxr_osd_message()` reads the
/// answer. `MXR_ERR_UNSUPPORTED` for a device that is not a OneIP output.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_osd_message(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_osd_message(device.into()))
    })
}

/// Shows a message on a unit's output, in place of any it has, until the unit
/// restarts.
///
/// `logo` is shown with it: 0 the built-in logo, -1 none, 1 to 5 a logo slot.
/// `priority` is `MXR_OSD_MESSAGE_PRIORITY_*`. `text` is markup of at most
/// `MXR_OSD_MESSAGE_TEXT_MAX` bytes with no control character; empty, the
/// logo shows alone. The unit answers with the message it then has, which
/// `mxr_osd_message()` reads, `MXR_OSD_MESSAGE_REFUSED` set when its video
/// processor has no message layer.
///
/// `MXR_ERR_INVALID_ARGUMENT`, sending nothing, for a null `text` and for a
/// logo, priority or text the unit would refuse, and `MXR_ERR_UNSUPPORTED`
/// for a device that is not a OneIP output.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `text` is
/// null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mxr_show_osd_message(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    logo: i8,
    priority: u8,
    text: *const c_char,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a NUL-terminated string or null.
        let text = match unsafe { req_str(text) } {
            Ok(text) => text,
            Err(code) => return code,
        };
        from_control(r.remote.show_osd_message(
            device.into(),
            logo,
            OsdMessagePriority::from_wire(priority),
            text,
        ))
    })
}

/// Takes the message down from a unit's output. As `mxr_show_osd_message()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_clear_osd_message(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.clear_osd_message(device.into()))
    })
}

/// Clears a unit's current system status. As `mxr_upgrade_unit_fpga()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_clear_unit_status(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.clear_unit_status(device.into()))
    })
}

/// Upgrades a unit's video processor to the latest image it holds, or
/// flashes that image again when `force` is set and it already runs it.
///
/// The unit takes a command only from its mesh controller or a management
/// application, which this client announces itself as. Nothing acknowledges
/// it: `mxr_unit_status()` reads what the unit reports next.
/// `MXR_ERR_UNSUPPORTED` for a device that is not a OneIP unit, and
/// `MXR_ERR_PROTOCOL_TOO_OLD` for one below protocol 0x2C.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_upgrade_unit_fpga(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    force: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.upgrade_unit_fpga(device.into(), force))
    })
}

/// Switches a unit to an IP configuration from DHCP. As
/// `mxr_set_unit_network()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_unit_dhcp(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_unit_dhcp(device.into()))
    })
}

/// Gives a unit a static IP configuration, each address a dotted quad.
///
/// `ip` and `netmask` are required; a null or empty gateway or DNS server is
/// sent as 0.0.0.0. `MXR_ERR_INVALID_ARGUMENT`, sending nothing, for text
/// that is not an address, an address a unit cannot hold, or a netmask that
/// is not a run of ones from the top.
///
/// The unit applies the change at once and reverts it unless its mesh
/// controller confirms it in time; `mxr_unit_status()` reports
/// `MXR_UNIT_NET_PENDING` meanwhile, and `mxr_confirm_unit_network()`
/// confirms it. Otherwise as `mxr_upgrade_unit_fpga()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and each address
/// is null or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_unit_network(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    ip: *const c_char,
    netmask: *const c_char,
    gateway: *const c_char,
    dns1: *const c_char,
    dns2: *const c_char,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        let read = |text: *const c_char, what: &str| -> Result<Ipv4Addr, mxr_result_t> {
            // SAFETY: the caller guarantees a NUL-terminated string or null.
            let text = unsafe { opt_str(text) }?.unwrap_or_default();
            if text.is_empty() {
                return Ok(Ipv4Addr::UNSPECIFIED);
            }
            text.parse().map_err(|_| {
                fail(
                    mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                    &format!("{what} is not an IPv4 address: {text:?}"),
                )
            })
        };
        let network = (|| {
            Ok(UnitNetConfig {
                ip: read(ip, "ip")?,
                netmask: read(netmask, "netmask")?,
                gateway: read(gateway, "gateway")?,
                dns1: read(dns1, "dns1")?,
                dns2: read(dns2, "dns2")?,
            })
        })();
        match network {
            Ok(network) => from_control(r.remote.set_unit_network(device.into(), network)),
            Err(code) => code,
        }
    })
}

/// Keeps a unit's pending IP configuration, as its mesh controller does when
/// it hears the unit at its new address. As `mxr_upgrade_unit_fpga()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_confirm_unit_network(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.confirm_unit_network(device.into()))
    })
}

/// Changes a OneIP device's VLAN configuration.
///
/// Sends `vlan`'s ids, its `uplink` and its `MXR_V2IP_VLAN_TRUNK` bit; its
/// other flags, `active_uplink` and `revert_s` are the device's to report and
/// are not sent. Returns `MXR_ERR_INVALID_ARGUMENT` for an id above
/// `MXR_V2IP_VLAN_ID_MAX` or an uplink above `MXR_V2IP_VLAN_PORTS`,
/// `MXR_ERR_UNSUPPORTED` for a device without `MXR_FEATURE_VLAN` or an SFP
/// uplink on a device without `MXR_V2IP_VLAN_HAS_SFP`, and
/// `MXR_ERR_NOT_REPORTED` before the device has reported its configuration,
/// sending nothing in each case.
///
/// The device reverts the change unless the mesh controller confirms it.
/// Nothing is cached: `mxr_v2ip_vlan()` reads what the device reports.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `vlan` is
/// null or points at an initialised `mxr_v2ip_vlan_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_vlan(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    vlan: *const mxr_v2ip_vlan_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(vlan) = (unsafe { vlan.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the VLAN pointer is null",
            );
        };
        from_control(r.remote.set_v2ip_vlan(
            device.into(),
            V2ipVlan {
                flags: V2ipVlanFlag::from_bits(vlan.flags),
                device: vlan.device,
                port: vlan.port,
                uplink: vlan.uplink,
                active_uplink: vlan.active_uplink,
                revert_s: vlan.revert_s,
            },
        ))
    })
}

/// Asks a OneIP sink for its test card, which it reports straight back;
/// `mxr_v2ip_testcard()` reads it.
///
/// `MXR_ERR_NOT_REPORTED` before the sink has reported its video processor
/// features, `MXR_ERR_UNSUPPORTED` when they lack bit 8 of
/// `mxr_v2ip_features()` (the test pattern), and `MXR_ERR_PROTOCOL_TOO_OLD`
/// for a sink below protocol 0x2B, sending nothing in each case. A sink with
/// the feature but without the module that draws the test card does not
/// answer.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_request_v2ip_testcard(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.request_v2ip_testcard(device.into()))
    })
}

/// Shows `MXR_V2IP_TEST_PATTERN_*` on a OneIP sink's output; `colour` is
/// `0xRRGGBB`, used by a flat pattern. The pattern runs until it is turned
/// off, and holds the output on while it does.
///
/// `MXR_ERR_INVALID_ARGUMENT` for a pattern above
/// `MXR_V2IP_TEST_PATTERN_CARD` or a colour above `0xFFFFFF`. Otherwise as
/// `mxr_request_v2ip_testcard()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_test_pattern(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    pattern: u8,
    colour: u32,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_test_pattern(
            device.into(),
            V2ipTestPattern::from_wire(pattern),
            colour,
        ))
    })
}

/// Plays a test tone on a OneIP sink's output.
///
/// `MXR_ERR_INVALID_ARGUMENT` for a value out of the ranges
/// `mxr_v2ip_test_tone_t` gives, unless its mode is `MXR_V2IP_TONE_MODE_OFF`,
/// which stops the tone whatever the rest holds. Otherwise as
/// `mxr_request_v2ip_testcard()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `tone` is
/// null or points at an initialised `mxr_v2ip_test_tone_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_test_tone(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    tone: *const mxr_v2ip_test_tone_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(tone) = (unsafe { tone.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the tone pointer is null",
            );
        };
        from_control(r.remote.set_v2ip_test_tone(device.into(), (*tone).into()))
    })
}

/// Sets a OneIP sink's lip-sync flash.
///
/// `MXR_ERR_INVALID_ARGUMENT` for a value out of the ranges
/// `mxr_v2ip_test_sync_t` gives. Otherwise as `mxr_request_v2ip_testcard()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `sync` is
/// null or points at an initialised `mxr_v2ip_test_sync_t`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_test_sync(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    sync: *const mxr_v2ip_test_sync_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        let Some(sync) = (unsafe { sync.as_ref() }) else {
            return fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the lip-sync pointer is null",
            );
        };
        from_control(r.remote.set_v2ip_test_sync(device.into(), (*sync).into()))
    })
}

/// Sets the infrared profile of a OneIP device's output infrared port.
///
/// `MXR_V2IP_IR_PROFILE_NOT_SET` makes the port follow the global one.
/// Otherwise as `mxr_set_v2ip_ir_profile()`.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_sink_ir_profile(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    profile: i8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_v2ip_sink_ir_profile(device.into(), profile))
    })
}

/// Sets the output format a OneIP sink scales to.
///
/// The mode is checked here and `MXR_ERR_INVALID_ARGUMENT` returned without
/// sending anything, because a sink refuses a bad one in silence. Passing is
/// not a guarantee: the sink also weighs the format against the attached
/// display's EDID and against what its own output stage can produce.
///
/// **Turn automatic scaling off first if it is on.** A sink silently refuses a
/// mode the display does not list while it is scaling automatically. Set the
/// mode, then turn automatic scaling back on if it was on.
///
/// **Pass an `svd` and a `refresh` that agree.** A sink stores both halves and,
/// with its match-source setting on as it ships, reports back the SVD matching
/// the refresh it holds: a 60Hz SVD written with a refresh of 50 reads back as
/// that SVD's 50Hz sibling, once, and stays there. A sink with match-source off
/// reports the SVD it was given. Either way a pair that agrees reads back
/// unchanged and the format driven is the same, and the substitution appears on
/// the sink's next report rather than in the next `mxr_v2ip_details()`.
///
/// A mode read from the sink's own web interface is not interchangeable with
/// this pair. That interface reports the SVD's 60Hz sibling and carries the
/// refresh in a field of its own, so writing back what it shows as the mode, on
/// its own, changes the setting rather than restoring it.
///
/// **Read any route you still need before writing.** The sink rebuilds and
/// rebroadcasts its subscription in response, and the addresses in
/// `mxr_v2ip_details()` can read as zero for up to a minute afterwards.
///
/// `MXR_ERR_UNSUPPORTED` for a unit below protocol 0x2F: released firmware
/// drops a scaling write from management.
///
/// # Safety
///
/// `remote` is null or a live handle, and `mode` points at an initialised
/// [`mxr_v2ip_output_mode_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_set_v2ip_output_mode(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    mode: *const mxr_v2ip_output_mode_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        match unsafe { output_mode(mode) } {
            Ok(m) => from_control(r.remote.set_v2ip_output_mode(device.into(), m)),
            Err(code) => code,
        }
    })
}

/// Clears the output format a OneIP sink is configured to scale to.
///
/// The sink stops scaling for that reason and keeps its automatic scaling
/// setting. This is the only way to express "no mode configured", and it is
/// what restoring a sink that had none requires: a sink reports no mode by
/// leaving `MXR_SCALING_FLAG_MODE_VALID` clear, which a write cannot say.
///
/// **Read any route you still need before writing.** The sink rebuilds and
/// rebroadcasts its subscription in response, and the addresses in
/// `mxr_v2ip_details()` can read as zero for up to a minute afterwards.
///
/// `MXR_ERR_UNSUPPORTED` for a unit below protocol 0x2F: released firmware
/// drops a scaling write from management.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_clear_v2ip_output_mode(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.clear_v2ip_output_mode(device.into()))
    })
}

// ---- video wall ----

/// Where a video-wall sink's window sits, and the picture it was measured
/// against.
///
/// `pos_x` must be a multiple of `MXR_VIDEO_WALL_POS_ALIGN`, `width` a
/// multiple of `MXR_VIDEO_WALL_WIDTH_ALIGN`, both sides at least
/// `MXR_VIDEO_WALL_MIN_SIZE`, and the window must fit inside the raster it
/// names. `pos_y` and `height` have no alignment rule. A zero `width` or
/// `height` clears the wall and is checked against none of this.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_video_wall_window_t {
    /// Window origin, horizontal.
    pub pos_x: u16,
    /// Window origin, vertical.
    pub pos_y: u16,
    /// Window width, or zero to clear the wall.
    pub width: u16,
    /// Window height, or zero to clear the wall.
    pub height: u16,
    /// Active picture width the window was measured against.
    pub raster_w: u16,
    /// Active picture height the window was measured against.
    pub raster_h: u16,
}

/// A window's horizontal origin must be a multiple of this.
pub const MXR_VIDEO_WALL_POS_ALIGN: u16 = 64;

/// A window's width must be a multiple of this.
pub const MXR_VIDEO_WALL_WIDTH_ALIGN: u16 = 4;

/// Neither side of a window may be smaller than this.
pub const MXR_VIDEO_WALL_MIN_SIZE: u16 = 64;

impl From<mxr_video_wall_window_t> for VideoWallWindow {
    fn from(w: mxr_video_wall_window_t) -> Self {
        Self {
            pos_x: w.pos_x,
            pos_y: w.pos_y,
            width: w.width,
            height: w.height,
            raster_w: w.raster_w,
            raster_h: w.raster_h,
        }
    }
}

/// Reads a window argument, refusing a null pointer.
unsafe fn wall_window(
    window: *const mxr_video_wall_window_t,
) -> Result<VideoWallWindow, mxr_result_t> {
    // SAFETY: the caller guarantees an initialised struct or null.
    match unsafe { window.as_ref() } {
        Some(w) => Ok((*w).into()),
        None => Err(fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "the window pointer is null",
        )),
    }
}

/// Shows a window on a sink's video wall without storing it.
///
/// The window lasts until the sink is told otherwise or restarts;
/// `mxr_revert_video_wall()` puts back whatever it has stored. A zero width or
/// height shows the whole frame again.
///
/// The geometry is checked here and `MXR_ERR_INVALID_ARGUMENT` returned
/// without sending anything, because the sink is not guaranteed to check it
/// itself.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it: `MXR_ERR_UNSUPPORTED` for a unit that does not report
/// `MXR_FEATURE_VIDEO_WALL`. Nothing answers, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle, and `window` points at an initialised
/// [`mxr_video_wall_window_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_preview_video_wall(
    remote: *const mxr_remote_t,
    sink: mxr_uid_t,
    window: *const mxr_video_wall_window_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        match unsafe { wall_window(window) } {
            Ok(w) => from_control(r.remote.preview_video_wall(sink.into(), w)),
            Err(code) => code,
        }
    })
}

/// Stores a window as a sink's video wall.
///
/// The geometry is checked here and `MXR_ERR_INVALID_ARGUMENT` returned
/// without sending anything. That matters more than a refused frame would: a
/// sink running a video-wall module older than 2026083100 writes the window to
/// its configuration before asking its video processor to apply it, and the
/// processor's refusal does not undo the write, so an out-of-spec window
/// survives a reboot and is re-offered on every stream restart until something
/// else replaces it. A power cycle does not clear it.
///
/// A zero width or height stores "show the whole frame".
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it: `MXR_ERR_UNSUPPORTED` for a unit that does not report
/// `MXR_FEATURE_VIDEO_WALL`. Nothing answers, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle, and `window` points at an initialised
/// [`mxr_video_wall_window_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_store_video_wall(
    remote: *const mxr_remote_t,
    sink: mxr_uid_t,
    window: *const mxr_video_wall_window_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees an initialised struct or null.
        match unsafe { wall_window(window) } {
            Ok(w) => from_control(r.remote.store_video_wall(sink.into(), w)),
            Err(code) => code,
        }
    })
}

/// Restores the window a sink has stored, discarding a preview.
///
/// Carries no window: the sink already holds the one this puts back.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it: `MXR_ERR_UNSUPPORTED` for a unit that does not report
/// `MXR_FEATURE_VIDEO_WALL`. Nothing answers, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_revert_video_wall(
    remote: *const mxr_remote_t,
    sink: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.revert_video_wall(sink.into()))
    })
}

// ---- multiviewer ----

/// Switches a multiviewer's window layout.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_view_mode(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    mode: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_view_mode(device.into(), MultiviewerViewMode::from_wire(mode)),
        )
    })
}

/// Puts a source in one of a multiviewer's windows.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_video_source(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    screen: u8,
    source: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_multiviewer_video_source(
            device.into(),
            screen,
            MultiviewerSource::from_wire(source),
        ))
    })
}

/// Chooses which window a multiviewer takes its audio from.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_audio_source(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    source: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_audio_source(device.into(), MultiviewerSource::from_wire(source)),
        )
    })
}

/// Sets a multiviewer's output volume and mute state.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_audio_volume(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    volume: u8,
    muted: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_audio_volume(device.into(), volume, muted),
        )
    })
}

/// Switches the EDID a multiviewer presents to its sources.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_edid_template(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    template: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_multiviewer_edid_template(
            device.into(),
            MultiviewerEdidTemplate::from_wire(template),
        ))
    })
}

/// Chooses which window a multiviewer forwards remote control to.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_remote_control(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    source: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote.set_multiviewer_remote_control(
                device.into(),
                MultiviewerSource::from_wire(source),
            ),
        )
    })
}

/// Sets the size of a multiviewer's picture-in-picture window.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_pip_size(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    size: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_pip_size(device.into(), MultiviewerPipSize::from_wire(size)),
        )
    })
}

/// Sets which corner a multiviewer's picture-in-picture window sits in.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_pip_position(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    position: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_multiviewer_pip_position(
            device.into(),
            MultiviewerPipPosition::from_wire(position),
        ))
    })
}

/// Sets how a multiviewer fits a source into its window.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_aspect_ratio(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    aspect: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote.set_multiviewer_aspect_ratio(
                device.into(),
                MultiviewerAspectRatio::from_wire(aspect),
            ),
        )
    })
}

/// Turns a multiviewer's automatic source switching on or off.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_auto_switch(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    enable: bool,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.set_multiviewer_auto_switch(device.into(), enable))
    })
}

/// Switches a multiviewer's output resolution.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_output_mode(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    mode: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_output_mode(device.into(), MultiviewerOutputMode::from_wire(mode)),
        )
    })
}

/// Sets a multiviewer's IT content flag.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_output_itc(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    mode: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_output_itc(device.into(), MultiviewerItcMode::from_wire(mode)),
        )
    })
}

/// Switches a multiviewer's HDCP mode.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_hdcp_mode(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    mode: u8,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_hdcp_mode(device.into(), MultiviewerHdcpMode::from_wire(mode)),
        )
    })
}

/// Maps one of a multiviewer's inputs to a source device.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_set_multiviewer_input_source(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    input: u8,
    source: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(
            r.remote
                .set_multiviewer_input_source(device.into(), input, source.into()),
        )
    })
}

/// Asks a multiviewer to map its inputs to the sources it can see.
///
/// A loadable module serves this, not the device firmware, and a model may
/// not have it. Nothing answers either way, so `MXR_OK` means the frame was
/// sent and not that anything acted on it.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_multiviewer_auto_route(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        from_control(r.remote.multiviewer_auto_route(device.into()))
    })
}

// ---- overlay sync ----

/// One setting of an overlay sync this client sends.
///
/// `kind` picks which fields are read: a caption reads `font` to `text`, a
/// logo reads `logo`, `x` and `y`, and a background reads `colour`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_osd_sync_entry_t {
    /// `MXR_OSD_SYNC_TYPE_*`.
    pub kind: u8,
    /// `MXR_OSD_SYNC_SIZE_*`: the output size it applies at.
    pub size: u8,
    /// The layers it applies to, a bit per layer.
    pub layers: u16,
    /// The fields it gives: `MXR_OSD_CAPTION_FIELD_*` for a caption,
    /// `MXR_OSD_LOGO_FIELD_*` for a logo, and bit 0 for a background's
    /// colour. A field given its default value goes back to the layout's.
    pub set: u16,
    /// A caption's font, null or empty for the layout's.
    pub font: *const c_char,
    /// A caption's size, zero for the layout's.
    pub font_size: u16,
    /// The horizontal position, `INT16_MIN` for the layout's.
    pub x: i16,
    /// The vertical position, `INT16_MIN` for the layout's.
    pub y: i16,
    /// A caption's wrap width in pixels: -1 automatic, 0 none, -2 the
    /// layout's.
    pub wrap: i16,
    /// `MXR_OSD_CAPTION_ALIGN_*`.
    pub align: u8,
    /// `MXR_OSD_CAPTION_FORMAT_*`.
    pub format: u8,
    /// A caption's or background's colour as 0xRRGGBB, `UINT32_MAX` for the
    /// layout's.
    pub colour: u32,
    /// A caption's text, null for none.
    pub text: *const c_char,
    /// The logo shown: 0 the built-in one, -1 none, 1 to 4 a slot.
    pub logo: i8,
}

/// The logo image for one slot.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_osd_sync_image_t {
    /// The PNG, null when `len` is zero.
    pub png: *const u8,
    /// Its size in bytes. Zero empties the slot.
    pub len: usize,
    /// The size it is shown at: 0 at its own pixels, 1 to 100 its longer
    /// side as a percent of the large overlay.
    pub percent: u8,
}

/// Reads one setting from the caller.
///
/// # Safety
///
/// `font` and `text` are null or NUL-terminated strings.
unsafe fn osd_entry(e: &mxr_osd_sync_entry_t) -> Result<OsdSyncEntry, mxr_result_t> {
    let value = match e.kind {
        MXR_OSD_SYNC_TYPE_CAPTION => {
            // SAFETY: the caller guarantees NUL-terminated strings or null.
            let (font, text) = unsafe { (opt_str(e.font)?, opt_str(e.text)?) };
            OsdSyncValue::Caption(OsdCaption {
                font: font.unwrap_or_default().to_owned(),
                size: e.font_size,
                x: e.x,
                y: e.y,
                wrap: e.wrap,
                align: OsdCaptionAlign::from_wire(e.align),
                format: OsdCaptionFormat::from_wire(e.format),
                colour: e.colour,
                text: text.unwrap_or_default().to_owned(),
            })
        }
        MXR_OSD_SYNC_TYPE_LOGO => OsdSyncValue::Logo(OsdLogo {
            logo: e.logo,
            x: e.x,
            y: e.y,
        }),
        MXR_OSD_SYNC_TYPE_BACKGROUND => OsdSyncValue::Background(e.colour),
        _ => {
            return Err(fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "an overlay setting of a MXR_OSD_SYNC_TYPE_* kind",
            ))
        }
    };
    Ok(OsdSyncEntry {
        size: OsdSyncSize::from_wire(e.size),
        layers: e.layers,
        set: e.set,
        value,
    })
}

/// Reads what the caller has units copy.
///
/// # Safety
///
/// `entries` is null or points at `count` entries, `images` is null or
/// points at `MXR_OSD_SYNC_SLOTS` images, and every pointer in them is as
/// their fields say.
unsafe fn osd_content(
    what: u8,
    entries: *const mxr_osd_sync_entry_t,
    count: usize,
    images: *const mxr_osd_sync_image_t,
) -> Result<OsdSyncContent, mxr_result_t> {
    let entries: &[mxr_osd_sync_entry_t] = match (entries.is_null(), count) {
        (_, 0) => &[],
        (true, _) => {
            return Err(fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "entries is null with a count above zero",
            ))
        }
        // SAFETY: the caller guarantees count entries at entries.
        (false, _) => unsafe { std::slice::from_raw_parts(entries, count) },
    };
    let entries = entries
        .iter()
        // SAFETY: the caller guarantees the strings in each entry.
        .map(|e| unsafe { osd_entry(e) })
        .collect::<Result<Vec<_>, _>>()?;
    let images = if images.is_null() {
        None
    } else {
        // SAFETY: the caller guarantees MXR_OSD_SYNC_SLOTS images at images.
        let images = unsafe { std::slice::from_raw_parts(images, MXR_OSD_SYNC_SLOTS) };
        let mut out: [OsdSyncImage; MXR_OSD_SYNC_SLOTS] = Default::default();
        for (o, i) in out.iter_mut().zip(images) {
            o.percent = i.percent;
            if i.len == 0 {
                continue;
            }
            if i.png.is_null() {
                return Err(fail(
                    mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                    "an image's png is null with a length above zero",
                ));
            }
            // SAFETY: the caller guarantees len bytes at png.
            o.png = unsafe { std::slice::from_raw_parts(i.png, i.len) }.to_vec();
        }
        Some(out)
    };
    Ok(OsdSyncContent {
        what: OsdSyncWhat::from_bits(what),
        entries,
        images,
    })
}

/// Reports a sync's number to a caller that asked for it.
///
/// # Safety
///
/// `out` is null or writable.
unsafe fn sent_seq(result: Result<u16, ControlError>, out: *mut u16) -> mxr_result_t {
    match result {
        Ok(seq) => {
            if !out.is_null() {
                // SAFETY: the caller guarantees a writable u16 or null.
                unsafe { out.write(seq) };
            }
            mxr_result_t::MXR_OK
        }
        Err(e) => from_control(Err(e)),
    }
}

/// Has every unit copy the given overlay settings and logo images, and keeps
/// them in place, writing the sync's number to `seq` when it is not null.
///
/// `what` is `MXR_OSD_SYNC_WHAT_*` bits; `MXR_OSD_SYNC_WHAT_LOGO_SLOTS`
/// follows from `images`, which is null to leave every unit's own or
/// `MXR_OSD_SYNC_SLOTS` images. The library keeps its own copies.
///
/// Units take it from the mesh controller or a management application,
/// which this client announces itself as. Each unit fetches the images from
/// an HTTP server this client runs on `mxr_config_t::logo_port`, then asks
/// for the settings, which this client answers until the sync is replaced or
/// `mxr_clear_osd_sync()`. Meanwhile it announces itself as the mesh's
/// overlay sync manager, so a unit waking from power save takes this sync
/// too, rather than the controller's. The controller can still send its own
/// over it. `mxr_osd_sync_report()` on each unit says how it went.
///
/// `MXR_ERR_INVALID_ARGUMENT` for a sync copying nothing, more than 128
/// settings, a caption font over 31 or text over 360 bytes, a logo other than
/// -1 to 4, or an image that is not a PNG of at most 512 KB and 1920 by 1080
/// pixels, shown at over 100 percent; the sync held before stays. A sync
/// that fails to go out leaves none held.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`. `entries` is
/// null or points at `count` entries whose strings are null or
/// NUL-terminated. `images` is null or points at `MXR_OSD_SYNC_SLOTS` images,
/// each `png` null or pointing at `len` bytes. `seq` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn mxr_send_osd_sync(
    remote: *const mxr_remote_t,
    what: u8,
    entries: *const mxr_osd_sync_entry_t,
    count: usize,
    images: *const mxr_osd_sync_image_t,
    seq: *mut u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees the arrays and the strings in them.
        let content = match unsafe { osd_content(what, entries, count, images) } {
            Ok(content) => content,
            Err(code) => return code,
        };
        // SAFETY: the caller guarantees a writable u16 or null.
        unsafe { sent_seq(r.remote.send_osd_sync(content), seq) }
    })
}

/// Has one unit copy the overlay sync this client holds again, writing the
/// new sync's number to `seq` when it is not null. As `mxr_send_osd_sync()`.
///
/// `MXR_ERR_INVALID_ARGUMENT` while no sync is held.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`, and `seq` is
/// null or writable.
#[no_mangle]
pub unsafe extern "C" fn mxr_resend_osd_sync(
    remote: *const mxr_remote_t,
    device: mxr_uid_t,
    seq: *mut u16,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        // SAFETY: the caller guarantees a writable u16 or null.
        unsafe { sent_seq(r.remote.resend_osd_sync(device.into()), seq) }
    })
}

/// Stops holding the overlay sync this client sent, and hands the overlay
/// sync manager's part back to the controller. Units keep what they took.
///
/// # Safety
///
/// `remote` is null or a live handle from `mxr_remote_new()`.
#[no_mangle]
pub unsafe extern "C" fn mxr_clear_osd_sync(remote: *const mxr_remote_t) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        r.remote.clear_osd_sync();
        mxr_result_t::MXR_OK
    })
}

#[cfg(test)]
mod tests {
    //! The boundary tests open no socket, so a sync they send never goes
    //! out and what it carried cannot be seen there. What the caller's
    //! structs become is read here instead.

    use std::ptr;

    use super::*;

    fn entry(kind: u8) -> mxr_osd_sync_entry_t {
        mxr_osd_sync_entry_t {
            kind,
            size: 2,
            layers: 0x2010,
            set: 0x0155,
            font: c"Sans".as_ptr(),
            font_size: 0x0124,
            x: -0x0203,
            y: 0x0405,
            wrap: -2,
            align: 2,
            format: 1,
            colour: 0x00A1_B2C3,
            text: c"No signal".as_ptr(),
            logo: 3,
        }
    }

    #[test]
    fn each_kind_reads_its_own_fields() {
        let entries = [
            entry(MXR_OSD_SYNC_TYPE_CAPTION),
            entry(MXR_OSD_SYNC_TYPE_LOGO),
            entry(MXR_OSD_SYNC_TYPE_BACKGROUND),
        ];
        // SAFETY: three entries whose strings are literals; no images.
        let c = unsafe { osd_content(0x05, entries.as_ptr(), 3, ptr::null()) }.expect("read");
        assert_eq!(c.what, OsdSyncWhat::from_bits(0x05));
        assert_eq!(c.images, None);
        for e in &c.entries {
            assert_eq!(e.size, OsdSyncSize::SMALL);
            assert_eq!(e.layers, 0x2010);
            assert_eq!(e.set, 0x0155);
        }
        assert_eq!(
            c.entries[0].value,
            OsdSyncValue::Caption(OsdCaption {
                font: "Sans".to_owned(),
                size: 0x0124,
                x: -0x0203,
                y: 0x0405,
                wrap: -2,
                align: OsdCaptionAlign::RIGHT,
                format: OsdCaptionFormat::MARKDOWN,
                colour: 0x00A1_B2C3,
                text: "No signal".to_owned(),
            })
        );
        assert_eq!(
            c.entries[1].value,
            OsdSyncValue::Logo(OsdLogo {
                logo: 3,
                x: -0x0203,
                y: 0x0405,
            })
        );
        assert_eq!(c.entries[2].value, OsdSyncValue::Background(0x00A1_B2C3));
    }

    #[test]
    fn null_strings_read_as_empty() {
        let mut e = entry(MXR_OSD_SYNC_TYPE_CAPTION);
        e.font = ptr::null();
        e.text = ptr::null();
        // SAFETY: one entry with null strings; no images.
        let c = unsafe { osd_content(1, &e, 1, ptr::null()) }.expect("read");
        let OsdSyncValue::Caption(caption) = &c.entries[0].value else {
            panic!("not a caption");
        };
        assert_eq!((caption.font.as_str(), caption.text.as_str()), ("", ""));
    }

    #[test]
    fn images_are_copied_slot_by_slot() {
        let png = [0x89u8, b'P', b'N', b'G'];
        let images: [mxr_osd_sync_image_t; MXR_OSD_SYNC_SLOTS] = [
            mxr_osd_sync_image_t {
                png: png.as_ptr(),
                len: 4,
                percent: 25,
            },
            mxr_osd_sync_image_t {
                png: ptr::null(),
                len: 0,
                percent: 0,
            },
            mxr_osd_sync_image_t {
                png: png.as_ptr(),
                len: 2,
                percent: 100,
            },
            mxr_osd_sync_image_t {
                png: png.as_ptr(),
                len: 0,
                percent: 7,
            },
        ];
        // SAFETY: four images, each png pointing at len bytes or more.
        let c = unsafe { osd_content(1, ptr::null(), 0, images.as_ptr()) }.expect("read");
        let got = c.images.expect("images");
        assert_eq!(got[0].png, png);
        assert_eq!(got[0].percent, 25);
        assert!(got[1].png.is_empty());
        assert_eq!(got[2].png, &png[..2]);
        assert_eq!(got[2].percent, 100);
        assert!(got[3].png.is_empty());
        assert_eq!(got[3].percent, 7);
    }

    #[test]
    fn a_sent_sync_reports_its_number_and_a_failed_one_does_not() {
        let mut seq = 0u16;
        // SAFETY: a writable u16.
        assert_eq!(
            unsafe { sent_seq(Ok(0xA1B2), &mut seq) },
            mxr_result_t::MXR_OK
        );
        assert_eq!(seq, 0xA1B2);
        // SAFETY: a null output is allowed.
        assert_eq!(
            unsafe { sent_seq(Ok(1), ptr::null_mut()) },
            mxr_result_t::MXR_OK
        );
        let refused = Err(ControlError::InvalidRequest("no overlay sync is held"));
        // SAFETY: a writable u16.
        let rc = unsafe { sent_seq(refused, &mut seq) };
        assert_eq!(rc, mxr_result_t::MXR_ERR_INVALID_ARGUMENT);
        assert_eq!(seq, 0xA1B2);
    }

    #[test]
    fn what_the_caller_cannot_mean_is_refused() {
        let bad = entry(3);
        // SAFETY: one entry; no images.
        let r = unsafe { osd_content(1, &bad, 1, ptr::null()) };
        assert!(
            matches!(r, Err(mxr_result_t::MXR_ERR_INVALID_ARGUMENT)),
            "kind"
        );
        // SAFETY: null entries with a count is what is under test.
        let r = unsafe { osd_content(1, ptr::null(), 1, ptr::null()) };
        assert!(
            matches!(r, Err(mxr_result_t::MXR_ERR_INVALID_ARGUMENT)),
            "entries"
        );
        let images = [mxr_osd_sync_image_t {
            png: ptr::null(),
            len: 1,
            percent: 0,
        }; MXR_OSD_SYNC_SLOTS];
        // SAFETY: four images; a null png with a length is what is under test.
        let r = unsafe { osd_content(1, ptr::null(), 0, images.as_ptr()) };
        assert!(
            matches!(r, Err(mxr_result_t::MXR_ERR_INVALID_ARGUMENT)),
            "png"
        );
    }
}
