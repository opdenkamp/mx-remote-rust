// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A discovered device and the bays it owns.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use crate::event::Event;
use crate::types::{
    AmpDolbySettings, AudioChangeSource, AudioEndpoints, AudioLink, DeviceStatus,
    DeviceV2ipDetails, DeviceV2ipSink, FirmwareVersion, MultiviewerStatus, NetworkPortStatus,
    OsdMessage, OsdSync, OsdSyncReport, RcSettings, TimeZone, TopologyEntry, UnitStatus,
    V2ipDeviceSettings, V2ipDeviceStats, V2ipDownmix, V2ipPrivacy, V2ipPtpState,
    V2ipScalingSettings, V2ipSinkSdp, V2ipStreamSources, V2ipTestcard, V2ipTilingConfig, V2ipVlan,
    VolumeMuteStatus, SCALING_FLAG_MODE_VALID,
};
use crate::wire::{
    BayConfig, BayFeatures, BayUid, DeviceFeature, DeviceUid, FirmwareType, V2ipFpgaFeature,
};

use super::bay::Bay;

/// A session description arriving in pages, as far as it has got.
#[derive(Clone, Debug)]
pub(crate) struct SdpPages {
    /// The length of the whole description.
    pub(crate) total: u16,
    /// The bytes received so far, from offset 0.
    pub(crate) data: Vec<u8>,
    /// When the last page arrived.
    pub(crate) at: Instant,
}

/// How long a device below [`MODERN_PROTOCOL`] may stay silent before it
/// counts as offline.
const SILENCE_LIMIT: Duration = Duration::from_secs(180);

/// How long a device at [`MODERN_PROTOCOL`] or above may stay silent.
///
/// That version announces every few seconds, so silence becomes meaningful
/// long before three minutes of it have passed.
const SILENCE_LIMIT_MODERN: Duration = Duration::from_secs(15);

/// The version from which the shorter limit applies.
const MODERN_PROTOCOL: u16 = 0x20;

/// How long a device at [`SLOW_HELLO_PROTOCOL`] or above may stay silent.
///
/// That version announces only every 20 to 30 seconds once every device it
/// knows is on it too.
const SILENCE_LIMIT_SLOW_HELLO: Duration = Duration::from_secs(60);

/// The version from which [`SILENCE_LIMIT_SLOW_HELLO`] applies.
const SLOW_HELLO_PROTOCOL: u16 = 0x2A;

/// How long a device is given to finish describing itself.
///
/// Past this its link configuration stops being waited for, and it is asked for
/// the rest a few times. It never stands in for the bay configuration - see
/// [`Device::has_bays`].
pub(crate) const CONFIG_GRACE: Duration = Duration::from_secs(15);

/// What a device advertises about itself in its hello frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HelloInfo {
    pub(crate) supported_protocol: u16,
    pub(crate) name: String,
    pub(crate) serial: String,
    pub(crate) version: String,
    pub(crate) features: DeviceFeature,
    pub(crate) address: Option<Ipv4Addr>,
}

impl HelloInfo {
    /// Compares what the device said about itself, ignoring where it said it
    /// from: an address change is a route change, not a configuration change.
    fn same_advertisement(&self, other: &Self) -> bool {
        self.supported_protocol == other.supported_protocol
            && self.name == other.name
            && self.serial == other.serial
            && self.version == other.version
            && self.features == other.features
    }
}

/// A device on the MX Remote network: a matrix, a OneIP unit or an amplifier.
#[derive(Clone, Debug)]
pub(crate) struct Device {
    pub(crate) uid: DeviceUid,
    pub(crate) hello: HelloInfo,
    pub(crate) bays: BTreeMap<u16, Bay>,
    pub(crate) temperatures: Vec<i8>,
    pub(crate) online: bool,
    pub(crate) have_config: bool,
    pub(crate) rebooting: bool,
    pub(crate) last_ping: Instant,
    /// When this device was first heard from.
    ///
    /// What [`CONFIG_GRACE`] is measured against, so it is the moment the
    /// device entered the registry and not the last hello: a device announces
    /// every few seconds, and a window restarted by each announcement is one
    /// that never closes.
    pub(crate) first_seen: Instant,
    /// The discovers naming this device that asked it for what it still owes,
    /// and when the last one went. Both reset once it has described itself.
    pub(crate) owed_asks: u8,
    pub(crate) last_owed_ask: Option<Instant>,
    /// Whether the device has reported its link configuration.
    pub(crate) link_config_received: bool,
    /// Whether the device has sent its primary bay configuration.
    ///
    /// Every unit sends that list whatever else it sends, so it is the one
    /// thing whose arrival can be required of any device.
    pub(crate) bay_config_received: bool,

