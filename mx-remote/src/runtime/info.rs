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
    AmpZoneSettings, ArcStatus, BayContentField, BayMirrorStatus, BaySignalDetails, DeviceStatus,
    MacAddress, MeshControllerReport, P8ModelInfo, PowerStatus, VolumeMuteStatus,
};
use crate::wire::{
    BayFeatures, BayStatus, BayUid, DeviceFeature, DeviceUid, EdidProfile, MxrSignalType, RcType,
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
    /// The name exactly as the device announced it: the bytes of its hello's
    /// name field up to the first NUL, or all sixteen when the name fills the
    /// field. Empty before the device has said, and never substituted or
    /// re-encoded as [`DeviceInfo::name`] is.
    pub raw_name: Vec<u8>,
    /// The firmware version exactly as the device announced it, read as
    /// [`DeviceInfo::raw_name`] is.
    pub raw_version: Vec<u8>,
    /// The serial exactly as the device announced it, read as
    /// [`DeviceInfo::raw_name`] is.
    pub raw_serial: Vec<u8>,
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
    /// Whether the device is online as a unit judges it: it is
    /// [`online`](DeviceInfo::online), and its bays have arrived since it was
    /// first heard, last announced a reboot or last went offline. A OneIP
    /// device's bays arrive with its source list, and any other device's with
    /// its primary bay list. Until then a unit uses none of the device's
    /// bays, links or routes.
    pub bays_online: bool,
    /// Whether a frame from the device arrived by multicast within the last
    /// two minutes, which is what a unit means by reaching it by multicast.
    ///
    /// A frame arrived by multicast when it was sent to a multicast group, as
    /// a unit judges it by the frame's destination hardware address: one sent
    /// to this host's own address is not, even on the discovery port. On
    /// Linux that is read from each datagram; elsewhere the endpoint that
    /// took it stands in, which counts such a unicast frame as multicast.
    ///
    /// Every frame with a valid header counts, the second copy of one sent to
    /// both endpoints and one from a device gone silent included, but not the
    /// frame that registered the device, as on a unit. `None` before the
    /// client has started, and on one running a single test endpoint
    /// ([`Config::target_ip`](crate::Config::target_ip)).
    pub multicast: Option<bool>,
    /// The source hardware address of the device's hellos, as a unit stores
    /// it with each one it takes.
    ///
    /// A UDP socket never sees it, so it is read from a copy of each frame
    /// taken below the IP stack, which only
    /// [`Config::capture_hello_mac`](crate::Config::capture_hello_mac) opens.
    /// `None` until a hello from the device has been captured.
    pub mac: Option<MacAddress>,
    /// The IGMP querier the device's last uplink report names, as a unit
    /// stores it; `None` while it names none, its report does not support
    /// IGMP, or it has sent no uplink report of the current layout.
    pub igmp_querier: Option<Ipv4Addr>,
    /// Whether every part of the device's configuration has arrived.
    ///
    /// A device that withholds its link configuration is reported complete
    /// fifteen seconds after it was discovered, and is asked for the rest up to
    /// three times, a minute apart: an unreported link reads as no link, which
    /// is what a link coming up later looks like anyway. Its bays, and a OneIP device's source list,
    /// are waited for without limit - a caller names things after a bay, and a
    /// name assigned to a placeholder outlives the frame that would have
    /// corrected it. A management client has none of these to send and is
    /// complete from its hello.
    pub configuration_complete: bool,
    /// Whether the device's firmware initialises the configuration it
    /// broadcasts.
    ///
    /// Firmware without it builds some frames over uninitialised stack, so
    /// those fields carry noise rather than values: the scaling flags and,
    /// behind a spuriously set valid bit, the scaling mode and refresh; bay 0's
    /// addresses in the OneIP sources frame; and the padding beside the
    /// remote-control target.
    pub config_initialised: bool,
    /// Temperatures the device reports, in degrees Celsius, in its own order.
    /// A sensor the device failed to read reports -55.
    pub temperatures: Vec<i8>,
    /// The mesh master this device follows, or the zero uid when it is in no
    /// mesh.
    pub mesh_master: DeviceUid,
    /// What the device last announced about itself as a mesh controller,
    /// `None` until it has.
    pub mesh_controller_report: Option<MeshControllerReport>,
    /// How many HDBaseT outputs this model has.
    pub hdbt_outputs: u8,
    /// Whether installation was marked complete, `None` before the device
    /// has said.
    pub setup_done: Option<bool>,
    /// The installer identifier the device carries, if it has reported one.
    pub installer_id: Option<u16>,
    /// A status code and message the device reports about itself.
    pub system_status: Option<(u16, String)>,
    /// Every bay on the device, in port order.
    pub bays: Vec<BayUid>,
}

