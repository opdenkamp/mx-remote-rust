// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Snapshots of a device and of a bay.
//!
//! State lives behind a lock and is read by the receive thread, so a caller is
//! handed a copy rather than a borrow of it. The copy also matches how the C
//! ABI reads state: a uid in, a struct filled out.

use std::net::Ipv4Addr;
use std::time::Instant;

use crate::state::{Bay, Device, State};
use crate::types::{
    AmpZoneSettings, ArcStatus, BayMirrorStatus, BaySignalDetails, DeviceStatus, PowerStatus,
    VolumeMuteStatus,
};
use crate::wire::{
    BayFeatures, BayUid, DeviceFeature, DeviceUid, EdidProfile, MxrSignalType, RcType,
};

/// What a device is, and what it is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceInfo {
    /// The device's unique identifier.
    pub uid: DeviceUid,
    /// The name the device advertises, or `Unknown` before it has said.
    pub name: String,
    /// Serial number, or `Unknown` before the device has said.
    pub serial: String,
    /// A friendly model name derived from the advertised name and the bays the
    /// device reports.
    pub model: String,
    /// Firmware version string from the hello frame.
    pub version: String,
    /// The highest protocol version the device can decode, or zero before it
    /// has said.
    pub supported_protocol: u16,
    /// What the device says it can do.
    pub features: DeviceFeature,
    /// The address the device was last heard from.
    pub address: Option<Ipv4Addr>,
    /// Online, offline, booting or rebooting.
    pub status: DeviceStatus,
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
    /// behind a spuriously set valid bit, the scaling mode and refresh; bay 0's
    /// addresses in the V2IP sources frame; and the padding beside the
    /// remote-control target.
    pub config_initialised: bool,
    /// Temperatures the device reports, in degrees Celsius, in its own order.
    pub temperatures: Vec<u8>,
    /// The mesh master this device follows, or the zero uid when it is in no
    /// mesh.
    pub mesh_master: DeviceUid,
    /// How many HDBaseT outputs this model has.
    pub hdbt_outputs: u8,
    /// Whether installation was marked complete, before the device has said.
    pub setup_done: Option<bool>,
    /// The installer identifier the device carries, if it has reported one.
    pub installer_id: Option<u16>,
    /// A status code and message the device reports about itself.
    pub system_status: Option<(u16, String)>,
    /// Every bay on the device, in port order.
    pub bays: Vec<BayUid>,
}

impl DeviceInfo {
    pub(crate) fn of(device: &Device, now: Instant) -> Self {
        Self {
            uid: device.uid,
            name: device.name().to_owned(),
            serial: device.serial().to_owned(),
            model: device.model_name().to_owned(),
            version: device.hello.version.clone(),
            supported_protocol: device.hello.supported_protocol,
            features: device.hello.features,
            address: device.hello.address,
            status: device.status(now),
            online: device.is_online(now),
            configuration_complete: device.configuration_complete(),
            config_initialised: device.config_initialised(),
            temperatures: device.temperatures.clone(),
            mesh_master: device.mesh_master,
            hdbt_outputs: device.hdbt_outputs(),
            setup_done: device.setup_done,
            installer_id: device.installer_id,
            system_status: device.sys_status.clone(),
            bays: device.bays.values().map(Bay::uid).collect(),
        }
    }
}