    pub(crate) v2ip_sources: Option<Vec<V2ipStreamSources>>,
    /// Source records by their position in the sender's list.
    ///
    /// A paged list arrives as windows that can be reordered or lost, so the
    /// records are held by position and `v2ip_sources` is the run of them that
    /// has arrived from the start.
    pub(crate) v2ip_source_pages: BTreeMap<u16, V2ipStreamSources>,
    pub(crate) v2ip_details: Option<DeviceV2ipDetails>,
    /// The time zone the device last announced for its mesh.
    pub(crate) time_zone: Option<TimeZone>,
    /// The time the device last announced, in seconds since 1970 UTC, and when
    /// that frame arrived.
    pub(crate) clock: Option<(u32, Instant)>,
    pub(crate) v2ip_sink: Option<DeviceV2ipSink>,
    /// What the device's video processor supports, once it has reported it.
    ///
    /// `None` until a frame the device sent about itself carries a non-empty
    /// mask: a device leaves the field zero when configuring someone else, and
    /// reports zero of its own while its processor has yet to answer.
    pub(crate) v2ip_features: Option<V2ipFpgaFeature>,
    /// The device settings, once any has been reported.
    pub(crate) v2ip_settings: Option<V2ipDeviceSettings>,
    /// The VLAN configuration the device last reported about itself.
    pub(crate) v2ip_vlan: Option<V2ipVlan>,
    /// The health and IP configuration the unit last reported.
    pub(crate) unit_status: Option<UnitStatus>,
    /// What the output last reported playing from a session description.
    pub(crate) v2ip_sink_sdp: Option<V2ipSinkSdp>,
    /// The session description the output last reported playing, `None`
    /// until it has, and once it reports playing none.
    pub(crate) v2ip_sink_description: Option<String>,
    /// The pages of a description the output is still sending.
    pub(crate) v2ip_sink_description_pages: Option<SdpPages>,
    pub(crate) v2ip_privacy: Option<V2ipPrivacy>,
    pub(crate) osd_sync: Option<OsdSync>,
    pub(crate) osd_sync_report: Option<OsdSyncReport>,
    pub(crate) osd_message: Option<OsdMessage>,
    /// The PTP state the device last reported about itself, `None` while it
    /// reports none.
    pub(crate) v2ip_ptp: Option<V2ipPtpState>,
    /// How the device's output mixes down to stereo, `None` while it reports
    /// no downmix.
    pub(crate) v2ip_downmix: Option<V2ipDownmix>,
    /// The test card the sink last reported.
    pub(crate) v2ip_testcard: Option<V2ipTestcard>,
    pub(crate) v2ip_stats: Option<V2ipDeviceStats>,
    pub(crate) setup_done: Option<bool>,
    pub(crate) installer_id: Option<u16>,
    pub(crate) tiling: Option<V2ipTilingConfig>,
    pub(crate) rc_settings: Option<RcSettings>,
    pub(crate) audio_select: Option<AudioChangeSource>,
    pub(crate) firmware: BTreeMap<FirmwareType, FirmwareVersion>,
    pub(crate) mesh_master: DeviceUid,
    pub(crate) network: BTreeMap<u16, NetworkPortStatus>,
    pub(crate) sys_status: Option<(u16, String)>,
    pub(crate) topology: Vec<TopologyEntry>,
    pub(crate) audio: Option<AudioEndpoints>,
    /// Whether the device's audio module uses the layout of released firmware
    /// up to 4.4.2, as its own features report showed; `None` before one.
    pub(crate) audio_legacy: Option<bool>,
    pub(crate) multiviewer: Option<MultiviewerStatus>,
    pub(crate) dolby_settings: Option<AmpDolbySettings>,
    /// The EDID the device presents on its input, as it last reported it.
    pub(crate) edid_input: Option<Vec<u8>>,
    /// The EDID of the display on the device's output, as it last reported it.
    pub(crate) edid_output: Option<Vec<u8>>,
}

impl Device {
    pub(crate) fn new(uid: DeviceUid, hello: HelloInfo, now: Instant) -> Self {
        Self {
            uid,
            hello,
            bays: BTreeMap::new(),
            temperatures: Vec::new(),
            online: true,
            have_config: false,
            rebooting: false,
            last_ping: now,
            first_seen: now,
            owed_asks: 0,
            last_owed_ask: None,
            link_config_received: false,
            bay_config_received: false,
            v2ip_sources: None,
            v2ip_source_pages: BTreeMap::new(),
            v2ip_details: None,
            time_zone: None,
            clock: None,
            v2ip_sink: None,
            v2ip_features: None,
            v2ip_settings: None,
            v2ip_vlan: None,
            v2ip_ptp: None,
            v2ip_downmix: None,
            unit_status: None,
            v2ip_sink_sdp: None,
            v2ip_sink_description: None,
            v2ip_sink_description_pages: None,
            v2ip_privacy: None,
            osd_sync: None,
            osd_sync_report: None,
            osd_message: None,
            v2ip_testcard: None,
            v2ip_stats: None,
            setup_done: None,
            installer_id: None,
            tiling: None,
            rc_settings: None,
            audio_legacy: None,
            audio_select: None,
            firmware: BTreeMap::new(),
            mesh_master: DeviceUid::ZERO,
            network: BTreeMap::new(),
            sys_status: None,
            topology: Vec::new(),
            audio: None,
            multiviewer: None,
            dolby_settings: None,
            edid_input: None,
            edid_output: None,
        }
    }

    /// The EDID the device last reported, for its output or for its input.
    pub(crate) fn edid(&self, output: bool) -> Option<&[u8]> {
        let stored = if output {
            &self.edid_output
        } else {
            &self.edid_input
        };
        stored.as_deref()
    }

    /// Stores an EDID the device reported.
    pub(crate) fn set_edid(&mut self, output: bool, data: Vec<u8>) {
        if output {
            self.edid_output = Some(data);
        } else {
            self.edid_input = Some(data);
        }
    }

    // ---- identity ----

    pub(crate) fn serial(&self) -> &str {
        if self.hello.serial.is_empty() {
            "Unknown"
        } else {
            &self.hello.serial
        }
    }

    pub(crate) fn name(&self) -> &str {
        if self.hello.name.is_empty() {
            "Unknown"
        } else if self.hello.name.trim().is_empty() {
            "<unnamed>"
        } else {
            &self.hello.name
        }
    }

    // ---- type checks ----

    pub(crate) fn is_v2ip(&self) -> bool {
        self.hello.features.has(DeviceFeature::V2IP_SINK)
            || self.hello.features.has(DeviceFeature::V2IP_SOURCE)
    }

    /// Whether the device reports a OneIP sink, which is what carries the
    /// scaling settings.
    pub(crate) fn is_v2ip_sink(&self) -> bool {
        self.hello.features.has(DeviceFeature::V2IP_SINK)
    }

    pub(crate) fn is_video_matrix(&self) -> bool {
        self.hello.features.has(DeviceFeature::VIDEO_ROUTING)
    }

    pub(crate) fn is_audio_matrix(&self) -> bool {
        self.hello.features.has(DeviceFeature::AUDIO_ROUTING)
            && !self.hello.features.has(DeviceFeature::VIDEO_ROUTING)
    }

    pub(crate) fn is_amp(&self) -> bool {
        self.hello.features.has(DeviceFeature::VOLUME_CONTROL) && self.is_audio_matrix()
    }

    pub(crate) fn is_multiviewer(&self) -> bool {
        self.is_v2ip() && self.hello.features.has(DeviceFeature::MULTIVIEWER)
    }

    /// Whether this device's firmware initialises the configuration it
    /// broadcasts.
    ///
    /// Firmware without it builds some frames over uninitialised stack, so
    /// those fields carry noise rather than values: the scaling flags and,
    /// behind a spuriously set valid bit, the scaling mode and refresh; bay 0's
    /// addresses in the OneIP sources frame; and the padding beside the
    /// remote-control target.
    pub(crate) fn config_initialised(&self) -> bool {
        self.hello.features.has(DeviceFeature::CONFIG_INITIALISED)
    }

