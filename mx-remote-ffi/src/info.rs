// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Reading state: the snapshot structs, and the calls that fill them.
//!
//! State lives behind a lock that the receive thread also takes, so nothing
//! here hands back a pointer into it. A caller passes an identifier and a
//! struct it owns, and gets a copy that stays valid however the network
//! behaves afterwards.
//!
//! Two conventions run through the structs. A flag the device has not reported
//! is a [`mxr_tribool_t`] rather than a `bool`, because firmware sends only
//! what it has and "off" is a different answer from "never said". A route that
//! names no bay carries the zero device identifier, which is the spelling the
//! protocol itself uses for absence.

use std::ffi::c_char;

use mx_remote::{
    AmpZoneSettings, ArcStatus, BayInfo, BaySignalDetails, DeviceInfo, DeviceStatus, PowerStatus,
    AMP_EQ_BANDS,
};

use crate::abi::{
    bay_or_zero, fail, guard, mxr_bay_uid_t, mxr_result_t, mxr_tribool_t, mxr_uid_t, put_str,
};
use crate::remote::{mxr_remote_t, with, MXR_IP_STRING_LEN};

/// Bytes a device, bay or port name needs, the terminator included.
///
/// The wire field is 16 bytes wide. The rest is headroom for the names this
/// library derives rather than reads, which that width does not bound.
pub const MXR_NAME_LEN: usize = 32;

/// Bytes a serial number needs, the terminator included.
pub const MXR_SERIAL_LEN: usize = 32;

/// Bytes a model name needs, the terminator included.
pub const MXR_MODEL_LEN: usize = 48;

/// Bytes a firmware version string needs, the terminator included.
///
/// The wire field is 128 bytes and is not NUL-terminated when full.
pub const MXR_VERSION_LEN: usize = 129;

/// Bytes a signal description such as `1080p60 444 8` needs, the terminator
/// included.
pub const MXR_SIGNAL_TYPE_LEN: usize = 48;

/// Bytes a device's system-status message needs, the terminator included.
pub const MXR_MESSAGE_LEN: usize = 128;

/// Number of EQ bands an amplifier zone carries.
///
/// Written as a literal because the generated header needs one, and checked
/// against the core crate's value below so the two cannot drift apart.
pub const MXR_AMP_EQ_BANDS: usize = 5;

const _: () = assert!(MXR_AMP_EQ_BANDS == AMP_EQ_BANDS);

/// The high-level state of a device on the network.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum mxr_device_status_t {
    /// Reachable and reporting.
    MXR_DEVICE_ONLINE = 0,
    /// Has stopped answering.
    MXR_DEVICE_OFFLINE = 1,
    /// Announced a reboot.
    MXR_DEVICE_REBOOTING = 2,
    /// Still coming up.
    MXR_DEVICE_BOOTING = 3,
    /// Present but not participating.
    MXR_DEVICE_INACTIVE = 4,
}

impl From<DeviceStatus> for mxr_device_status_t {
    fn from(status: DeviceStatus) -> Self {
        match status {
            DeviceStatus::Online => Self::MXR_DEVICE_ONLINE,
            DeviceStatus::Offline => Self::MXR_DEVICE_OFFLINE,
            DeviceStatus::Rebooting => Self::MXR_DEVICE_REBOOTING,
            DeviceStatus::Booting => Self::MXR_DEVICE_BOOTING,
            DeviceStatus::Inactive => Self::MXR_DEVICE_INACTIVE,
        }
    }
}

/// The CEC power state of whatever is connected to a bay.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum mxr_power_status_t {
    /// The bay has not reported a power state.
    MXR_POWER_UNKNOWN = 0,
    /// Powered on.
    MXR_POWER_ON = 1,
    /// Powered off.
    MXR_POWER_OFF = 2,
}

impl From<Option<PowerStatus>> for mxr_power_status_t {
    fn from(status: Option<PowerStatus>) -> Self {
        match status {
            Some(PowerStatus::On) => Self::MXR_POWER_ON,
            Some(PowerStatus::Off) => Self::MXR_POWER_OFF,
            // A bay that reported PowerStatus::Unknown and one that reported
            // nothing at all are the same answer to the only question a caller
            // can ask of this field.
            Some(PowerStatus::Unknown) | None => Self::MXR_POWER_UNKNOWN,
        }
    }
}

