// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! A single input or output port on a device.

use crate::event::Event;
use crate::types::{
    AmpZoneSettings, ArcStatus, BayMirrorStatus, BaySignalDetails, ConnectStatus, HiddenStatus,
    PowerStatus, VolumeMuteStatus,
};
use crate::wire::{
    BayConfig, BayFeatures, BayStatus, BayUid, DeviceUid, EdidProfile, MxrSignalType, RcType,
};

/// One port on a device.
///
/// A bay names other bays by [`BayUid`] rather than holding a reference to
/// them: a routed source is often a bay on a different device, and the sink
/// learns of it before that device has been discovered.
#[derive(Clone, Debug)]
pub(crate) struct Bay {
    pub(crate) device: DeviceUid,
    pub(crate) port: u16,
    pub(crate) port_name: String,
    pub(crate) user_name: Option<String>,
    pub(crate) features: BayFeatures,
    pub(crate) status_mask: BayStatus,
    /// The bay number the device's own API and topology use, once a bay config
    /// has reported it. Until then [`Bay::bay_num`] reads it off the port name.
    pub(crate) mbay_id: Option<u8>,

    pub(crate) video_source: Option<BayUid>,
    pub(crate) audio_source: Option<BayUid>,

    pub(crate) power_status: Option<PowerStatus>,
    pub(crate) faulty: Option<bool>,
    pub(crate) hidden: Option<bool>,
    pub(crate) poe_powered: Option<bool>,
    pub(crate) hdbt_connected: Option<bool>,
    pub(crate) signal_detected: Option<bool>,
    pub(crate) hpd_detected: Option<bool>,
    pub(crate) cec_detected: Option<bool>,
    pub(crate) decoder_disabled: Option<bool>,
    pub(crate) encoder_disabled: Option<bool>,
    pub(crate) signal_type: Option<String>,
    pub(crate) signal_details: Option<BaySignalDetails>,
    pub(crate) signal_mode: MxrSignalType,
    pub(crate) filtered: Vec<DeviceUid>,
    pub(crate) arc: ArcStatus,
    pub(crate) audio_volume: Option<VolumeMuteStatus>,
    pub(crate) rc_type: Option<RcType>,
    pub(crate) edid_profile: Option<EdidProfile>,
    /// The source device this V2IP bay maps to, from the bay-mapping report.
    pub(crate) v2ip_uid: DeviceUid,
    pub(crate) mirror: BayMirrorStatus,
    pub(crate) audio_endpoint: Option<u8>,
    pub(crate) amp_settings: Option<AmpZoneSettings>,
}

impl Bay {
    pub(crate) fn new(device: DeviceUid, cfg: &BayConfig) -> Self {
        Self {
            device,
            port: u16::from(cfg.port),
            port_name: cfg.bay_name.clone(),
            user_name: None,
            features: cfg.features,
            status_mask: cfg.status,
            mbay_id: None,
            video_source: None,
            audio_source: None,
            power_status: None,
            faulty: None,
            hidden: None,
            poe_powered: None,
            hdbt_connected: None,
            signal_detected: None,
            hpd_detected: None,
            cec_detected: None,
            decoder_disabled: None,
            encoder_disabled: None,
            signal_type: None,
            signal_details: None,
            signal_mode: MxrSignalType::NONE,
            filtered: Vec::new(),
            arc: ArcStatus::Inactive,
            audio_volume: None,
            rc_type: None,
            edid_profile: None,
            v2ip_uid: DeviceUid::ZERO,
            mirror: BayMirrorStatus::default(),
            audio_endpoint: None,
            amp_settings: None,
        }
    }

    /// How this bay is addressed: its device and its port number.
    ///
    /// This is the identity every command and event uses. [`Bay::link_key`] is
    /// the other one, and the two differ for a V2IP source.
    pub(crate) fn uid(&self) -> BayUid {
        BayUid::new(self.device, self.port)
    }

    // ---- static properties ----

    pub(crate) fn is_input(&self) -> bool {
        self.features.has(BayFeatures::HDMI_IN)
            || self.features.has(BayFeatures::AUDIO_DIG_IN)
            || self.features.has(BayFeatures::AUDIO_ANA_IN)
            || self.is_v2ip_source()
    }

    pub(crate) fn is_output(&self) -> bool {
        self.features.has(BayFeatures::HDMI_OUT)
            || self.features.has(BayFeatures::AUDIO_AMP_OUT)
            || self.features.has(BayFeatures::AUDIO_DIG_OUT)
            || self.features.has(BayFeatures::AUDIO_ANA_OUT)
            || self.is_v2ip_sink()
    }