    pub(crate) fn has_local_source(&self) -> bool {
        self.first_input().is_some_and(Bay::is_local)
    }

    pub(crate) fn has_local_sink(&self) -> bool {
        self.first_output().is_some_and(Bay::is_local)
    }

    // ---- status ----

    pub(crate) fn is_online(&self, now: Instant) -> bool {
        let limit = if self.hello.supported_protocol >= SLOW_HELLO_PROTOCOL {
            SILENCE_LIMIT_SLOW_HELLO
        } else if self.hello.supported_protocol >= MODERN_PROTOCOL {
            SILENCE_LIMIT_MODERN
        } else {
            SILENCE_LIMIT
        };
        now.saturating_duration_since(self.last_ping) < limit
    }

    pub(crate) fn is_rebooting(&self, now: Instant) -> bool {
        self.rebooting
            || (self.is_online(now) && self.hello.features.has(DeviceFeature::STATUS_REBOOT))
    }

    pub(crate) fn status(&self, now: Instant) -> DeviceStatus {
        if !self.is_online(now) {
            return DeviceStatus::Offline;
        }
        if self.is_rebooting(now) {
            return DeviceStatus::Rebooting;
        }
        if self.hello.features.has(DeviceFeature::BOOTING) {
            return DeviceStatus::Booting;
        }
        DeviceStatus::Online
    }

    // ---- bays ----

    pub(crate) fn bay(&self, port: u16) -> Option<&Bay> {
        self.bays.get(&port)
    }

    /// A bay to change, which a mesh bay is not: it describes itself through
    /// the bay it stands for, so what the unit listing it says about it is
    /// the unit's copy of that bay, and changes nothing.
    pub(crate) fn bay_mut(&mut self, port: u16) -> Option<&mut Bay> {
        self.bays.get_mut(&port).filter(|b| !b.is_v2ip_remote())
    }

    pub(crate) fn bay_by_name(&self, name: &str) -> Option<&Bay> {
        self.bays.values().find(|b| b.port_name == name)
    }

    /// The bay the device's own API would call `mode` number `bay`.
    pub(crate) fn bay_by_mode_num(&self, mode: &str, bay: u8) -> Option<&Bay> {
        self.bays
            .values()
            .find(|b| b.mode_str() == mode && b.bay_num() == bay)
    }

    pub(crate) fn first_input(&self) -> Option<&Bay> {
        self.bays.values().find(|b| b.is_input())
    }

    pub(crate) fn first_output(&self) -> Option<&Bay> {
        self.bays.values().find(|b| b.is_output())
    }

    /// The unit's own bay 0 in the given direction, which its mesh bays on
    /// other units stand for.
    pub(crate) fn local_bay(&self, input: bool) -> Option<&Bay> {
        self.bays
            .values()
            .find(|b| b.is_local() && if input { b.is_input() } else { b.is_output() })
    }

    pub(crate) fn first_output_port(&self) -> Option<u16> {
        self.first_output().map(|b| b.port)
    }

    /// The streams the given source bay advertises.
    ///
    /// The OneIP sources frame lists one record per source bay in bay order. A
    /// receiver with no local source still sends a record for the bay it does
    /// not have, so its list is offset by one.
    pub(crate) fn v2ip_source_for(&self, bay: &Bay) -> Option<&V2ipStreamSources> {
        if !bay.is_input() || !self.is_v2ip() {
            return None;
        }
        let offset = u8::from(!self.has_local_source());
        let index = bay.bay_num().checked_sub(offset)?;
        self.v2ip_sources.as_ref()?.get(usize::from(index))
    }

    /// The cross-device identity a bay is linked by.
    ///
    /// A OneIP source is the same physical input wherever it appears, so it is
    /// keyed by the device producing the stream rather than by the port it
    /// happens to be mapped to. Everything else is keyed by its own port.
    pub(crate) fn link_key(&self, bay: &Bay) -> BayUid {
        if bay.is_v2ip_source() {
            let from_stream = self
                .v2ip_source_for(bay)
                .map(|s| s.uid)
                .filter(|uid| !uid.is_zero());
            let source = from_stream.or_else(|| Some(bay.v2ip_uid).filter(|u| !u.is_zero()));
            if let Some(source) = source {
                return BayUid::new(source, 0);
            }
        }
        bay.uid()
    }

    // ---- configuration completeness ----

    /// Whether the device's bay configuration has arrived.
    ///
    /// That it was sent at all is the whole of what can be established. A
    /// device pages its bays and nothing on the wire marks the last page -
    /// no count, no index, no terminating frame - and nothing it says about
    /// itself gives the number to expect, so counting what arrived can only be
    /// compared against a guess.
    ///
    /// For a OneIP device this is half the answer by itself: its bays include
    /// ones that live on other devices, and those arrive on a frame of their
    /// own that [`Self::configuration_complete`] requires separately.
    ///
    /// Unlike the link configuration, waiting for this never times out. A bay
    /// is what a caller names things after, and a name it assigns before this
    /// frame arrives is one it assigned to a placeholder - persisted, reused
    /// from then on, and undone only by editing whatever holds it. So a device
    /// that has not sent its bays is reported as undescribed for as long as
    /// that lasts.
    fn has_bays(&self) -> bool {
        self.bay_config_received
    }

    /// Whether the device has yet to report its link configuration.
    ///
    /// That it reported at all is the whole of what can be established, and no
    /// count of bays stands in for it. Two reasons, either enough on its own:
    ///
    /// A link record describes one of the sender's own ports. Most of a OneIP
    /// device's bays are proxies for streams that live on other devices, and
    /// those own no record - a 14-bay transceiver reports two, for its local
    /// input and its local output.
    ///
    /// A list longer than one payload is cut to what fits rather than
    /// continued. An 18-bay amplifier reports 17 records in one page and sends
    /// no second one, so even a device whose bays are all its own never
    /// accounts for the last of them.
    ///
    /// Waiting ends after [`CONFIG_GRACE`], and the device is asked for the
    /// rest a few times. Nothing a caller has already built needs revising when the
    /// records do arrive: a link is reported per bay and read on access, so an
    /// unreported one reads as no link, which is what a link coming up later
    /// looks like anyway. A withheld frame therefore costs a consumer that
    /// window rather than leaving the device permanently undescribed, a state
    /// nothing on the wire distinguishes from a device with no links to offer.
    fn needs_link_config(&self, now: Instant) -> bool {
        (self.is_amp() || self.is_video_matrix() || self.is_audio_matrix() || self.is_v2ip())
            && !self.link_config_received
            && now.saturating_duration_since(self.first_seen) <= CONFIG_GRACE
    }