/// What a bay is, and what is connected to it.
///
/// A status this bay has never reported is `None`, which is not the same as
/// the status being off: a device reports only what it has.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct BayInfo {
    /// How the bay is addressed: its device and its port.
    pub uid: BayUid,
    /// The name the device gives the port, such as `Output 1`.
    pub port_name: String,
    /// The name the installer gave the bay, falling back to the port name.
    pub user_name: String,
    /// The bay number the device's own API and topology use.
    pub bay_num: u8,
    /// What the bay is wired for.
    pub features: BayFeatures,
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
    /// The bay routed to this one for video.
    pub video_source: Option<BayUid>,
    /// The bay routed to this one for audio, which follows the video source
    /// until the bay is told otherwise.
    pub audio_source: Option<BayUid>,
    /// Power state of what is connected.
    pub power_status: Option<PowerStatus>,
    /// Whether the bay is hidden from the installation's user interface.
    pub hidden: Option<bool>,
    /// Whether the device reports the bay as faulty.
    pub faulty: Option<bool>,
    /// Whether the bay is delivering power over the link.
    pub poe_powered: Option<bool>,
    /// Whether an HDBaseT link is up.
    pub hdbt_connected: Option<bool>,
    /// Whether a signal is present.
    pub signal_detected: Option<bool>,
    /// Whether hot-plug detect is asserted.
    pub hpd_detected: Option<bool>,
    /// Whether a CEC device answered.
    pub cec_detected: Option<bool>,
    /// Whether the bay's encoder is switched off.
    pub encoder_disabled: Option<bool>,
    /// Whether the bay's decoder is switched off.
    pub decoder_disabled: Option<bool>,
    /// The signal as the device describes it, such as `1080p60 444 8`.
    pub signal_type: Option<String>,
    /// The signal as the device measures it.
    pub signal_details: Option<BaySignalDetails>,
    /// The signal format the device reports for the bay.
    pub signal_mode: MxrSignalType,
    /// Whether audio return is active, and over which connector.
    pub arc: ArcStatus,
    /// Volume and mute, for a bay that has them.
    ///
    /// A bay with no volume control of its own reads its
    /// [`linked_bay`](BayInfo::linked_bay)'s, because a link to an amplifier
    /// zone is how the mesh says that zone is this bay's volume.
    pub volume: Option<VolumeMuteStatus>,
    /// The kind of remote control attached.
    pub rc_type: Option<RcType>,
    /// The EDID profile the bay presents.
    pub edid_profile: Option<EdidProfile>,
    /// Which bay this one mirrors, if any.
    pub mirror: BayMirrorStatus,
    /// The audio endpoint this bay feeds, on a device that has them.
    pub audio_endpoint: Option<u8>,
    /// Amplifier settings, on an amplifier zone.
    pub amp_settings: Option<AmpZoneSettings>,
    /// Devices whose signals this bay refuses.
    pub filtered: Vec<DeviceUid>,
    /// The bay on another device this one is linked to, once that bay has been
    /// discovered.
    ///
    /// A link is mesh configuration rather than a route: it names the bay
    /// elsewhere that belongs to this one, such as the amplifier zone carrying
    /// a OneIP output's volume. [`BayInfo::volume`] is already read through it.
    pub linked_bay: Option<BayUid>,
    /// The source device a V2IP bay maps to, or the zero uid.
    pub v2ip_uid: DeviceUid,
}

impl BayInfo {
    pub(crate) fn of(state: &State, bay: &Bay) -> Self {
        Self {
            uid: bay.uid(),
            port_name: bay.port_name.clone(),
            user_name: bay.user_name().to_owned(),
            bay_num: bay.bay_num(),
            features: bay.features,
            is_input: bay.is_input(),
            is_output: bay.is_output(),
            is_audio: bay.is_audio(),
            has_dolby: bay.has_dolby(),
            is_local: bay.is_local(),
            video_source: bay.video_source,
            audio_source: bay.effective_audio_source(),
            power_status: bay.power_status,
            hidden: bay.hidden,
            faulty: bay.faulty,
            poe_powered: bay.poe_powered,
            hdbt_connected: bay.hdbt_connected,
            signal_detected: bay.signal_detected,
            hpd_detected: bay.hpd_detected,
            cec_detected: bay.cec_detected,
            encoder_disabled: bay.encoder_disabled,
            decoder_disabled: bay.decoder_disabled,
            signal_type: bay.signal_type.clone(),
            signal_details: bay.signal_details,
            signal_mode: bay.signal_mode,
            arc: bay.arc,
            volume: state
                .bay(state.volume_bay(bay.uid()))
                .and_then(|b| b.audio_volume),
            rc_type: bay.rc_type,
            edid_profile: bay.edid_profile,
            mirror: bay.mirror,
            audio_endpoint: bay.audio_endpoint,
            amp_settings: bay.amp_settings,
            filtered: bay.filtered.clone(),
            linked_bay: state.linked_bay(bay.uid()),
            v2ip_uid: bay.v2ip_uid,
        }
    }
}