/// The audio return channel a bay is carrying.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum mxr_arc_status_t {
    /// No audio is being returned.
    MXR_ARC_INACTIVE = 0,
    /// Returned over HDMI.
    MXR_ARC_HDMI = 1,
    /// Returned over optical.
    MXR_ARC_OPTICAL = 2,
    /// Returned over analogue.
    MXR_ARC_ANALOG = 3,
}

impl From<ArcStatus> for mxr_arc_status_t {
    fn from(arc: ArcStatus) -> Self {
        match arc {
            ArcStatus::Inactive => Self::MXR_ARC_INACTIVE,
            ArcStatus::Hdmi => Self::MXR_ARC_HDMI,
            ArcStatus::Optical => Self::MXR_ARC_OPTICAL,
            ArcStatus::Analog => Self::MXR_ARC_ANALOG,
        }
    }
}

/// What a device is, and what it is doing.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_device_info_t {
    /// The device's identifier.
    pub uid: mxr_uid_t,
    /// The name the device advertises.
    pub name: [c_char; MXR_NAME_LEN],
    /// Serial number.
    pub serial: [c_char; MXR_SERIAL_LEN],
    /// A friendly model name, derived from the advertised name and the bays
    /// the device reports.
    pub model: [c_char; MXR_MODEL_LEN],
    /// Firmware version string from the hello frame.
    pub version: [c_char; MXR_VERSION_LEN],
    /// The highest protocol version the device can decode, or zero before it
    /// has said. A command above it is refused rather than sent.
    pub supported_protocol: u16,
    /// What the device says it can do, as `MXR_FEATURE_*` bits.
    pub features: u32,
    /// The address the device was last heard from, empty when never.
    pub address: [c_char; MXR_IP_STRING_LEN],
    /// Online, offline, booting or rebooting.
    pub status: mxr_device_status_t,
    /// Whether the device has been heard from recently enough to count as
    /// present.
    pub online: bool,
    /// Whether every part of the device's configuration has arrived.
    pub configuration_complete: bool,
    /// Whether the device's firmware initialises the configuration it
    /// broadcasts.
    ///
    /// Firmware without it builds some frames over uninitialised stack, so
    /// those fields carry noise rather than values: the scaling flags and,
    /// behind a spuriously set valid bit, the scaling mode and refresh; bay
    /// zero's addresses in the V2IP sources frame; and the padding beside the
    /// remote-control target.
    pub config_initialised: bool,
    /// The mesh master this device follows, zero when it is in no mesh.
    pub mesh_master: mxr_uid_t,
    /// How many HDBaseT outputs this model has.
    pub hdbt_outputs: u8,
    /// Whether installation was marked complete.
    pub setup_done: mxr_tribool_t,
    /// The installer identifier, or -1 when the device has not reported one.
    pub installer_id: i32,
    /// Whether the device has reported a status about itself.
    pub has_system_status: bool,
    /// The status code, meaningful only when `has_system_status` is set.
    pub system_status: u16,
    /// The status message, empty when there is none.
    pub system_message: [c_char; MXR_MESSAGE_LEN],
    /// How many temperatures `mxr_device_temperatures()` would return.
    pub temperature_count: usize,
    /// How many bays `mxr_device_bays()` would return.
    pub bay_count: usize,
}