    /// Whether the device has described itself fully.
    ///
    /// A management client is described by its hello alone: it has no bays or
    /// links to send. That is [`DeviceFeature::MANAGER`], which no unit sets.
    pub(crate) fn configuration_complete(&self, now: Instant) -> bool {
        if self.hello.features.has(DeviceFeature::MANAGER) {
            return true;
        }
        self.has_bays()
            && !(self.is_v2ip() && self.v2ip_sources.is_none())
            && !self.needs_link_config(now)
    }

    /// Announces completion the first time every part has arrived.
    ///
    /// Also called on every pass of the probe loop, which is what gives the
    /// window in [`Self::needs_link_config`] a moment to expire in: no frame
    /// need arrive for a device to stop waiting for its links, so that pass is
    /// the only thing that can notice.
    pub(crate) fn check_config_complete(&mut self, now: Instant, ev: &mut Vec<Event>) {
        if self.have_config || !self.configuration_complete(now) {
            return;
        }
        self.have_config = true;
        ev.push(Event::DeviceConfigComplete { device: self.uid });
    }

    /// How many HDBaseT outputs this model has, by name.
    pub(crate) fn hdbt_outputs(&self) -> u8 {
        let name = self.hello.name.as_str();
        if name.starts_with("FF88") {
            8
        } else if name.starts_with("FF66") {
            6
        } else if name.starts_with("FF64")
            || name.starts_with("SP14")
            || matches!(name, "FFMB44" | "FFMS44" | "FFMG44")
        {
            4
        } else {
            0
        }
    }

    /// A friendly model name, from the hello name for matrices and from the
    /// bays it actually has for a OneIP unit.
    pub(crate) fn model_name(&self) -> &str {
        if self.is_v2ip() {
            return match (
                self.is_multiviewer(),
                self.has_local_source(),
                self.has_local_sink(),
            ) {
                (true, _, _) => "OneIP Multiviewer",
                (_, true, true) => "OneIP Transceiver",
                (_, true, false) => "OneIP Transmitter",
                _ => "OneIP Receiver",
            };
        }
        match self.hello.name.as_str() {
            "PROAMP8" => "ProAmp8",
            "PROAMPv2" => "ProAmp8 v2",
            "FFMB44" => "neo:4 Bronze",
            "FFMS44" => "neo:4 Silver",
            "FFMG44" => "neo:4 Gold",
            "FF88SA" | "FF88S" | "FF88T" => "neo:X",
            "FF88" => "neo:8",
            "FF88A" | "FF88A1" => "neo:8 Audio",
            "FF66SA" => "neo:6 X",
            "FF66A" | "FF66A1" => "neo:6 Audio",
            "FF64S" => "neo:6",
            "SP14" | "SP142" => "neo:4 Splitter",
            other => other,
        }
    }

    // ---- mutators ----

    pub(crate) fn apply_hello(&mut self, hello: HelloInfo, now: Instant, ev: &mut Vec<Event>) {
        self.last_ping = now;
        let changed = !self.hello.same_advertisement(&hello);
        // A unit about to reboot says so in its hello, and what it said
        // about its bays holds only until it has said it again.
        if hello.features.has(DeviceFeature::STATUS_REBOOT) {
            self.await_config();
        }
        self.hello = hello;
        self.rebooting = false;
        if changed {
            ev.push(Event::DeviceConfigChanged { device: self.uid });
        }
    }

    /// Merges one bay descriptor, resolving its routed source ports against
    /// the bays this device already has.
    ///
    /// A record with an empty name describes no bay, as a unit sends for an
    /// input it cannot number, and is dropped. So is a record for a OneIP bay
    /// standing for another unit: what it says about that unit's bay is never
    /// refreshed, and the bay is built from the bay mappings instead.
    pub(crate) fn apply_bay_config(&mut self, cfg: &BayConfig, now: Instant, ev: &mut Vec<Event>) {
        self.last_ping = now;
        if cfg.bay_name.is_empty()
            || cfg.features.has(BayFeatures::V2IP_SOURCE_REMOTE)
            || cfg.features.has(BayFeatures::V2IP_SINK_REMOTE)
        {
            return;
        }
        let is_v2ip = self.is_v2ip();
        let video = self.routed_source(u16::from(cfg.video_source));
        let audio = self.routed_source(u16::from(cfg.audio_source));
        let is_new = !self.bays.contains_key(&u16::from(cfg.port));

        let bay = self
            .bays
            .entry(u16::from(cfg.port))
            .or_insert_with(|| Bay::new(self.uid, cfg));

        bay.features = cfg.features;
        bay.mode = cfg.modenum;
        bay.status_mask = cfg.status;
        bay.set_user_name(cfg.user_name.clone(), ev);
        if bay.mbay_id.is_none() {
            bay.mbay_id = Some(cfg.bay);
        }
        // A unit maps its own OneIP bay to itself.
        if bay.is_v2ip_source() || bay.is_v2ip_sink() {
            bay.v2ip_uid = self.uid;
        }
        bay.apply_bay_status(cfg.status, ev);
        bay.signal_mode = cfg.signal_mode;
        // A OneIP source reporting a signal describes it in its own detailed
        // report, which carries the frame rate this field has no room for.
        if !cfg.status.has(crate::wire::BayStatus::SIGNAL_DETECTED) || !is_v2ip {
            bay.set_signal_type(cfg.signal_type.clone(), ev);
        }
        if bay.is_output() {
            // A port that names no source, 0xFF among them, keeps the route:
            // a unit sends 0xFF for both whenever either is unrouted, or
            // routed to a port too wide for the field. Its routing change
            // report is what clears a route.
            if video.is_some() {
                bay.set_video_source(video, ev);
            }
            if audio.is_some() {
                bay.set_audio_source(audio, ev);
            }
        } else {
            bay.set_rc_type(cfg.rc_type, ev);
            bay.set_edid_profile(cfg.edid_profile, ev);
        }

        if is_new {
            ev.push(Event::BayRegistered {
                bay: BayUid::new(self.uid, u16::from(cfg.port)),
            });
            // The audio tree names the bays it runs through, and may have
            // arrived before this one did.
            self.attach_audio_endpoints(ev);
            self.check_config_complete(now, ev);
        }
    }

