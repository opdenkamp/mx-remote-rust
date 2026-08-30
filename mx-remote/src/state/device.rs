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
    PduState, RcSettings, TopologyEntry, V2ipDeviceStats, V2ipStreamSources, V2ipTilingConfig,
    VolumeMuteStatus,
};
use crate::wire::{BayConfig, BayUid, DeviceFeature, DeviceUid, FirmwareType};

use super::bay::Bay;

/// How long a device may stay silent before it counts as offline.
///
/// Protocol 0x20 brought a device announcement every few seconds, so silence
/// becomes meaningful far sooner than it does on the older cadence.
const SILENCE_LIMIT: Duration = Duration::from_secs(120);
const SILENCE_LIMIT_MODERN: Duration = Duration::from_secs(15);
const MODERN_PROTOCOL: u16 = 0x20;

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
    pub(crate) temperatures: Vec<u8>,
    pub(crate) online: bool,
    pub(crate) have_config: bool,
    pub(crate) rebooting: bool,
    pub(crate) last_ping: Instant,
    pub(crate) hello_received: Instant,
    pub(crate) link_config_received: bool,

    pub(crate) v2ip_sources: Option<Vec<V2ipStreamSources>>,
    pub(crate) v2ip_details: Option<DeviceV2ipDetails>,
    pub(crate) v2ip_sink: Option<DeviceV2ipSink>,
    pub(crate) v2ip_stats: Option<V2ipDeviceStats>,
    pub(crate) pdu_state: Option<PduState>,
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
    pub(crate) multiviewer: Option<MultiviewerStatus>,
    pub(crate) dolby_settings: Option<AmpDolbySettings>,
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
            hello_received: now,
            link_config_received: false,
            v2ip_sources: None,
            v2ip_details: None,
            v2ip_sink: None,
            v2ip_stats: None,
            pdu_state: None,
            setup_done: None,
            installer_id: None,
            tiling: None,
            rc_settings: None,
            audio_select: None,
            firmware: BTreeMap::new(),
            mesh_master: DeviceUid::ZERO,
            network: BTreeMap::new(),
            sys_status: None,
            topology: Vec::new(),
            audio: None,
            multiviewer: None,
            dolby_settings: None,
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
    /// addresses in the V2IP sources frame; and the padding beside the
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
        let limit = if self.hello.supported_protocol >= MODERN_PROTOCOL {
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

    pub(crate) fn bay_mut(&mut self, port: u16) -> Option<&mut Bay> {
        self.bays.get_mut(&port)
    }

    pub(crate) fn bay_by_name(&self, name: &str) -> Option<&Bay> {
        self.bays.values().find(|b| b.port_name == name)
    }

    /// The input bay carrying the given user-assigned name.
    ///
    /// Inputs only, and hidden ones are skipped: this resolves the name a
    /// picker shows, and a picker does not offer what it does not list.
    pub(crate) fn bay_by_user_name(&self, name: &str) -> Option<&Bay> {
        self.inputs().find(|b| b.user_name() == name)
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

    pub(crate) fn first_output_port(&self) -> Option<u16> {
        self.first_output().map(|b| b.port)
    }

    pub(crate) fn inputs(&self) -> impl Iterator<Item = &Bay> {
        self.bays
            .values()
            .filter(|b| b.is_input() && b.hidden != Some(true))
    }

    pub(crate) fn outputs(&self) -> impl Iterator<Item = &Bay> {
        self.bays.values().filter(|b| b.is_output())
    }

    /// The streams the given source bay advertises.
    ///
    /// The V2IP sources frame lists one record per source bay in bay order. A
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
    /// A V2IP source is the same physical input wherever it appears, so it is
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

    fn has_bays(&self) -> bool {
        self.bays.len() >= self.inputs().count() + self.outputs().count()
    }

    fn needs_link_config(&self) -> bool {
        (self.is_amp() || self.is_video_matrix() || self.is_audio_matrix() || self.is_v2ip())
            && !self.link_config_received
    }

    pub(crate) fn configuration_complete(&self) -> bool {
        self.has_bays()
            && !(self.is_v2ip() && self.v2ip_sources.is_none())
            && !self.needs_link_config()
    }

    fn check_config_complete(&mut self, ev: &mut Vec<Event>) {
        if self.have_config || !self.configuration_complete() {
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
        self.hello_received = now;
        let changed = !self.hello.same_advertisement(&hello);
        self.hello = hello;
        self.rebooting = false;
        if changed {
            ev.push(Event::DeviceConfigChanged { device: self.uid });
        }
    }

    /// Merges one bay descriptor, resolving its routed source ports against
    /// the bays this device already has.
    pub(crate) fn apply_bay_config(&mut self, cfg: &BayConfig, now: Instant, ev: &mut Vec<Event>) {
        self.last_ping = now;
        let is_v2ip = self.is_v2ip();
        let video = self.routed_source(cfg.video_source);
        let audio = self.routed_source(cfg.audio_source);
        let is_new = !self.bays.contains_key(&u16::from(cfg.port));

        let bay = self
            .bays
            .entry(u16::from(cfg.port))
            .or_insert_with(|| Bay::new(self.uid, cfg));

        bay.features = cfg.features;
        bay.status_mask = cfg.status;
        bay.set_user_name(cfg.user_name.clone(), ev);
        if bay.mbay_id.is_none() {
            bay.mbay_id = Some(cfg.bay);
        }
        bay.apply_bay_status(cfg.status, ev);
        bay.signal_mode = cfg.signal_mode;
        // A V2IP source reporting a signal describes it in its own detailed
        // report, which carries the frame rate this field has no room for.
        if !cfg.status.has(crate::wire::BayStatus::SIGNAL_DETECTED) || !is_v2ip {
            bay.set_signal_type(cfg.signal_type.clone(), ev);
        }
        if bay.is_output() {
            bay.set_video_source(video, ev);
            bay.set_audio_source(audio, ev);
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
            self.check_config_complete(ev);
        }
    }

    /// The identity of a local bay named by port number in a routing report.
    fn routed_source(&self, port: u8) -> Option<BayUid> {
        self.bays.get(&u16::from(port)).map(Bay::uid)
    }

    pub(crate) fn on_link_config_received(&mut self, ev: &mut Vec<Event>) {
        self.link_config_received = true;
        self.check_config_complete(ev);
    }

    pub(crate) fn set_temperatures(&mut self, temperatures: Vec<u8>, ev: &mut Vec<Event>) {
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
            self.have_config = false;
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

    pub(crate) fn set_pdu_state(&mut self, state: PduState, ev: &mut Vec<Event>) {
        if self.pdu_state == Some(state) {
            return;
        }
        self.pdu_state = Some(state);
        ev.push(Event::PduStateChanged {
            device: self.uid,
            state,
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
    pub(crate) fn set_v2ip_details(&mut self, details: DeviceV2ipDetails, ev: &mut Vec<Event>) {
        let merged = details.merge(self.v2ip_details);
        if self.v2ip_details == Some(merged) {
            return;
        }
        self.v2ip_details = Some(merged);
        ev.push(Event::V2ipDetailsChanged {
            device: self.uid,
            details: merged,
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
        let same = self
            .audio
            .as_ref()
            .is_some_and(|current| current.same_tree(&endpoints));
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