/// What a bay is, and what is connected to it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_bay_info_t {
    /// How the bay is addressed.
    pub uid: mxr_bay_uid_t,
    /// The name the device gives the port, such as `Output 1`.
    pub port_name: [c_char; MXR_NAME_LEN],
    /// The name the installer gave the bay, falling back to the port name.
    pub user_name: [c_char; MXR_NAME_LEN],
    /// The bay number the device's own API and topology use, which is not the
    /// port number this library addresses it by.
    pub bay_num: u8,
    /// What the bay is wired for, as `MXR_BAY_*` bits.
    pub features: u32,
    /// Whether the bay takes a signal in.
    pub is_input: bool,
    /// Whether the bay puts a signal out.
    pub is_output: bool,
    /// Whether the bay carries audio and no video.
    pub is_audio: bool,
    /// Whether the bay can decode Dolby.
    pub has_dolby: bool,
    /// Whether the bay is on this device rather than reached through the mesh.
    pub is_local: bool,
    /// The bay routed to this one for video, zero when unrouted.
    pub video_source: mxr_bay_uid_t,
    /// The bay routed to this one for audio, which follows the video source
    /// until the bay is told otherwise. Zero when unrouted.
    pub audio_source: mxr_bay_uid_t,
    /// Power state of what is connected.
    pub power_status: mxr_power_status_t,
    /// Whether the bay is hidden from the installation's user interface.
    pub hidden: mxr_tribool_t,
    /// Whether the device reports the bay as faulty.
    pub faulty: mxr_tribool_t,
    /// Whether the bay is delivering power over the link.
    pub poe_powered: mxr_tribool_t,
    /// Whether an HDBaseT link is up.
    pub hdbt_connected: mxr_tribool_t,
    /// Whether a signal is present.
    pub signal_detected: mxr_tribool_t,
    /// Whether hot-plug detect is asserted.
    pub hpd_detected: mxr_tribool_t,
    /// Whether a CEC device answered.
    pub cec_detected: mxr_tribool_t,
    /// Whether the bay's encoder is switched off.
    pub encoder_disabled: mxr_tribool_t,
    /// Whether the bay's decoder is switched off.
    pub decoder_disabled: mxr_tribool_t,
    /// The signal as the device describes it, empty when it has not.
    pub signal_type: [c_char; MXR_SIGNAL_TYPE_LEN],
    /// The signal format the device reports, as an `mxr_signal_type_t` value.
    pub signal_mode: u16,
    /// Whether audio return is active, and over which connector.
    pub arc: mxr_arc_status_t,
    /// Whether the bay has reported a volume.
    pub has_volume: bool,
    /// The combined left/right volume percentage.
    pub volume: u8,
    /// Whether either channel is muted.
    pub muted: mxr_tribool_t,
    /// Whether the bay has reported a remote-control type.
    pub has_rc_type: bool,
    /// The kind of remote control attached, as the wire value.
    pub rc_type: u8,
    /// Whether the bay has reported an EDID profile.
    pub has_edid_profile: bool,
    /// The EDID profile the bay presents.
    pub edid_profile: u16,
    /// The bay this one mirrors, zero when it mirrors nothing.
    pub mirror: mxr_bay_uid_t,
    /// The audio endpoint this bay feeds, or -1 on a device without them.
    pub audio_endpoint: i16,
    /// The bay on another device this one is linked to, zero when it is
    /// linked to none or that bay is not yet known.
    ///
    /// The link is mesh configuration, not a route: it names the bay elsewhere
    /// that belongs to this one, such as the amplifier zone carrying a OneIP
    /// output's volume. `volume` is already read through it, and
    /// `mxr_set_volume()` already writes through it.
    pub linked_bay: mxr_bay_uid_t,
    /// The source device a V2IP bay maps to, zero when it maps to none.
    pub v2ip_uid: mxr_uid_t,
    /// How many devices `mxr_bay_filtered()` would return.
    pub filtered_count: usize,
}

/// The signal a bay measures, beyond the description in
/// [`mxr_bay_info_t::signal_type`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_signal_details_t {
    /// Frame rate in Hz, already corrected for a 1000/1001 clock.
    pub frame_rate: f64,
    /// TMDS clock rate in Hz.
    pub tmds_clock: u32,
    /// Video clock rate in Hz.
    pub clock_rate: u32,
    /// The bay status word from the report's bay block.
    pub status: u32,
    /// The signal type the bay is scaling to.
    pub scaling: u16,
}

/// A ProAmp8 zone's gain, delay, tone and power settings.
///
/// Gains and volume limits run 0-248 in 0.5dB steps, with 200 as 0dB. Tone and
/// EQ values are neutral at 128.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_amp_zone_settings_t {
    /// Left channel gain.
    pub gain_left: u8,
    /// Right channel gain.
    pub gain_right: u8,
    /// Lowest volume the zone may be set to.
    pub volume_min: u8,
    /// Highest volume the zone may be set to.
    pub volume_max: u8,
    /// Bass tone control.
    pub bass: u8,
    /// Treble tone control.
    pub treble: u8,
    /// 0 = normal, 1 = bridged.
    pub bridged: u8,
    /// Power-on mode.
    pub power_mode: u8,
    /// Signal level that switches the zone on automatically.
    pub power_level: u8,
    /// Left channel delay, in 1/48000 second increments.
    pub delay_left: u32,
    /// Right channel delay, in 1/48000 second increments.
    pub delay_right: u32,
    /// Idle time before the zone powers down, in seconds.
    pub power_timeout: u32,
    /// Left channel EQ, from 100Hz to 10KHz.
    pub eq_left: [u8; MXR_AMP_EQ_BANDS],
    /// Right channel EQ, from 100Hz to 10KHz.
    pub eq_right: [u8; MXR_AMP_EQ_BANDS],
}