    pub(crate) fn is_hdmi(&self) -> bool {
        self.features.has(BayFeatures::HDMI_IN) || self.features.has(BayFeatures::HDMI_OUT)
    }

    pub(crate) fn is_audio(&self) -> bool {
        !self.is_hdmi()
            && (self.features.has(BayFeatures::AUDIO_AMP_OUT)
                || self.features.has(BayFeatures::AUDIO_ANA_IN)
                || self.features.has(BayFeatures::AUDIO_ANA_OUT)
                || self.features.has(BayFeatures::AUDIO_DIG_IN)
                || self.features.has(BayFeatures::AUDIO_DIG_OUT))
    }

    pub(crate) fn is_v2ip_source(&self) -> bool {
        self.features.has(BayFeatures::V2IP_SOURCE_LOCAL)
            || self.features.has(BayFeatures::V2IP_SOURCE_REMOTE)
    }

    pub(crate) fn is_v2ip_sink(&self) -> bool {
        self.features.has(BayFeatures::V2IP_SINK_LOCAL)
            || self.features.has(BayFeatures::V2IP_SINK_REMOTE)
    }

    /// Whether this bay lives on another device and reaches us through the
    /// mesh.
    pub(crate) fn is_v2ip_remote(&self) -> bool {
        self.features.has(BayFeatures::V2IP_SINK_REMOTE)
            || self.features.has(BayFeatures::V2IP_SOURCE_REMOTE)
    }

    pub(crate) fn is_local(&self) -> bool {
        !self.is_v2ip_remote()
    }

    pub(crate) fn has_volume_control(&self) -> bool {
        self.features.has(BayFeatures::AUDIO_ANA_OUT)
            || self.features.has(BayFeatures::AUDIO_AMP_OUT)
            || self.features.has(BayFeatures::AUDIO_ANA_IN)
            || self.features.has(BayFeatures::AUDIO_DIG_IN)
    }

    pub(crate) fn has_dolby(&self) -> bool {
        self.features.has(BayFeatures::DOLBY)
    }

