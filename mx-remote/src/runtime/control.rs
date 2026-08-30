// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The control surface: what a caller can ask a device to do.
//!
//! Every method here has the same shape. It reads the registry to decide what
//! to send, releases that lock, transmits, and only then writes back what the
//! device will have done. The order is what makes a handler woken by the
//! write-back free to call in again, and it keeps the receive thread from
//! waiting on a socket write for the lock it needs to decode.
//!
//! Nothing here reaches the wire on its own: a payload is bytes until the
//! single transmit path stamps and writes it, which is where the addressee's
//! protocol version is checked.

use std::fmt;
use std::net::Ipv4Addr;

use crate::event::Event;
use crate::state::{Bay, Device, State};
use crate::types::{
    AmpZoneSettings, HiddenStatus, PowerStatus, V2ipAudioFormat, V2ipStreamSources,
    VolumeMuteStatus,
};
use crate::wire::{
    audio_cmd_header, audio_param, audio_sub, build_amp_zone_settings, build_audio_select_input,
    build_bay_hide, build_edid_profile, build_rc_action, build_set_bay_name, build_set_volume,
    build_stats_request, build_target_only, build_v2ip_manual_source_switch,
    build_v2ip_source_switch, mv_cmd_payload, mv_sub, op, Addressee, BayUid, DeviceUid,
    EdidProfile, MultiviewerAspectRatio, MultiviewerEdidTemplate, MultiviewerHdcpMode,
    MultiviewerItcMode, MultiviewerOutputMode, MultiviewerPipPosition, MultiviewerPipSize,
    MultiviewerSource, MultiviewerViewMode, Opcode, RcAction, SendError, StreamAddr, V2ipStreams,
    DEVICE_NAME_LEN, V2IP_PORT_AUDIO,
};

use super::{Remote, Shared};