impl From<AmpZoneSettings> for mxr_amp_zone_settings_t {
    fn from(s: AmpZoneSettings) -> Self {
        Self {
            gain_left: s.gain_left,
            gain_right: s.gain_right,
            volume_min: s.volume_min,
            volume_max: s.volume_max,
            bass: s.bass,
            treble: s.treble,
            bridged: s.bridged,
            power_mode: s.power_mode,
            power_level: s.power_level,
            delay_left: s.delay_left,
            delay_right: s.delay_right,
            power_timeout: s.power_timeout,
            eq_left: s.eq_left,
            eq_right: s.eq_right,
        }
    }
}

impl From<mxr_amp_zone_settings_t> for AmpZoneSettings {
    fn from(s: mxr_amp_zone_settings_t) -> Self {
        Self {
            gain_left: s.gain_left,
            gain_right: s.gain_right,
            volume_min: s.volume_min,
            volume_max: s.volume_max,
            delay_left: s.delay_left,
            delay_right: s.delay_right,
            bass: s.bass,
            treble: s.treble,
            bridged: s.bridged,
            power_mode: s.power_mode,
            power_level: s.power_level,
            power_timeout: s.power_timeout,
            eq_left: s.eq_left,
            eq_right: s.eq_right,
        }
    }
}

impl From<BaySignalDetails> for mxr_signal_details_t {
    fn from(d: BaySignalDetails) -> Self {
        Self {
            frame_rate: d.frame_rate,
            tmds_clock: d.tmds_clock,
            clock_rate: d.clock_rate,
            status: d.status.bits(),
            scaling: d.scaling.to_wire(),
        }
    }
}

impl mxr_device_info_t {
    /// Copies a snapshot into the C shape.
    fn of(info: &DeviceInfo) -> Self {
        let mut out = Self {
            uid: info.uid.into(),
            name: [0; MXR_NAME_LEN],
            serial: [0; MXR_SERIAL_LEN],
            model: [0; MXR_MODEL_LEN],
            version: [0; MXR_VERSION_LEN],
            supported_protocol: info.supported_protocol,
            features: info.features.bits(),
            address: [0; MXR_IP_STRING_LEN],
            status: info.status.into(),
            online: info.online,
            configuration_complete: info.configuration_complete,
            config_initialised: info.config_initialised,
            mesh_master: info.mesh_master.into(),
            hdbt_outputs: info.hdbt_outputs,
            setup_done: info.setup_done.into(),
            // The wire field is 16 bits, so -1 cannot collide with a value a
            // device could report.
            installer_id: info.installer_id.map_or(-1, i32::from),
            has_system_status: info.system_status.is_some(),
            system_status: info.system_status.as_ref().map_or(0, |(code, _)| *code),
            system_message: [0; MXR_MESSAGE_LEN],
            temperature_count: info.temperatures.len(),
            bay_count: info.bays.len(),
        };
        put_str(&mut out.name, &info.name);
        put_str(&mut out.serial, &info.serial);
        put_str(&mut out.model, &info.model);
        put_str(&mut out.version, &info.version);
        if let Some(address) = info.address {
            put_str(&mut out.address, &address.to_string());
        }
        if let Some((_, message)) = &info.system_status {
            put_str(&mut out.system_message, message);
        }
        out
    }
}