    /// The word the device uses for this bay's direction in the names it
    /// reports and in its bay-mapping frames.
    pub(crate) fn mode_str(&self) -> &'static str {
        if self.is_output() {
            "Output"
        } else if self.is_input() {
            "Input"
        } else {
            "unknown"
        }
    }

    /// The bay number the device's own API and topology use.
    ///
    /// A bay config reports it directly. Before one arrives it comes off the
    /// port name, which the device formats as its direction and this number.
    pub(crate) fn bay_num(&self) -> u8 {
        if let Some(id) = self.mbay_id {
            return id;
        }
        let Some(tail) = self.port_name.strip_prefix(self.mode_str()) else {
            return 0;
        };
        let digits: String = tail
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().unwrap_or(0)
    }

    pub(crate) fn user_name(&self) -> &str {
        self.user_name.as_deref().unwrap_or(&self.port_name)
    }

    /// The routed audio source, which follows the video source until the bay
    /// is told otherwise.
    pub(crate) fn effective_audio_source(&self) -> Option<BayUid> {
        self.audio_source.or(self.video_source)
    }

    // ---- mutators ----

    pub(crate) fn set_user_name(&mut self, value: String, ev: &mut Vec<Event>) {
        if self.user_name() == value {
            return;
        }
        self.user_name = Some(value.clone());
        ev.push(Event::NameChanged {
            bay: self.uid(),
            name: value,
        });
    }

    pub(crate) fn set_video_source(&mut self, source: Option<BayUid>, ev: &mut Vec<Event>) {
        if !self.is_output() {
            return;
        }
        let Some(source) = source else {
            self.video_source = None;
            return;
        };
        if self.video_source == Some(source) {
            return;
        }
        self.video_source = Some(source);
        ev.push(Event::VideoSourceChanged {
            bay: self.uid(),
            source: Some(source),
        });
    }

    pub(crate) fn set_audio_source(&mut self, source: Option<BayUid>, ev: &mut Vec<Event>) {
        if !self.is_output() {
            return;
        }
        let Some(source) = source else {
            self.audio_source = None;
            return;
        };
        let previous = self.effective_audio_source();
        self.audio_source = Some(source);
        let current = self.effective_audio_source();
        if previous != current {
            ev.push(Event::AudioSourceChanged {
                bay: self.uid(),
                source: current,
            });
        }
    }

    pub(crate) fn set_signal_type(&mut self, value: String, ev: &mut Vec<Event>) {
        if self.signal_type.as_deref() == Some(value.as_str()) {
            return;
        }
        self.signal_type = Some(value.clone());
        ev.push(Event::SignalTypeChanged {
            bay: self.uid(),
            signal_type: value,
        });
    }

    pub(crate) fn set_signal_details(&mut self, details: BaySignalDetails) {
        self.signal_details = Some(details);
    }

    pub(crate) fn set_filtered(&mut self, filtered: Vec<DeviceUid>, ev: &mut Vec<Event>) {
        if self.filtered == filtered {
            return;
        }
        self.filtered.clone_from(&filtered);
        ev.push(Event::FilteredDevicesChanged {
            bay: self.uid(),
            filtered,
        });
    }

    pub(crate) fn set_arc(&mut self, arc: ArcStatus, ev: &mut Vec<Event>) {
        if self.arc == arc {
            return;
        }
        self.arc = arc;
        ev.push(Event::ArcChanged {
            bay: self.uid(),
            arc,
        });
    }

    pub(crate) fn set_power_status(&mut self, power: PowerStatus, ev: &mut Vec<Event>) {
        if self.power_status == Some(power) {
            return;
        }
        self.power_status = Some(power);
        ev.push(Event::PowerChanged {
            bay: self.uid(),
            power,
        });
    }

    pub(crate) fn set_edid_profile(&mut self, profile: EdidProfile, ev: &mut Vec<Event>) {
        if !self.is_hdmi() || !self.is_input() || self.edid_profile == Some(profile) {
            return;
        }
        self.edid_profile = Some(profile);
        ev.push(Event::EdidProfileChanged {
            bay: self.uid(),
            profile,
        });
    }

    pub(crate) fn set_rc_type(&mut self, rc_type: RcType, ev: &mut Vec<Event>) {
        if !self.is_hdmi() || !self.is_input() || self.rc_type == Some(rc_type) {
            return;
        }
        self.rc_type = Some(rc_type);
        ev.push(Event::RcTypeChanged {
            bay: self.uid(),
            rc_type,
        });
    }

    pub(crate) fn set_volume_status(&mut self, other: VolumeMuteStatus, ev: &mut Vec<Event>) {
        if !self.has_volume_control() {
            return;
        }
        let changed = match self.audio_volume.as_mut() {
            None => {
                self.audio_volume = Some(other);
                true
            }
            Some(current) => merge_volume(current, other),
        };
        if let (true, Some(volume)) = (changed, self.audio_volume) {
            ev.push(Event::VolumeChanged {
                bay: self.uid(),
                volume,
            });
        }
    }

    pub(crate) fn set_mirroring(&mut self, mirror: BayMirrorStatus, ev: &mut Vec<Event>) {
        if self.mirror == mirror {
            return;
        }
        self.mirror = mirror;
        ev.push(Event::MirrorStatusChanged {
            bay: self.uid(),
            mirror,
        });
    }

    pub(crate) fn set_amp_settings(&mut self, settings: AmpZoneSettings, ev: &mut Vec<Event>) {
        if self.amp_settings.as_ref() == Some(&settings) {
            return;
        }
        self.amp_settings = Some(settings);
        ev.push(Event::AmpZoneSettingsChanged {
            bay: self.uid(),
            settings,
        });
    }

    pub(crate) fn set_audio_endpoint(&mut self, endpoint: u8, ev: &mut Vec<Event>) {
        if self.audio_endpoint == Some(endpoint) {
            return;
        }
        self.audio_endpoint = Some(endpoint);
        ev.push(Event::AudioEndpointChanged {
            bay: self.uid(),
            endpoint,
        });
    }

    /// Applies a `DEV_CONNECT` report, which means "a signal arrived" on an
    /// input and "a sink answered" on an output.
    pub(crate) fn apply_connect_status(&mut self, status: ConnectStatus, ev: &mut Vec<Event>) {
        let connected = status == ConnectStatus::Connected;
        if self.is_input() {
            if set_bool(&mut self.signal_detected, connected) {
                ev.push(Event::SignalDetectedChanged {
                    bay: self.uid(),
                    detected: connected,
                });
            }
        } else if set_bool(&mut self.hpd_detected, connected) {
            ev.push(Event::HpdDetectedChanged {
                bay: self.uid(),
                detected: connected,
            });
        }
    }

    pub(crate) fn apply_hidden(&mut self, hidden: HiddenStatus, ev: &mut Vec<Event>) {
        if hidden == HiddenStatus::Unknown {
            return;
        }
        let value = hidden == HiddenStatus::Hidden;
        if set_bool(&mut self.hidden, value) {
            ev.push(Event::HiddenChanged {
                bay: self.uid(),
                hidden: value,
            });
        }
    }

    pub(crate) fn apply_signal_status(
        &mut self,
        detected: bool,
        description: Option<String>,
        ev: &mut Vec<Event>,
    ) {
        if set_bool(&mut self.signal_detected, detected) {
            ev.push(Event::SignalDetectedChanged {
                bay: self.uid(),
                detected,
            });
        }
        if let Some(description) = description {
            self.set_signal_type(description, ev);
        }
    }

    /// Applies a bay status word: the flag bits, then the power state and the
    /// audio return channel it encodes.
    pub(crate) fn apply_bay_status(&mut self, data: BayStatus, ev: &mut Vec<Event>) {
        let uid = self.uid();
        macro_rules! flag {
            ($field:ident, $bit:ident, $event:ident { $arg:ident }) => {
                let value = data.has(BayStatus::$bit);
                if set_bool(&mut self.$field, value) {
                    ev.push(Event::$event {
                        bay: uid,
                        $arg: value,
                    });
                }
            };
        }
        flag!(faulty, FAULT, FaultyChanged { faulty });
        flag!(hidden, HIDDEN, HiddenChanged { hidden });
        flag!(poe_powered, POWERED, PoePoweredChanged { powered });
        flag!(
            hdbt_connected,
            HDBT_CONNECTED,
            HdbtConnectedChanged { connected }
        );
        flag!(hpd_detected, HPD_DETECTED, HpdDetectedChanged { detected });
        flag!(cec_detected, CEC_DETECTED, CecDetectedChanged { detected });
        flag!(
            signal_detected,
            SIGNAL_DETECTED,
            SignalDetectedChanged { detected }
        );
        flag!(
            encoder_disabled,
            ENCODER_DISABLE,
            EncoderDisabledChanged { disabled }
        );
        flag!(
            decoder_disabled,
            DECODER_DISABLE,
            DecoderDisabledChanged { disabled }
        );

        // Power is reported by a device answering CEC. Without an answer there
        // is nothing to report, whatever the two power bits say.
        let power = if !data.has(BayStatus::CEC_DETECTED) {
            PowerStatus::Unknown
        } else if data.has(BayStatus::POWERED_ON) {
            PowerStatus::On
        } else if data.has(BayStatus::POWERED_OFF) {
            PowerStatus::Off
        } else {
            PowerStatus::Unknown
        };
        self.set_power_status(power, ev);

        let arc = if data.has(BayStatus::AUDIO_ARC_HDMI) {
            ArcStatus::Hdmi
        } else if data.has(BayStatus::AUDIO_ARC_OPTIC) {
            ArcStatus::Optical
        } else if data.has(BayStatus::AUDIO_ARC_ANALOG) {
            ArcStatus::Analog
        } else {
            ArcStatus::Inactive
        };
        self.set_arc(arc, ev);
    }
}

/// Records a boolean status, reporting whether it changed.
///
/// A field that has never been reported counts as false, so the first report
/// of a raised flag is a change and the first report of a cleared one is not.
fn set_bool(slot: &mut Option<bool>, value: bool) -> bool {
    let changed = slot.unwrap_or(false) != value;
    *slot = Some(value);
    changed
}

/// Merges a volume report into the state, reporting whether anything changed.
///
/// Each field is merged only when the report carries it: a device that reports
/// only a mute state must not clear the volume beside it.
fn merge_volume(current: &mut VolumeMuteStatus, other: VolumeMuteStatus) -> bool {
    let mut changed = false;
    if other.volume_left.is_some() && current.volume_left != other.volume_left {
        current.volume_left = other.volume_left;
        changed = true;
    }
    if other.volume_right.is_some() && current.volume_right != other.volume_right {
        current.volume_right = other.volume_right;
        changed = true;
    }
    if other.muted_left.is_some() && current.muted_left != other.muted_left {
        current.muted_left = other.muted_left;
        changed = true;
    }
    if other.muted_right.is_some() && current.muted_right != other.muted_right {
        current.muted_right = other.muted_right;
        changed = true;
    }
    changed
}