    /// Takes one page of the device's OneIP bay mappings: the unit owning
    /// each bay from the one on `first_port` on, and returns the mesh bays it
    /// registered or pointed at another unit.
    ///
    /// The entries run on by bay number in the direction of the first bay,
    /// which is a run of ports only in some numberings. The unit's own bays
    /// come from its bay configuration, and an entry naming the unit itself is
    /// one of them. Every other entry is a mesh bay, which a page creates. A
    /// zero uid maps nothing: released firmware counts more bays than it has,
    /// and sends one for each extra. The page ends past the last bay.
    pub(crate) fn set_v2ip_bay_mapping_page(
        &mut self,
        first_port: u16,
        page: &[DeviceUid],
    ) -> Vec<(BayUid, MeshBayChange)> {
        let layout = self.port_layout();
        let mut changes = Vec::new();
        let Some((is_input, first)) = layout.decode(first_port) else {
            return changes;
        };
        let bays = (first..=u8::MAX).map_while(|n| Some((n, layout.encode(is_input, n)?)));
        for ((number, port), owner) in bays.zip(page) {
            if owner.is_zero() || *owner == self.uid {
                continue;
            }
            match self.bays.get_mut(&port) {
                Some(bay) if bay.v2ip_uid == *owner => {}
                Some(bay) => {
                    bay.v2ip_uid = *owner;
                    changes.push((bay.uid(), MeshBayChange::Remapped));
                }
                None => {
                    let bay = Bay::mesh(self.uid, port, is_input, number, *owner);
                    changes.push((bay.uid(), MeshBayChange::Registered));
                    self.bays.insert(port, bay);
                }
            }
        }
        changes
    }

    /// How the unit numbers its bays' ports, as its protocol says.
    fn port_layout(&self) -> PortLayout {
        if self.hello.supported_protocol >= FIXED_PORTS_SINCE {
            PortLayout::Fixed
        } else {
            PortLayout::Split
        }
    }

    /// The identity of a local bay named by port number in a routing report.
    ///
    /// A OneIP unit's input port names a bay even before the unit's mappings
    /// have created it, as they do after its bay configuration.
    pub(crate) fn routed_source(&self, port: u16) -> Option<BayUid> {
        let named = self.bays.contains_key(&port)
            || (self.is_v2ip()
                && self
                    .port_layout()
                    .decode(port)
                    .is_some_and(|(input, _)| input));
        named.then(|| BayUid::new(self.uid, port))
    }

    /// Treats the device as undescribed until it sends its bays again, as a
    /// unit does for one that went offline or rebooted.
    fn await_config(&mut self) {
        self.have_config = false;
        self.bay_config_received = false;
    }

    /// Notes that the device has sent its primary bay configuration.
    pub(crate) fn note_bay_config(&mut self, now: Instant, ev: &mut Vec<Event>) {
        if self.bay_config_received {
            return;
        }
        self.bay_config_received = true;
        self.check_config_complete(now, ev);
    }

    /// Notes that the device has reported its link configuration.
    pub(crate) fn note_link_config(&mut self, now: Instant, ev: &mut Vec<Event>) {
        if self.link_config_received {
            return;
        }
        self.link_config_received = true;
        self.check_config_complete(now, ev);
    }

    pub(crate) fn set_temperatures(&mut self, temperatures: Vec<i8>, ev: &mut Vec<Event>) {
        if self.temperatures == temperatures {
            return;
        }
        self.temperatures.clone_from(&temperatures);
        ev.push(Event::DeviceTemperatureChanged {
            device: self.uid,
            temperatures,
        });
    }

    /// Records that the device was heard from at `stamped`, and re-evaluates
    /// its liveness as of `now`.
    ///
    /// The two clocks differ whenever a datagram is handled later than it
    /// arrived. The frame's own time says when the device was last heard; only
    /// the current time says how long ago that was, so measuring the gap from
    /// the frame's time would make every device look freshly seen.
    pub(crate) fn touch(&mut self, stamped: Instant, now: Instant, ev: &mut Vec<Event>) {
        self.last_ping = stamped;
        self.check_online(now, ev);
    }

    pub(crate) fn check_online(&mut self, now: Instant, ev: &mut Vec<Event>) {
        let online = self.is_online(now);
        if online == self.online {
            return;
        }
        self.online = online;
        if !online {
            self.await_config();
        }
        ev.push(Event::DeviceOnlineChanged {
            device: self.uid,
            online,
        });
    }

    pub(crate) fn set_firmware_version(&mut self, version: FirmwareVersion, ev: &mut Vec<Event>) {
        if self.firmware.get(&version.firmware_type) == Some(&version) {
            return;
        }
        self.firmware.insert(version.firmware_type, version.clone());
        ev.push(Event::FirmwareVersionChanged {
            device: self.uid,
            version,
        });
    }

    pub(crate) fn set_system_status(&mut self, status: u16, message: String, ev: &mut Vec<Event>) {
        let current = (status, message);
        if self.sys_status.as_ref() == Some(&current) {
            return;
        }
        self.sys_status = Some(current.clone());
        ev.push(Event::SystemStatusChanged {
            device: self.uid,
            status: current.0,
            message: current.1,
        });
    }

    pub(crate) fn update_network_status(&mut self, status: NetworkPortStatus, ev: &mut Vec<Event>) {
        if self.network.get(&status.port) == Some(&status) {
            return;
        }
        self.network.insert(status.port, status.clone());
        ev.push(Event::NetworkStatusChanged {
            device: self.uid,
            status,
        });
    }

    pub(crate) fn set_v2ip_stats(&mut self, stats: V2ipDeviceStats, ev: &mut Vec<Event>) {
        self.v2ip_stats = Some(stats);
        ev.push(Event::V2ipStatsChanged {
            device: self.uid,
            stats,
        });
    }

    /// How many amplifier outputs the reported Dolby mode groups together.
    ///
    /// Zero unless the amplifier is in a Dolby mode, and the group is always
    /// the outputs numbered below the count: `mxr_amp_dolby_settings`
    /// spells its `dolby_config` as 0 standard, 1 three-zone, 2 four-zone.
    /// The bay feature bit does not describe the group - an output outside it
    /// can carry the bit and name the same Dolby input.
    fn dolby_zones(&self) -> u8 {
        match self.dolby_settings.map(|d| d.mode) {
            Some(1) => 3,
            Some(2) => 4,
            _ => 0,
        }
    }