impl mxr_bay_info_t {
    /// Copies a snapshot into the C shape.
    fn of(info: &BayInfo) -> Self {
        let mut out = Self {
            uid: info.uid.into(),
            port_name: [0; MXR_NAME_LEN],
            user_name: [0; MXR_NAME_LEN],
            bay_num: info.bay_num,
            features: info.features.bits(),
            is_input: info.is_input,
            is_output: info.is_output,
            is_audio: info.is_audio,
            has_dolby: info.has_dolby,
            is_local: info.is_local,
            video_source: bay_or_zero(info.video_source),
            audio_source: bay_or_zero(info.audio_source),
            power_status: info.power_status.into(),
            hidden: info.hidden.into(),
            faulty: info.faulty.into(),
            poe_powered: info.poe_powered.into(),
            hdbt_connected: info.hdbt_connected.into(),
            signal_detected: info.signal_detected.into(),
            hpd_detected: info.hpd_detected.into(),
            cec_detected: info.cec_detected.into(),
            encoder_disabled: info.encoder_disabled.into(),
            decoder_disabled: info.decoder_disabled.into(),
            signal_type: [0; MXR_SIGNAL_TYPE_LEN],
            signal_mode: info.signal_mode.to_wire(),
            arc: info.arc.into(),
            has_volume: info.volume.is_some(),
            volume: info.volume.map_or(0, |v| v.volume()),
            muted: info.volume.and_then(|v| v.muted()).into(),
            has_rc_type: info.rc_type.is_some(),
            rc_type: info.rc_type.map_or(0, |t| t.to_wire()),
            has_edid_profile: info.edid_profile.is_some(),
            edid_profile: info.edid_profile.map_or(0, |p| p.to_wire()),
            mirror: bay_or_zero(info.mirror.target),
            // An endpoint is a byte on the wire, so -1 cannot collide with one.
            audio_endpoint: info.audio_endpoint.map_or(-1, i16::from),
            linked_bay: bay_or_zero(info.linked_bay),
            v2ip_uid: info.v2ip_uid.into(),
            filtered_count: info.filtered.len(),
        };
        put_str(&mut out.port_name, &info.port_name);
        put_str(&mut out.user_name, &info.user_name);
        if let Some(signal_type) = &info.signal_type {
            put_str(&mut out.signal_type, signal_type);
        }
        out
    }
}

/// Fills `out` with what is known about a device.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_device_info_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_device(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_device_info_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() {
            return null_out("device info");
        }
        match r.remote.device(uid.into()) {
            Some(info) => {
                // SAFETY: checked non-null just above.
                unsafe { *out = mxr_device_info_t::of(&info) };
                mxr_result_t::MXR_OK
            }
            None => not_heard_from(uid),
        }
    })
}

/// Fills `out` with what is known about a bay.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_bay_info_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_bay(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    out: *mut mxr_bay_info_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() {
            return null_out("bay info");
        }
        match r.remote.bay(bay.into()) {
            Some(info) => {
                // SAFETY: checked non-null just above.
                unsafe { *out = mxr_bay_info_t::of(&info) };
                mxr_result_t::MXR_OK
            }
            None => fail(
                mxr_result_t::MXR_ERR_NOT_FOUND,
                &format!("no bay {}", mx_remote::BayUid::from(bay)),
            ),
        }
    })
}

/// Fills `out` with the signal a bay measures.
///
/// Fails with `MXR_ERR_NOT_REPORTED` on a bay that has sent no signal report.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_signal_details_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_bay_signal_details(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    out: *mut mxr_signal_details_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() {
            return null_out("signal details");
        }
        let Some(info) = r.remote.bay(bay.into()) else {
            return fail(
                mxr_result_t::MXR_ERR_NOT_FOUND,
                &format!("no bay {}", mx_remote::BayUid::from(bay)),
            );
        };
        match info.signal_details {
            Some(details) => {
                // SAFETY: checked non-null just above.
                unsafe { *out = details.into() };
                mxr_result_t::MXR_OK
            }
            None => fail(
                mxr_result_t::MXR_ERR_NOT_REPORTED,
                "the bay has reported no signal details",
            ),
        }
    })
}