impl DeviceInfo {
    /// `paths_known` says whether frames are told apart by the endpoint they
    /// arrived by.
    pub(crate) fn of(device: &Device, now: Instant, paths_known: bool) -> Self {
        Self {
            uid: device.uid,
            name: device.name().to_owned(),
            serial: device.serial().to_owned(),
            model: device.model_name().to_owned(),
            version: device.hello.version.clone(),
            raw_name: device.hello.raw_name.clone(),
            raw_version: device.hello.raw_version.clone(),
            raw_serial: device.hello.raw_serial.clone(),
            supported_protocol: device.hello.supported_protocol,
            features: device.hello.features,
            address: device.hello.address,
            status: device.status(now),
            online: device.is_online(now),
            bays_online: device.bays_arrived && device.is_online(now),
            multicast: paths_known.then(|| !device.broadcast_only(now)),
            mac: None,
            igmp_querier: device.igmp_querier,
            configuration_complete: device.configuration_complete(now),
            config_initialised: device.config_initialised(),
            temperatures: device.temperatures.clone(),
            mesh_master: device.mesh_master,
            mesh_controller_report: device.mesh_controller_report,
            hdbt_outputs: device.hdbt_outputs(),
            setup_done: device.setup_done,
            installer_id: device.installer_id,
            system_status: device.sys_status.clone(),
            bays: device.bays.values().map(Bay::uid).collect(),
        }
    }
}

/// One device's place in the tree a unit lists its registry as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceTreeEntry {
    /// The device, or this client itself.
    pub uid: DeviceUid,
    /// How deep it sits: zero at the root, one under a device whose topology
    /// report listed it, and so on.
    pub depth: usize,
}

/// What a bay is, and what is connected to it.
///
/// A status this bay has never reported is `None`, which is not the same as
/// the status being off: a device reports only what it has.
///
/// A mesh bay, a OneIP bay on one unit standing for another unit's input or
/// output, reports that unit's own bay 0 in the same direction. Only its
/// identity is its own: `uid`, `port_name`, `bay_num`, `features`, its
/// direction, `is_local` and `v2ip_uid`, which names that unit. Every change
/// to that bay is reported for each mesh bay standing for it as well. Until
/// the unit has reported the bay, the rest reads as never reported.
///
/// A command about a mesh bay goes to that unit about that bay, and is
/// refused until the unit has reported it. A route keeps naming its source
/// by a port of the device listing the mesh bay.
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
    /// The bay's whole status word as the device last sent it, in its bay
    /// list, a status report or a signal report; `None` before any. It carries what no
    /// other field does, such as
    /// [`ENCODER_ERROR`](BayStatus::ENCODER_ERROR) and the source's HDCP mode
    /// ([`BayStatus::hdcp`]).
    pub status: Option<BayStatus>,
    /// Whether the bay is hidden from the installation's user interface.
    pub hidden: Option<bool>,
    /// Whether the device reports the bay as faulty.
    pub faulty: Option<bool>,
    /// Whether the amplifier zone the bay drives is powered on.
    pub zone_powered: Option<bool>,
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
    /// The Pulse-Eight device the device last reported connected to the bay,
    /// from the long form of its connect frame; `None` after a short or
    /// disconnecting one.
    pub p8_device: Option<P8ModelInfo>,
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
    /// The unit a OneIP bay stands for, or the zero uid.
    pub v2ip_uid: DeviceUid,
    /// What the source on an input is showing, as its unit last reported.
    ///
    /// A unit reports this whenever it changes, and keeps it while the source
    /// has no signal; check [`BayInfo::signal_detected`] before showing it.
    pub content: Vec<BayContentField>,
}

impl BayInfo {
    pub(crate) fn of(state: &State, bay: &Bay) -> Self {
        let d = state.described(bay);
        Self {
            uid: bay.uid(),
            port_name: bay.port_name.clone(),
            user_name: d.user_name().to_owned(),
            bay_num: bay.bay_num(),
            features: bay.features,
            is_input: bay.is_input(),
            is_output: bay.is_output(),
            is_audio: bay.is_audio(),
            has_dolby: bay.has_dolby(),
            is_local: bay.is_local(),
            video_source: d.video_source,
            audio_source: d.effective_audio_source(),
            power_status: d.power_status,
            status: d.status_word,
            hidden: d.hidden,
            faulty: d.faulty,
            zone_powered: d.zone_powered,
            hdbt_connected: d.hdbt_connected,
            signal_detected: d.signal_detected,
            hpd_detected: d.hpd_detected,
            cec_detected: d.cec_detected,
            encoder_disabled: d.encoder_disabled,
            decoder_disabled: d.decoder_disabled,
            signal_type: d.signal_type.clone(),
            signal_details: d.signal_details,
            p8_device: bay.p8_device.clone(),
            signal_mode: d.signal_mode,
            arc: d.arc,
            volume: state
                .bay(state.volume_bay(d.uid()))
                .and_then(|b| b.audio_volume),
            rc_type: d.rc_type,
            edid_profile: d.edid_profile,
            mirror: d.mirror,
            audio_endpoint: d.audio_endpoint,
            amp_settings: d.amp_settings,
            filtered: d.filtered.clone(),
            linked_bay: state.linked_bay(d.uid()),
            v2ip_uid: bay.v2ip_uid,
            content: d.content.clone(),
        }
    }
}