    /// Applies a volume to a bay, and to the rest of its Dolby group.
    ///
    /// An amplifier reports one volume for a Dolby group, against its first
    /// output, because the group is driven as one. Every output in it holds
    /// that volume, so a caller reading any of them sees what the zone is set
    /// to rather than nothing at all.
    pub(crate) fn apply_bay_volume(
        &mut self,
        port: u16,
        volume: VolumeMuteStatus,
        ev: &mut Vec<Event>,
    ) {
        let Some(bay) = self.bay_mut(port) else {
            return;
        };
        bay.set_volume_status(volume, ev);

        let zones = self.dolby_zones();
        let leads_group = self
            .bay(port)
            .is_some_and(|b| b.is_output() && b.has_dolby() && b.bay_num() == 0);
        if zones == 0 || !self.is_amp() || !leads_group {
            return;
        }
        let group: Vec<u16> = self
            .bays
            .values()
            .filter(|b| b.is_output() && b.has_dolby() && (1..zones).contains(&b.bay_num()))
            .map(|b| b.port)
            .collect();
        for port in group {
            if let Some(bay) = self.bay_mut(port) {
                bay.set_volume_status(volume, ev);
            }
        }
    }

    pub(crate) fn set_dolby_settings(&mut self, settings: AmpDolbySettings, ev: &mut Vec<Event>) {
        if self.dolby_settings == Some(settings) {
            return;
        }
        self.dolby_settings = Some(settings);
        ev.push(Event::AmpDolbySettingsChanged {
            device: self.uid,
            settings,
        });
    }

    pub(crate) fn set_setup_completed(&mut self, completed: bool, ev: &mut Vec<Event>) {
        if self.setup_done == Some(completed) {
            return;
        }
        self.setup_done = Some(completed);
        ev.push(Event::SetupStatusChanged {
            device: self.uid,
            completed,
        });
    }

    pub(crate) fn set_installer_id(&mut self, installer_id: u16, ev: &mut Vec<Event>) {
        if self.installer_id == Some(installer_id) {
            return;
        }
        self.installer_id = Some(installer_id);
        ev.push(Event::InstallerIdChanged {
            device: self.uid,
            installer_id,
        });
    }

    pub(crate) fn set_tiling(&mut self, tiling: V2ipTilingConfig, ev: &mut Vec<Event>) {
        if self.tiling == Some(tiling) {
            return;
        }
        self.tiling = Some(tiling);
        ev.push(Event::TilingChanged {
            device: self.uid,
            tiling,
        });
    }

    pub(crate) fn set_rc_settings(&mut self, settings: RcSettings, ev: &mut Vec<Event>) {
        if self.rc_settings.as_ref() == Some(&settings) {
            return;
        }
        self.rc_settings = Some(settings.clone());
        ev.push(Event::RcSettingsChanged {
            device: self.uid,
            settings,
        });
    }

    pub(crate) fn set_audio_select_input(
        &mut self,
        change: AudioChangeSource,
        ev: &mut Vec<Event>,
    ) {
        if self.audio_select == Some(change) {
            return;
        }
        self.audio_select = Some(change);
        ev.push(Event::AudioSelectInput {
            device: self.uid,
            change,
        });
    }

    pub(crate) fn set_mesh_master(&mut self, master: DeviceUid, ev: &mut Vec<Event>) {
        if self.mesh_master == master {
            return;
        }
        self.mesh_master = master;
        ev.push(Event::MeshMasterChanged {
            device: self.uid,
            master,
        });
    }

    pub(crate) fn set_topology(&mut self, topology: Vec<TopologyEntry>, ev: &mut Vec<Event>) {
        if self.topology == topology {
            return;
        }
        self.topology.clone_from(&topology);
        ev.push(Event::TopologyChanged {
            device: self.uid,
            topology,
        });
    }

    /// Merges one frame of a device's source list into the list it belongs to.
    ///
    /// A list that fits one frame is the whole list and replaces what was held.
    /// A longer one arrives as pages, and a page is a window rather than the
    /// list: its records belong at `first + index` whatever order the pages
    /// arrive in, and it says nothing about the records it leaves out. Only a
    /// frame covering the whole list may shorten it.
    ///
    /// `total` is the sender's count at the moment that page was built rather
    /// than a promise about the set, so it decides only whether this frame is
    /// the whole list - it never sizes the result.
    pub(crate) fn merge_v2ip_sources(
        &mut self,
        first: usize,
        total: usize,
        page: &[V2ipStreamSources],
        ev: &mut Vec<Event>,
    ) {
        let whole = first == 0 && page.len() == total;
        if whole {
            self.v2ip_source_pages.clear();
        }
        for (index, source) in page.iter().enumerate() {
            let Ok(at) = u16::try_from(first + index) else {
                return;
            };
            self.v2ip_source_pages.insert(at, *source);
        }
        if whole {
            self.set_v2ip_sources(page.to_vec(), ev);
            return;
        }
        // A record's position is what maps it to a bay, so the list reported is
        // the run that has arrived from the start. A gap is where it stops,
        // never something to fill: a default in the middle would report a bay
        // as advertising no streams, which is a reading rather than an absence.
        let mut list = Vec::with_capacity(self.v2ip_source_pages.len());
        for (at, source) in &self.v2ip_source_pages {
            if usize::from(*at) != list.len() {
                break;
            }
            list.push(*source);
        }
        if list.is_empty() {
            return;
        }
        self.set_v2ip_sources(list, ev);
    }

    pub(crate) fn set_v2ip_sources(
        &mut self,
        sources: Vec<V2ipStreamSources>,
        ev: &mut Vec<Event>,
    ) {
        if self.v2ip_sources.as_ref() == Some(&sources) {
            return;
        }
        self.v2ip_sources = Some(sources.clone());
        ev.push(Event::V2ipSourcesChanged {
            device: self.uid,
            sources,
        });
    }