/// Fills `out` with an amplifier zone's settings.
///
/// Fails with `MXR_ERR_NOT_REPORTED` on a bay that is not an amplifier zone or
/// has not reported its settings.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` points at a writable
/// [`mxr_amp_zone_settings_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_bay_amp_settings(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    out: *mut mxr_amp_zone_settings_t,
) -> mxr_result_t {
    // SAFETY: the caller guarantees a live handle or null.
    let handle = unsafe { remote.as_ref() };
    with(handle, |r| {
        if out.is_null() {
            return null_out("amp zone settings");
        }
        let Some(info) = r.remote.bay(bay.into()) else {
            return fail(
                mxr_result_t::MXR_ERR_NOT_FOUND,
                &format!("no bay {}", mx_remote::BayUid::from(bay)),
            );
        };
        match info.amp_settings {
            Some(settings) => {
                // SAFETY: checked non-null just above.
                unsafe { *out = settings.into() };
                mxr_result_t::MXR_OK
            }
            None => fail(
                mxr_result_t::MXR_ERR_NOT_REPORTED,
                "the bay has reported no amplifier settings",
            ),
        }
    })
}

/// Writes a device's bays in port order, and returns how many there are.
///
/// Returns the full count even when it exceeds `cap`, so calling with `cap`
/// zero sizes the buffer. Returns zero for a device never heard from.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_bay_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_device_bays(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut mxr_bay_uid_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(info) = (unsafe { device_info(remote, uid) }) else {
            return 0;
        };
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&info.bays, out, cap) }
    })
}

/// Writes the temperatures a device reports, in its own order, in degrees
/// Celsius, and returns how many there are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable bytes.
#[no_mangle]
pub unsafe extern "C" fn mxr_device_temperatures(
    remote: *const mxr_remote_t,
    uid: mxr_uid_t,
    out: *mut u8,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(info) = (unsafe { device_info(remote, uid) }) else {
            return 0;
        };
        // SAFETY: the caller guarantees cap writable bytes at out.
        unsafe { copy_into(&info.temperatures, out, cap) }
    })
}

/// Writes the devices whose signals a bay refuses, and returns how many there
/// are.
///
/// # Safety
///
/// `remote` is null or a live handle, and `out` is null or points at `cap`
/// writable [`mxr_uid_t`].
#[no_mangle]
pub unsafe extern "C" fn mxr_bay_filtered(
    remote: *const mxr_remote_t,
    bay: mxr_bay_uid_t,
    out: *mut mxr_uid_t,
    cap: usize,
) -> usize {
    guard(0, || {
        // SAFETY: the caller guarantees a live handle or null.
        let Some(r) = (unsafe { remote.as_ref() }) else {
            fail(
                mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
                "the client handle is null",
            );
            return 0;
        };
        let Some(info) = r.remote.bay(bay.into()) else {
            fail(
                mxr_result_t::MXR_ERR_NOT_FOUND,
                &format!("no bay {}", mx_remote::BayUid::from(bay)),
            );
            return 0;
        };
        // SAFETY: the caller guarantees cap writable elements at out.
        unsafe { copy_into(&info.filtered, out, cap) }
    })
}

/// The snapshot of a device, having reported why there is none.
///
/// # Safety
///
/// `remote` is null or a live handle.
unsafe fn device_info(remote: *const mxr_remote_t, uid: mxr_uid_t) -> Option<DeviceInfo> {
    // SAFETY: the caller guarantees a live handle or null.
    let Some(r) = (unsafe { remote.as_ref() }) else {
        fail(
            mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
            "the client handle is null",
        );
        return None;
    };
    let info = r.remote.device(uid.into());
    if info.is_none() {
        not_heard_from(uid);
    }
    info
}

/// Copies a list into a caller's array and reports how long the list is.
///
/// # Safety
///
/// `out` is null or points at `cap` writable elements.
pub(crate) unsafe fn copy_into<T: Copy, U: Copy + Into<T>>(
    items: &[U],
    out: *mut T,
    cap: usize,
) -> usize {
    if !out.is_null() {
        // SAFETY: the caller guarantees cap writable elements at out.
        let dst = unsafe { std::slice::from_raw_parts_mut(out, cap) };
        for (slot, item) in dst.iter_mut().zip(items) {
            *slot = (*item).into();
        }
    }
    items.len()
}

pub(crate) fn null_out(what: &str) -> mxr_result_t {
    fail(
        mxr_result_t::MXR_ERR_INVALID_ARGUMENT,
        &format!("the {what} output pointer is null"),
    )
}

pub(crate) fn not_heard_from(uid: mxr_uid_t) -> mxr_result_t {
    fail(
        mxr_result_t::MXR_ERR_NOT_FOUND,
        &format!("no device {}", mx_remote::DeviceUid::from(uid)),
    )
}