/// Why a control method did nothing.
#[derive(Debug)]
#[non_exhaustive]
pub enum ControlError {
    /// No device with this identifier has been heard from.
    UnknownDevice(DeviceUid),
    /// The device has reported no bay on this port.
    UnknownBay(BayUid),
    /// No input bay on the device carries this user-assigned name.
    UnknownSource(String),
    /// The addressee does not do what was asked of it.
    Unsupported(&'static str),
    /// The device has not reported something the request is assembled from.
    ///
    /// Unlike [`ControlError::Unsupported`], the same call may succeed once it
    /// has: this says the value is missing, not that it cannot exist.
    NotReported(&'static str),
    /// The frame could not be sent.
    Send(SendError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDevice(uid) => write!(f, "no device {uid}"),
            Self::UnknownBay(uid) => write!(f, "no bay {uid}"),
            Self::UnknownSource(name) => write!(f, "no source named {name:?}"),
            Self::Unsupported(what) => f.write_str(what),
            Self::NotReported(what) => write!(f, "{what} has not been reported"),
            Self::Send(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ControlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Send(e) => Some(e),
            _ => None,
        }
    }
}

impl From<SendError> for ControlError {
    fn from(e: SendError) -> Self {
        Self::Send(e)
    }
}

/// What a command does to this client's copy of the registry once its frame is
/// away.
///
/// A device does not acknowledge a command, so without this a caller that read
/// back what it just wrote would see the old value until some unrelated report
/// happened to carry the new one.
type WriteBack = Box<dyn FnOnce(&mut State, &mut Vec<Event>) + Send>;

/// One command: the frame to send, and what the addressee will do with it.
struct Command {
    to: Addressee,
    opcode: Opcode,
    payload: Vec<u8>,
    write_back: Option<WriteBack>,
}

impl Command {
    fn new(to: Addressee, opcode: Opcode, payload: Vec<u8>) -> Self {
        Self {
            to,
            opcode,
            payload,
            write_back: None,
        }
    }

    /// Records what to apply locally once the frame is away.
    fn then(mut self, f: impl FnOnce(&mut State, &mut Vec<Event>) + Send + 'static) -> Self {
        self.write_back = Some(Box::new(f));
        self
    }
}

impl Shared {
    /// Runs one command: prepare under the registry lock, send without it,
    /// then write back.
    fn command(
        &self,
        prepare: impl FnOnce(&State) -> Result<Command, ControlError>,
    ) -> Result<(), ControlError> {
        let command = self.read(prepare)?;
        self.send(&command.to, command.opcode, &command.payload)?;
        if let Some(write_back) = command.write_back {
            self.mutate(|state, ev| write_back(state, ev));
        }
        Ok(())
    }
}

fn device_of(state: &State, uid: DeviceUid) -> Result<&Device, ControlError> {
    state.device(uid).ok_or(ControlError::UnknownDevice(uid))
}

fn bay_of(state: &State, uid: BayUid) -> Result<(&Device, &Bay), ControlError> {
    let device = device_of(state, uid.device)?;
    let bay = device.bay(uid.port).ok_or(ControlError::UnknownBay(uid))?;
    Ok((device, bay))
}

/// The streams the source bay on `port` advertises.
fn source_streams(device: &Device, port: u16) -> Result<&V2ipStreamSources, ControlError> {
    let source = device
        .bay(port)
        .ok_or(ControlError::UnknownBay(BayUid::new(device.uid, port)))?;
    device
        .v2ip_source_for(source)
        .ok_or(ControlError::NotReported("the source's stream addresses"))
}

/// A sink bay, or the reason it cannot be routed.
fn v2ip_sink(state: &State, uid: BayUid) -> Result<(&Device, &Bay), ControlError> {
    let (device, bay) = bay_of(state, uid)?;
    if !bay.is_v2ip_sink() {
        return Err(ControlError::Unsupported("routing needs a V2IP sink"));
    }
    Ok((device, bay))
}

/// The name as the device will store it: the field is
/// [`DEVICE_NAME_LEN`] bytes wide, so a longer one is cut there.
fn stored_name(name: &str) -> String {
    let bytes = name.as_bytes();
    String::from_utf8_lossy(bytes.get(..DEVICE_NAME_LEN).unwrap_or(bytes)).into_owned()
}

impl Remote {
    // ---- routing ----

    /// Routes this V2IP sink's video to the stream a source port advertises.
    pub fn select_video_source(&self, sink: BayUid, source_port: u16) -> Result<(), ControlError> {
        self.shared.command(|state| {
            let (device, bay) = v2ip_sink(state, sink)?;
            if !bay.is_output() {
                return Err(ControlError::Unsupported("not an output bay"));
            }
            let streams = source_streams(device, source_port)?;
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_SOURCE_SWITCH,
                build_v2ip_source_switch(device.uid, streams.video.ip, Ipv4Addr::UNSPECIFIED),
            ))
        })
    }