    /// Merges an encoder configuration report, which carries only the fields
    /// the sender had values for.
    /// Folds a received configuration onto the cached one.
    ///
    /// `whole_scaling` says the frame's scaling block is the sink's whole
    /// state rather than a write of one half of it, so a mode it does not
    /// carry is a mode the sink no longer has.
    pub(crate) fn set_v2ip_details(
        &mut self,
        details: DeviceV2ipDetails,
        whole_scaling: bool,
        ev: &mut Vec<Event>,
    ) {
        let mut merged = details.merge(self.v2ip_details);
        if whole_scaling && details.scaling.flags & SCALING_FLAG_MODE_VALID == 0 {
            merged.scaling.mode = details.scaling.mode;
            merged.scaling.refresh = details.scaling.refresh;
            merged.scaling.flags &= !SCALING_FLAG_MODE_VALID;
        }
        if self.v2ip_details == Some(merged) {
            return;
        }
        self.v2ip_details = Some(merged);
        ev.push(Event::V2ipDetailsChanged {
            device: self.uid,
            details: merged,
        });
    }

    /// The scaling block as last reported or written, all-zero before either.
    pub(crate) fn v2ip_scaling(&self) -> V2ipScalingSettings {
        self.v2ip_details.unwrap_or_default().scaling
    }

    /// Replaces the cached scaling block with the state a write will have left
    /// on the device.
    ///
    /// Separate from [`Device::set_v2ip_details`] because that folds a received
    /// frame on, and folding cannot express a cleared mode: a write clears one
    /// by sending the valid bit over a zero mode, while a device with no mode
    /// configured reports the valid bit clear. Only the second is a state a
    /// device broadcasts, so it is the one to cache.
    pub(crate) fn set_v2ip_scaling(&mut self, scaling: V2ipScalingSettings, ev: &mut Vec<Event>) {
        let mut details = self.v2ip_details.unwrap_or_default();
        if details.scaling == scaling {
            return;
        }
        details.scaling = scaling;
        self.v2ip_details = Some(details);
        ev.push(Event::V2ipDetailsChanged {
            device: self.uid,
            details,
        });
    }

    /// Records the processor's feature mask, which only ever gains bits.
    ///
    /// The caller passes a non-empty mask from a frame the device sent about
    /// itself; every other frame says nothing about the subject's processor and
    /// must leave what is cached alone.
    pub(crate) fn set_v2ip_features(&mut self, features: V2ipFpgaFeature, ev: &mut Vec<Event>) {
        if self.v2ip_features == Some(features) {
            return;
        }
        self.v2ip_features = Some(features);
        ev.push(Event::V2ipFeaturesChanged {
            device: self.uid,
            features,
        });
    }

    /// Folds a settings block onto the cached one.
    ///
    /// The caller has already limited a write about this device to what the
    /// device takes from one; a block that carries no setting leaves the cache
    /// as it was.
    pub(crate) fn set_time_zone(&mut self, time_zone: TimeZone, ev: &mut Vec<Event>) {
        if self.time_zone.as_ref() == Some(&time_zone) {
            return;
        }
        self.time_zone = Some(time_zone.clone());
        ev.push(Event::TimeZoneChanged {
            device: self.uid,
            time_zone,
        });
    }

    pub(crate) fn merge_v2ip_settings(&mut self, frame: V2ipDeviceSettings, ev: &mut Vec<Event>) {
        if frame.valid.is_empty() {
            return;
        }
        let merged = frame.merge(self.v2ip_settings.unwrap_or_default());
        if self.v2ip_settings == Some(merged) {
            return;
        }
        self.v2ip_settings = Some(merged);
        ev.push(Event::V2ipDeviceSettingsChanged {
            device: self.uid,
            settings: merged,
        });
    }

    pub(crate) fn set_v2ip_vlan(&mut self, vlan: V2ipVlan, ev: &mut Vec<Event>) {
        if self.v2ip_vlan == Some(vlan) {
            return;
        }
        self.v2ip_vlan = Some(vlan);
        ev.push(Event::V2ipVlanChanged {
            device: self.uid,
            vlan,
        });
    }

    pub(crate) fn set_unit_status(&mut self, status: UnitStatus, ev: &mut Vec<Event>) {
        if self.unit_status.as_ref() == Some(&status) {
            return;
        }
        self.unit_status = Some(status.clone());
        ev.push(Event::UnitStatusChanged {
            device: self.uid,
            status,
        });
    }

    pub(crate) fn set_osd_sync(&mut self, sync: OsdSync, ev: &mut Vec<Event>) {
        if self.osd_sync.as_ref() == Some(&sync) {
            return;
        }
        self.osd_sync = Some(sync.clone());
        ev.push(Event::OsdSyncChanged {
            device: self.uid,
            sync,
        });
    }

    pub(crate) fn set_osd_message(&mut self, message: OsdMessage, ev: &mut Vec<Event>) {
        if self.osd_message.as_ref() == Some(&message) {
            return;
        }
        self.osd_message = Some(message.clone());
        ev.push(Event::OsdMessageChanged {
            device: self.uid,
            message,
        });
    }

    pub(crate) fn set_osd_sync_report(&mut self, report: OsdSyncReport, ev: &mut Vec<Event>) {
        if self.osd_sync_report.as_ref() == Some(&report) {
            return;
        }
        self.osd_sync_report = Some(report.clone());
        ev.push(Event::OsdSyncReportChanged {
            device: self.uid,
            report,
        });
    }

    pub(crate) fn set_v2ip_sink_sdp(&mut self, sdp: V2ipSinkSdp, ev: &mut Vec<Event>) {
        if self.v2ip_sink_sdp.as_ref() == Some(&sdp) {
            return;
        }
        self.v2ip_sink_sdp = Some(sdp.clone());
        ev.push(Event::V2ipSinkSdpChanged {
            device: self.uid,
            sdp,
        });
    }

    pub(crate) fn set_v2ip_sink_description(
        &mut self,
        description: Option<String>,
        ev: &mut Vec<Event>,
    ) {
        if self.v2ip_sink_description == description {
            return;
        }
        self.v2ip_sink_description.clone_from(&description);
        ev.push(Event::V2ipSinkDescriptionChanged {
            device: self.uid,
            description,
        });
    }

    pub(crate) fn set_v2ip_privacy(&mut self, privacy: V2ipPrivacy, ev: &mut Vec<Event>) {
        if self.v2ip_privacy == Some(privacy) {
            return;
        }
        self.v2ip_privacy = Some(privacy);
        ev.push(Event::V2ipPrivacyChanged {
            device: self.uid,
            privacy,
        });
    }

    pub(crate) fn set_v2ip_ptp(&mut self, ptp: Option<V2ipPtpState>, ev: &mut Vec<Event>) {
        if self.v2ip_ptp == ptp {
            return;
        }
        self.v2ip_ptp = ptp;
        ev.push(Event::V2ipPtpChanged {
            device: self.uid,
            ptp,
        });
    }

