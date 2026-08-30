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

use mx_remote::{
    EdidProfile, MultiviewerAspectRatio, MultiviewerEdidTemplate, MultiviewerHdcpMode,
    MultiviewerItcMode, MultiviewerOutputMode, MultiviewerPipPosition, MultiviewerPipSize,
    MultiviewerSource, MultiviewerViewMode, RcAction, V2ipAudioFormat,
};

use crate::abi::{
    fail, from_control, mxr_bay_uid_t, mxr_result_t, mxr_tribool_t, mxr_uid_t, req_str,
};
use crate::info::mxr_amp_zone_settings_t;
use crate::remote::{mxr_remote_t, with};

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

/// Routes a V2IP sink's video to the stream a source port advertises.
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

/// Routes a V2IP sink's audio to the stream a source port advertises,
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

/// Routes a V2IP sink's video to the source bay with this user-assigned name.
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

/// Routes a V2IP sink's audio to the source bay with this user-assigned name.
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

/// Routes a V2IP sink's audio to a multicast group directly, for a source this
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

/// Activates or clears an audio endpoint's trigger.
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

/// Sets an audio endpoint's volume.
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

/// Subscribes to, or unsubscribes from, a device's V2IP statistics.
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

/// Sends the monitoring pulse that tells devices this client is watching.
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

// ---- multiviewer ----

/// Switches a multiviewer's window layout.
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