    /// Routes this V2IP sink's audio to the stream a source port advertises.
    pub fn select_audio_source(&self, sink: BayUid, source_port: u16) -> Result<(), ControlError> {
        self.shared.command(|state| {
            let (device, _) = v2ip_sink(state, sink)?;
            let streams = source_streams(device, source_port)?;
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_SOURCE_SWITCH,
                build_v2ip_source_switch(device.uid, Ipv4Addr::UNSPECIFIED, streams.audio.ip),
            ))
        })
    }

    /// Routes this V2IP sink's video to the input bay with the given
    /// user-assigned name.
    pub fn select_video_source_by_name(
        &self,
        sink: BayUid,
        name: &str,
    ) -> Result<(), ControlError> {
        self.select_video_source(sink, self.source_port(sink, name)?)
    }

    /// Routes this V2IP sink's audio to a multicast address directly, leaving
    /// its video and ancillary streams alone.
    ///
    /// An unset port is the standard V2IP audio port. A format overrides the
    /// sample rate and channel count the receiver would otherwise assume.
    pub fn select_audio_source_addr(
        &self,
        sink: BayUid,
        audio_ip: Ipv4Addr,
        audio_port: Option<u16>,
        format: Option<V2ipAudioFormat>,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let (device, _) = v2ip_sink(state, sink)?;
            let streams = V2ipStreams {
                audio: StreamAddr {
                    ip: audio_ip,
                    port: audio_port.unwrap_or(V2IP_PORT_AUDIO),
                },
                ..V2ipStreams::default()
            };
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_MANUAL_SRC_SWITCH,
                build_v2ip_manual_source_switch(device.uid, streams, format),
            ))
        })
    }

    /// Routes this V2IP sink's audio from the input bay with the given
    /// user-assigned name.
    ///
    /// A format is carried on the manual switch frame, which is the only form
    /// that can override the receiver's sample rate and channel count.
    pub fn select_audio_source_by_name(
        &self,
        sink: BayUid,
        name: &str,
        format: Option<V2ipAudioFormat>,
    ) -> Result<(), ControlError> {
        let port = self.source_port(sink, name)?;
        let Some(format) = format else {
            return self.select_audio_source(sink, port);
        };
        self.shared.command(move |state| {
            let (device, _) = v2ip_sink(state, sink)?;
            let audio = source_streams(device, port)?.audio;
            let streams = V2ipStreams {
                audio: StreamAddr {
                    ip: audio.ip,
                    port: audio.port,
                },
                ..V2ipStreams::default()
            };
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_MANUAL_SRC_SWITCH,
                build_v2ip_manual_source_switch(device.uid, streams, Some(format)),
            ))
        })
    }

    /// The port of the input bay on `sink`'s device carrying `name`.
    fn source_port(&self, sink: BayUid, name: &str) -> Result<u16, ControlError> {
        self.shared.read(|state| {
            let (device, _) = bay_of(state, sink)?;
            device
                .bay_by_user_name(name)
                .map(|b| b.port)
                .ok_or_else(|| ControlError::UnknownSource(name.to_owned()))
        })
    }

    // ---- bay settings ----

    /// Renames a bay.
    pub fn set_bay_name(&self, bay: BayUid, name: &str) -> Result<(), ControlError> {
        let name = stored_name(name);
        self.shared.command(move |state| {
            let (device, _) = bay_of(state, bay)?;
            let payload = build_set_bay_name(device.uid, bay.port, &name);
            Ok(
                Command::new(Addressee::device(device), op::CHANGE_BAY_NAME, payload).then(
                    move |state, ev| {
                        if let Some(b) = state.bay_mut(bay) {
                            b.set_user_name(name, ev);
                        }
                    },
                ),
            )
        })
    }

    /// Hides a bay from the pickers that list it, or shows it again.
    pub fn set_bay_hidden(&self, bay: BayUid, hidden: bool) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let (device, _) = bay_of(state, bay)?;
            Ok(Command::new(
                Addressee::device(device),
                op::BAY_HIDE,
                build_bay_hide(device.uid, bay.port, hidden),
            )
            .then(move |state, ev| {
                if let Some(b) = state.bay_mut(bay) {
                    let status = if hidden {
                        HiddenStatus::Hidden
                    } else {
                        HiddenStatus::Visible
                    };
                    b.apply_hidden(status, ev);
                }
            }))
        })
    }

    /// Sets the EDID profile an input presents to the source attached to it.
    pub fn select_edid_profile(
        &self,
        bay: BayUid,
        profile: EdidProfile,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let (device, _) = bay_of(state, bay)?;
            Ok(Command::new(
                Addressee::device(device),
                op::BAY_EDID_PROFILE,
                build_edid_profile(device.uid, profile),
            )
            .then(move |state, ev| {
                if let Some(b) = state.bay_mut(bay) {
                    b.set_edid_profile(profile, ev);
                }
            }))
        })
    }

    /// Sends a remote-control action to whatever is attached to a bay.
    pub fn send_action(&self, bay: BayUid, action: RcAction) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let (device, _) = bay_of(state, bay)?;
            Ok(Command::new(
                Addressee::device(device),
                op::RC_TX_ACTION,
                build_rc_action(device.uid, bay.port, action),
            ))
        })
    }

    /// Powers on the device attached to a bay.
    pub fn power_on(&self, bay: BayUid) -> Result<(), ControlError> {
        self.set_power(bay, RcAction::POWER_ON, PowerStatus::On)
    }

    /// Powers off the device attached to a bay.
    pub fn power_off(&self, bay: BayUid) -> Result<(), ControlError> {
        self.set_power(bay, RcAction::POWER_OFF, PowerStatus::Off)
    }

    fn set_power(
        &self,
        bay: BayUid,
        action: RcAction,
        power: PowerStatus,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let (device, _) = bay_of(state, bay)?;
            Ok(Command::new(
                Addressee::device(device),
                op::RC_TX_ACTION,
                build_rc_action(device.uid, bay.port, action),
            )
            .then(move |state, ev| {
                if let Some(b) = state.bay_mut(bay) {
                    b.set_power_status(power, ev);
                }
            }))
        })
    }

    /// Sets a bay's volume, as a percentage, and optionally its mute state.
    ///
    /// Both channels are set together: the wire carries them separately, but
    /// nothing on this surface splits them.
    ///
    /// A bay with no volume control of its own is set through its
    /// [`linked_bay`](crate::BayInfo::linked_bay), so an output wired to an
    /// amplifier zone reaches that zone. [`volume_up`](Self::volume_up),
    /// [`volume_down`](Self::volume_down) and [`set_muted`](Self::set_muted)
    /// follow the same link, and read the volume they step from through it.
    pub fn set_volume(
        &self,
        bay: BayUid,
        volume: u8,
        muted: Option<bool>,
    ) -> Result<(), ControlError> {
        let volume = volume.min(100);
        let wanted = VolumeMuteStatus {
            volume_left: Some(volume),
            volume_right: Some(volume),
            muted_left: muted,
            muted_right: muted,
        };
        self.shared.command(move |state| {
            // The mesh may put this bay's volume control on another device, and
            // the command belongs where the volume lives, not where it was
            // addressed.
            let target = state.volume_bay(bay);
            let (device, b) = bay_of(state, target)?;
            if !b.has_volume_control() {
                return Err(ControlError::Unsupported("the bay has no volume control"));
            }
            Ok(Command::new(
                Addressee::device(device),
                op::AUDIO_SET_VOLUME,
                build_set_volume(device.uid, target.port, wanted),
            )
            .then(move |state, ev| {
                if let Some(device) = state.device_mut(target.device) {
                    device.apply_bay_volume(target.port, wanted, ev);
                }
            }))
        })
    }

    /// Raises a bay's volume by one percent.
    pub fn volume_up(&self, bay: BayUid) -> Result<(), ControlError> {
        self.set_volume(bay, self.current_volume(bay)?.saturating_add(1), None)
    }

    /// Lowers a bay's volume by one percent.
    pub fn volume_down(&self, bay: BayUid) -> Result<(), ControlError> {
        self.set_volume(bay, self.current_volume(bay)?.saturating_sub(1), None)
    }

    /// Mutes or unmutes a bay, keeping the volume it is set to.
    pub fn set_muted(&self, bay: BayUid, muted: bool) -> Result<(), ControlError> {
        self.set_volume(bay, self.current_volume(bay)?, Some(muted))
    }

    /// The volume a step or a mute is relative to.
    fn current_volume(&self, bay: BayUid) -> Result<u8, ControlError> {
        self.shared.read(|state| {
            let (_, b) = bay_of(state, state.volume_bay(bay))?;
            b.audio_volume
                .map(|v| v.volume())
                .ok_or(ControlError::NotReported("the bay's volume"))
        })
    }

    /// Applies amplifier settings to a zone.
    pub fn set_amp_zone_settings(
        &self,
        bay: BayUid,
        settings: AmpZoneSettings,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let (device, _) = bay_of(state, bay)?;
            Ok(Command::new(
                Addressee::device(device),
                op::AMP_ZONE_SETTINGS,
                build_amp_zone_settings(device.uid, bay.port, &settings),
            )
            .then(move |state, ev| {
                if let Some(b) = state.bay_mut(bay) {
                    b.set_amp_settings(settings, ev);
                }
            }))
        })
    }

    // ---- audio endpoints ----

    /// Mutes or unmutes an audio endpoint.
    pub fn set_audio_endpoint_muted(
        &self,
        device: DeviceUid,
        endpoint: u16,
        muted: bool,
    ) -> Result<(), ControlError> {
        self.audio_endpoint(device, audio_sub::MUTE, endpoint, u32::from(muted))
    }

    /// Sets an audio endpoint's trigger output.
    pub fn set_audio_endpoint_trigger(
        &self,
        device: DeviceUid,
        endpoint: u16,
        active: bool,
    ) -> Result<(), ControlError> {
        self.audio_endpoint(device, audio_sub::TRIGGER, endpoint, u32::from(active))
    }

    /// Sets an audio endpoint's volume.
    pub fn set_audio_endpoint_volume(
        &self,
        device: DeviceUid,
        endpoint: u16,
        volume: u32,
    ) -> Result<(), ControlError> {
        self.audio_endpoint(device, audio_sub::VOLUME, endpoint, volume)
    }

    fn audio_endpoint(
        &self,
        device: DeviceUid,
        sub: u16,
        endpoint: u16,
        value: u32,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let device = device_of(state, device)?;
            let mut payload = audio_cmd_header(sub, device.uid);
            payload.extend_from_slice(&audio_param(endpoint, value));
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_AUDIO,
                payload,
            ))
        })
    }

    /// Routes a source endpoint on one device to a sink endpoint on another.
    pub fn select_audio_endpoint_input(
        &self,
        sink: DeviceUid,
        sink_endpoint: u16,
        source: DeviceUid,
        source_endpoint: u16,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let device = device_of(state, sink)?;
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_AUDIO,
                build_audio_select_input(sink, sink_endpoint, source, source_endpoint),
            ))
        })
    }

    // ---- the whole device ----

    /// Starts or stops a V2IP device reporting its transport statistics.
    pub fn subscribe_v2ip_stats(
        &self,
        device: DeviceUid,
        subscribe: bool,
    ) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let device = device_of(state, device)?;
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_STATS,
                build_stats_request(device.uid, subscribe),
            ))
        })
    }

    /// Reboots a device.
    ///
    /// The device is marked as rebooting once the frame is away, so the
    /// silence that follows does not read as one that went offline.
    pub fn reboot(&self, device: DeviceUid) -> Result<(), ControlError> {
        self.shared.command(move |state| {
            let d = device_of(state, device)?;
            Ok(Command::new(
                Addressee::device(d),
                op::SYS_REBOOT,
                build_target_only(d.uid),
            )
            .then(move |state, _| {
                if let Some(d) = state.device_mut(device) {
                    d.rebooting = true;
                }
            }))
        })
    }

    /// Asks every peer to report its monitoring data now rather than on its own
    /// schedule.
    pub fn send_monitoring_pulse(&self) -> Result<(), ControlError> {
        self.shared
            .send(&Addressee::Broadcast, op::SYS_MONITORING_PULSE, &[])?;
        Ok(())
    }

    // ---- multiviewer ----

    /// Sets the window layout.
    pub fn set_multiviewer_view_mode(
        &self,
        device: DeviceUid,
        mode: MultiviewerViewMode,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::VIEW_MODE, &[mode.to_wire()])
    }

    /// Assigns a source to one window.
    pub fn set_multiviewer_video_source(
        &self,
        device: DeviceUid,
        screen: u8,
        source: MultiviewerSource,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::VIDEO_SOURCE, &[screen, source.to_wire()])
    }

    /// Selects which window's audio is output.
    pub fn set_multiviewer_audio_source(
        &self,
        device: DeviceUid,
        source: MultiviewerSource,
    ) -> Result<(), ControlError> {
        // The status report numbers the windows from one and this command from
        // zero.
        self.multiviewer(
            device,
            mv_sub::AUDIO_SOURCE,
            &[source.to_wire().saturating_sub(1)],
        )
    }

    /// Sets the output volume and mute state.
    pub fn set_multiviewer_audio_volume(
        &self,
        device: DeviceUid,
        volume: u8,
        muted: bool,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::AUDIO_VOLUME, &[volume, u8::from(muted)])
    }

    /// Sets the EDID template presented to the sources.
    pub fn set_multiviewer_edid_template(
        &self,
        device: DeviceUid,
        template: MultiviewerEdidTemplate,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::EDID_TEMPLATE, &[template.to_wire()])
    }

    /// Selects which window receives remote-control passthrough.
    pub fn set_multiviewer_remote_control(
        &self,
        device: DeviceUid,
        source: MultiviewerSource,
    ) -> Result<(), ControlError> {
        // Numbered from zero here, as on
        // [`Remote::set_multiviewer_audio_source`].
        self.multiviewer(
            device,
            mv_sub::ROUTE_RC,
            &[source.to_wire().saturating_sub(1)],
        )
    }

    /// Sets how large the picture-in-picture window is.
    pub fn set_multiviewer_pip_size(
        &self,
        device: DeviceUid,
        size: MultiviewerPipSize,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::PIP_SIZE, &[size.to_wire()])
    }

    /// Sets which corner the picture-in-picture window sits in.
    pub fn set_multiviewer_pip_position(
        &self,
        device: DeviceUid,
        position: MultiviewerPipPosition,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::PIP_POSITION, &[position.to_wire()])
    }

    /// Sets the aspect ratio the windows are scaled to.
    pub fn set_multiviewer_aspect_ratio(
        &self,
        device: DeviceUid,
        aspect: MultiviewerAspectRatio,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::ASPECT, &[aspect.to_wire()])
    }

    /// Enables or disables switching windows on its own.
    pub fn set_multiviewer_auto_switch(
        &self,
        device: DeviceUid,
        enable: bool,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::AUTO_SWITCH, &[u8::from(enable)])
    }

    /// Sets the output resolution and refresh rate.
    pub fn set_multiviewer_output_mode(
        &self,
        device: DeviceUid,
        mode: MultiviewerOutputMode,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::OUTPUT_MODE, &[mode.to_wire()])
    }

    /// Sets the IT-content flag on the output.
    pub fn set_multiviewer_output_itc(
        &self,
        device: DeviceUid,
        mode: MultiviewerItcMode,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::OUTPUT_ITC_MODE, &[mode.to_wire()])
    }

    /// Sets the HDCP version negotiated on the output.
    pub fn set_multiviewer_hdcp_mode(
        &self,
        device: DeviceUid,
        mode: MultiviewerHdcpMode,
    ) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::HDCP_MODE, &[mode.to_wire()])
    }

    /// Maps a source device onto one of the multiviewer's inputs.
    ///
    /// The zero identifier clears the mapping.
    pub fn set_multiviewer_input_source(
        &self,
        device: DeviceUid,
        input: u8,
        source: DeviceUid,
    ) -> Result<(), ControlError> {
        let mut args = Vec::with_capacity(24);
        args.extend_from_slice(source.as_bytes());
        args.push(input);
        // mv_config_source_t is 4-aligned behind its uid, so seven bytes of
        // padding follow the input index.
        args.extend_from_slice(&[0; 7]);
        self.multiviewer(device, mv_sub::CONFIG_SOURCE, &args)
    }

    /// Asks the multiviewer to route its sources itself.
    pub fn multiviewer_auto_route(&self, device: DeviceUid) -> Result<(), ControlError> {
        self.multiviewer(device, mv_sub::AUTO_ROUTE, &[])
    }

    fn multiviewer(&self, device: DeviceUid, sub: u8, args: &[u8]) -> Result<(), ControlError> {
        self.shared.command(|state| {
            let device = device_of(state, device)?;
            if !device.is_multiviewer() {
                return Err(ControlError::Unsupported("the device is not a multiviewer"));
            }
            Ok(Command::new(
                Addressee::device(device),
                op::V2IP_MULTIVIEWER,
                mv_cmd_payload(device.uid, sub, args),
            ))
        })
    }
}