    pub(crate) fn set_v2ip_downmix(&mut self, downmix: Option<V2ipDownmix>, ev: &mut Vec<Event>) {
        if self.v2ip_downmix == downmix {
            return;
        }
        self.v2ip_downmix = downmix;
        ev.push(Event::V2ipDownmixChanged {
            device: self.uid,
            downmix,
        });
    }

    pub(crate) fn set_v2ip_testcard(&mut self, testcard: V2ipTestcard, ev: &mut Vec<Event>) {
        if self.v2ip_testcard == Some(testcard) {
            return;
        }
        self.v2ip_testcard = Some(testcard);
        ev.push(Event::V2ipTestcardChanged {
            device: self.uid,
            testcard,
        });
    }

    pub(crate) fn set_v2ip_sink(&mut self, sink: DeviceV2ipSink, ev: &mut Vec<Event>) {
        if self.v2ip_sink == Some(sink) {
            return;
        }
        self.v2ip_sink = Some(sink);
        ev.push(Event::V2ipSinkChanged {
            device: self.uid,
            sink,
        });
    }

    pub(crate) fn set_multiviewer_status(
        &mut self,
        status: MultiviewerStatus,
        ev: &mut Vec<Event>,
    ) {
        if self.multiviewer.as_ref() == Some(&status) {
            return;
        }
        self.multiviewer = Some(status.clone());
        ev.push(Event::MultiviewerStatusChanged {
            device: self.uid,
            status,
        });
    }

    /// Replaces the audio endpoint tree and reattaches the bays that carry it.
    ///
    /// A device re-sends the tree whenever any routing within it changes, so
    /// only a change to the tree's own shape is announced. The attachments are
    /// redone either way, because a bay discovered after the tree first
    /// arrived has nothing to attach it to until the tree comes round again.
    pub(crate) fn set_audio_endpoints(&mut self, endpoints: AudioEndpoints, ev: &mut Vec<Event>) {
        let same = self.audio.as_ref().is_some_and(|current| {
            current.same_tree(&endpoints) && current.same_status(&endpoints)
        });
        self.audio = Some(endpoints);
        self.attach_audio_endpoints(ev);
        if same {
            return;
        }
        if let Some(endpoints) = self.audio.clone() {
            ev.push(Event::AudioEndpointsChanged {
                device: self.uid,
                endpoints,
            });
        }
    }

    /// Attaches each audio endpoint to the bay that carries it.
    ///
    /// A OneIP unit has one input and one output, and its tree crosses them:
    /// the local input feeds the endpoint that leaves the box, so the input
    /// bay carries the tree's first output and the output bay its first input.
    /// An amplifier instead numbers its endpoints, inputs below ten and
    /// outputs from ten.
    fn attach_audio_endpoints(&mut self, ev: &mut Vec<Event>) {
        let Some(endpoints) = self.audio.clone() else {
            return;
        };
        let mut pairs: Vec<(u16, u8)> = Vec::new();
        if self.is_v2ip() && self.has_local_source() {
            let mut pair = |bay: Option<&Bay>, endpoint: Option<&crate::types::AudioEndpoint>| {
                if let (Some(bay), Some(endpoint)) = (bay, endpoint) {
                    pairs.push((bay.port, endpoint.id));
                }
            };
            pair(self.first_input(), endpoints.first_root_output());
            pair(self.first_output(), endpoints.first_root_input());
        } else if self.is_amp() {
            for ep in endpoints.list() {
                let (mode, number) = if ep.id < 10 {
                    ("Input", ep.id)
                } else {
                    ("Output", ep.id - 10)
                };
                if let Some(bay) = self.bay_by_mode_num(mode, number) {
                    pairs.push((bay.port, ep.id));
                }
            }
        }
        for (port, endpoint) in pairs {
            if let Some(bay) = self.bays.get_mut(&port) {
                bay.set_audio_endpoint(endpoint, ev);
            }
        }
    }

    pub(crate) fn apply_audio_links(&mut self, links: &[AudioLink]) {
        let Some(audio) = self.audio.as_mut() else {
            return;
        };
        for link in links {
            audio.apply_link(link);
        }
    }
}

/// The protocol from which a OneIP unit numbers its ports
/// [`PortLayout::Fixed`].
const FIXED_PORTS_SINCE: u16 = 0x2B;

/// The bays a OneIP unit numbers in each direction.
const V2IP_BAYS: u8 = 127;

/// The bays of each direction that come first in [`PortLayout::Split`].
const SPLIT_FIRST: u8 = 16;

/// How a OneIP unit numbers the ports of its bays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortLayout {
    /// Input n on port n, output n on port 127 + n.
    Fixed,
    /// Inputs 0-15 on ports 0-15 and outputs 0-15 on 16-31, then inputs
    /// 16-126 on 32-142 and outputs 16-126 on 143-253.
    Split,
}

impl PortLayout {
    /// The direction and number of the bay on `port`. A port past the last
    /// output reads as an output numbered past the last bay, which
    /// [`PortLayout::encode`] gives no port.
    fn decode(self, port: u16) -> Option<(bool, u8)> {
        let (is_input, first_port) = match (self, port) {
            (Self::Fixed, 0..=126) | (Self::Split, 0..=15) => (true, 0),
            (Self::Split, 16..=31) => (false, 16),
            (Self::Split, 32..=142) => (true, 16),
            (Self::Fixed | Self::Split, _) => (false, 127),
        };
        let bay = u8::try_from(port - first_port).ok()?;
        Some((is_input, bay))
    }

    /// The port of input or output `bay`, `None` past the last bay.
    fn encode(self, is_input: bool, bay: u8) -> Option<u16> {
        if bay >= V2IP_BAYS {
            return None;
        }
        let bay = u16::from(bay);
        let split = u16::from(SPLIT_FIRST);
        Some(match (self, is_input) {
            (Self::Fixed, true) => bay,
            (Self::Fixed, false) => bay + 127,
            (Self::Split, true) if bay < split => bay,
            (Self::Split, false) if bay < split => bay + split,
            (Self::Split, true) => bay + split,
            (Self::Split, false) => bay + 127,
        })
    }
}

/// What a bay mapping page did to a mesh bay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MeshBayChange {
    /// The bay is new.
    Registered,
    /// The bay now stands for another unit.
    Remapped,
}
