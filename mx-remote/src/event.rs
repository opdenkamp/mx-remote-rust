// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Events and the handler that receives them.

use crate::types::*;
use crate::wire::{BayUid, DeviceUid, EdidProfile, LinkFeature, RcAction, RcKey, RcType};

/// Declares the event set.
///
/// One declaration produces the [`Event`] enum, the [`EventHandler`] trait and
/// the dispatch that connects them, so a new event cannot reach the enum
/// without reaching the trait, and cannot be dispatched without fanning in to
/// the generic update. The fan-in is written once here rather than repeated at
/// each call site.
///
/// The section an event is declared in decides what it fans in to: `device`
/// events reach `on_device_update`, `bay` events reach `on_bay_update`, and
/// `bay_and_device` events reach both. A link change concerns the bay and the
/// device that owns it, which is why it is neither of the first two.
macro_rules! events {
    (
        device {
            $( $(#[$dmeta:meta])* $dvariant:ident => $dmethod:ident ( $($darg:ident : $dty:ty),* ); )*
        }
        bay {
            $( $(#[$bmeta:meta])* $bvariant:ident => $bmethod:ident ( $($barg:ident : $bty:ty),* ); )*
        }
        bay_and_device {
            $( $(#[$lmeta:meta])* $lvariant:ident => $lmethod:ident ( $($larg:ident : $lty:ty),* ); )*
        }
    ) => {
        /// Something that changed, or a request that arrived.
        ///
        /// Events are collected while the state lock is held and dispatched
        /// after it is released, so a handler may call back into the library.
        #[derive(Clone, Debug, PartialEq)]
        #[non_exhaustive]
        pub enum Event {
            $(
                $(#[$dmeta])*
                $dvariant {
                    /// The device the event concerns.
                    device: DeviceUid,
                    $( #[allow(missing_docs)] $darg: $dty, )*
                },
            )*
            $(
                $(#[$bmeta])*
                $bvariant {
                    /// The bay the event concerns.
                    bay: BayUid,
                    $( #[allow(missing_docs)] $barg: $bty, )*
                },
            )*
            $(
                $(#[$lmeta])*
                $lvariant {
                    /// The bay the event concerns.
                    bay: BayUid,
                    $( #[allow(missing_docs)] $larg: $lty, )*
                },
            )*
        }

        /// Receives events.
        ///
        /// Every method has a no-op default, so an implementation names only
        /// the events it cares about. Handlers run one at a time from the
        /// thread that produced the event, with no lock held: calling back into
        /// the library from one is safe, but blocking for long stalls the
        /// receive path.
        #[allow(unused_variables)]
        pub trait EventHandler: Send + Sync {
            $(
                $(#[$dmeta])*
                fn $dmethod(&self, device: DeviceUid $(, $darg: $dty)*) {}
            )*
            $(
                $(#[$bmeta])*
                fn $bmethod(&self, bay: BayUid $(, $barg: $bty)*) {}
            )*
            $(
                $(#[$lmeta])*
                fn $lmethod(&self, bay: BayUid $(, $larg: $lty)*) {}
            )*

            /// Fired after every device-level event above.
            fn on_device_update(&self, device: DeviceUid) {}

            /// Fired after every bay-level event above.
            fn on_bay_update(&self, bay: BayUid) {}
        }

        impl Event {
            /// Delivers this event to `handler`, then the generic update it
            /// fans in to.
            pub(crate) fn dispatch(self, handler: &dyn EventHandler) {
                match self {
                    $(
                        Self::$dvariant { device $(, $darg)* } => {
                            handler.$dmethod(device $(, $darg)*);
                            handler.on_device_update(device);
                        }
                    )*
                    $(
                        Self::$bvariant { bay $(, $barg)* } => {
                            handler.$bmethod(bay $(, $barg)*);
                            handler.on_bay_update(bay);
                        }
                    )*
                    $(
                        Self::$lvariant { bay $(, $larg)* } => {
                            handler.$lmethod(bay $(, $larg)*);
                            handler.on_bay_update(bay);
                            handler.on_device_update(bay.device);
                        }
                    )*
                }
            }
        }
    };
}

/// Ignores every event.
///
/// The handler a client that only reads state through [`Remote`] needs, since
/// the trait is not optional and its methods all default to nothing.
///
/// [`Remote`]: crate::Remote
impl EventHandler for () {}

events! {
    device {
        /// The device's configuration changed.
        DeviceConfigChanged => on_device_config_changed();
        /// The device has reported every part of its configuration.
        DeviceConfigComplete => on_device_config_complete();
        /// The device started or stopped answering.
        DeviceOnlineChanged => on_device_online_changed(online: bool);
        /// The device reported new temperatures.
        DeviceTemperatureChanged => on_device_temperature_changed(temperatures: Vec<u8>);
        /// A firmware component reported its version.
        FirmwareVersionChanged => on_firmware_version_changed(version: FirmwareVersion);
        /// The device reported a system status.
        SystemStatusChanged => on_system_status_changed(status: u16, message: String);
        /// A network port reported its link state.
        NetworkStatusChanged => on_network_status_changed(status: NetworkPortStatus);
        /// The device reported V2IP statistics.
        V2ipStatsChanged => on_v2ip_stats_changed(stats: V2ipDeviceStats);
        /// The streams the device's source bays advertise changed.
        V2ipSourcesChanged => on_v2ip_sources_changed(sources: Vec<V2ipStreamSources>);
        /// The device's V2IP encoder configuration changed.
        V2ipDetailsChanged => on_v2ip_details_changed(details: DeviceV2ipDetails);
        /// The streams the device's sink is subscribed to changed.
        V2ipSinkChanged => on_v2ip_sink_changed(sink: DeviceV2ipSink);
        /// A multiviewer reported its state.
        MultiviewerStatusChanged => on_multiviewer_status_changed(status: MultiviewerStatus);
        /// The device reported its audio endpoint tree.
        AudioEndpointsChanged => on_audio_endpoints_changed(endpoints: AudioEndpoints);
        /// The device reported its mesh master.
        MeshMasterChanged => on_mesh_master_changed(master: DeviceUid);
        /// The device reported its view of the mesh topology.
        TopologyChanged => on_topology_changed(topology: Vec<TopologyEntry>);
        /// A ProAmp8 reported its Dolby settings.
        AmpDolbySettingsChanged => on_amp_dolby_settings_changed(settings: AmpDolbySettings);
        /// A PDU reported its electrical state.
        PduStateChanged => on_pdu_state_changed(state: PduState);
        /// Installer setup was completed or cleared.
        SetupStatusChanged => on_setup_status_changed(completed: bool);
        /// The installer id changed.
        InstallerIdChanged => on_installer_id_changed(installer_id: u16);
        /// The sink was told to show a window.
        TilingChanged => on_tiling_changed(tiling: V2ipTilingConfig);
        /// A source bay's remote-control configuration changed.
        RcSettingsChanged => on_rc_settings_changed(settings: RcSettings);
        /// A V2IP device was linked to a remote peer.
        V2ipLinkChanged => on_v2ip_link_changed(target: DeviceUid);
        /// A multiviewer command arrived.
        MultiviewerCommand => on_multiviewer_command(command: MultiviewerCommand);
        /// An audio endpoint was switched to a new source.
        AudioSelectInput => on_audio_select_input(change: AudioChangeSource);
        /// An audio endpoint was muted or unmuted.
        AudioEndpointMute => on_audio_endpoint_mute(endpoint: u16, muted: bool);
        /// An audio endpoint's trigger changed.
        AudioEndpointTrigger => on_audio_endpoint_trigger(endpoint: u16, active: bool);
        /// An audio endpoint's volume changed.
        AudioEndpointVolume => on_audio_endpoint_volume(endpoint: u16, volume: u32);
        /// A peer asked every device to announce itself.
        DiscoverRequest => on_discover_request();
        /// A peer asked a device to switch a sink.
        SetRouteRequested => on_set_route_requested(request: SetRouteRequest);
        /// A peer asked a device for its EDID.
        EdidRequested => on_edid_requested(request: EdidRequest);
        /// A device answered with its EDID.
        EdidReceived => on_edid_received(edid: EdidRecord);
        /// A peer asked a device to rename a bay.
        BayNameChangeRequested => on_bay_name_change_requested(change: BayNameChange);
        /// A peer asked a device to switch its EDID profile.
        EdidProfileChangeRequested => on_edid_profile_change_requested(change: EdidProfileChange);
        /// A peer asked a device to reboot.
        RebootRequested => on_reboot_requested(request: RebootRequest);
        /// A peer asked devices to factory-reset.
        FactoryResetRequested => on_factory_reset_requested(request: FactoryResetRequest);
        /// A device sent its monitoring pulse.
        MonitoringPulse => on_monitoring_pulse();
        /// A peer asked a device to upgrade its FPGA.
        UpgradeFpgaRequested => on_upgrade_fpga_requested();
        /// A peer asked a device to re-detect its bays.
        DetectBaysRequested => on_detect_bays_requested();
        /// A peer asked a sink to enter or leave power save.
        PowerSaveRequested => on_power_save_requested(request: V2ipPowerSaveRequest);
        /// A peer asked a device to send a remote-control key.
        KeyTransmitRequested => on_key_transmit_requested(request: KeyTransmitRequest);
        /// A peer asked a device to perform a remote-control action.
        ActionTransmitRequested => on_action_transmit_requested(request: ActionTransmitRequest);
        /// A peer asked a device to blast raw infrared.
        IrTransmitRequested => on_ir_transmit_requested(request: IrTransmitRequest);
        /// A device was added to or removed from the source blacklist.
        BlacklistChanged => on_blacklist_changed(change: V2ipBlacklistChange);
        /// A video wall command arrived.
        VideoWallCommand => on_video_wall_command(command: VideoWallCommand);
    }
    bay {
        /// A bay was seen for the first time.
        BayRegistered => on_bay_registered();
        /// The bay's routed video source changed.
        VideoSourceChanged => on_video_source_changed(source: Option<BayUid>);
        /// The bay's routed audio source changed.
        AudioSourceChanged => on_audio_source_changed(source: Option<BayUid>);
        /// The bay's volume or mute state changed.
        VolumeChanged => on_volume_changed(volume: VolumeMuteStatus);
        /// The attached device's power state changed.
        PowerChanged => on_power_changed(power: PowerStatus);
        /// The bay was renamed.
        NameChanged => on_name_changed(name: String);
        /// A signal appeared or disappeared.
        SignalDetectedChanged => on_signal_detected_changed(detected: bool);
        /// The bay started or stopped reporting a fault.
        FaultyChanged => on_faulty_changed(faulty: bool);
        /// The bay was hidden or shown.
        HiddenChanged => on_hidden_changed(hidden: bool);
        /// Power over Ethernet started or stopped supplying the bay.
        PoePoweredChanged => on_poe_powered_changed(powered: bool);
        /// The HDBaseT link came up or went down.
        HdbtConnectedChanged => on_hdbt_connected_changed(connected: bool);
        /// The signal format description changed.
        SignalTypeChanged => on_signal_type_changed(signal_type: String);
        /// Hot-plug detect was asserted or released.
        HpdDetectedChanged => on_hpd_detected_changed(detected: bool);
        /// A CEC device answered or stopped answering.
        CecDetectedChanged => on_cec_detected_changed(detected: bool);
        /// The audio return channel changed.
        ArcChanged => on_arc_changed(arc: ArcStatus);
        /// The input's EDID profile changed.
        EdidProfileChanged => on_edid_profile_changed(profile: EdidProfile);
        /// The input's remote-control type changed.
        RcTypeChanged => on_rc_type_changed(rc_type: RcType);
        /// A remote-control key was pressed on the bay.
        KeyPressed => on_key_pressed(key: RcKey);
        /// A remote-control action was received on the bay.
        ActionReceived => on_action_received(action: RcAction);
        /// The bay started or stopped mirroring another output.
        MirrorStatusChanged => on_mirror_status_changed(mirror: BayMirrorStatus);
        /// A ProAmp8 zone's settings changed.
        AmpZoneSettingsChanged => on_amp_zone_settings_changed(settings: AmpZoneSettings);
        /// A volume step was requested on the bay.
        VolumeStep => on_volume_step(up: bool);
        /// The bay detected audio clipping.
        AudioClipped => on_audio_clip(clip: AudioClip);
        /// Raw infrared was captured on the bay.
        IrCaptured => on_ir_captured(capture: IrCapture);
        /// The devices filtered out of this sink's picker changed.
        FilteredDevicesChanged => on_filtered_devices_changed(filtered: Vec<DeviceUid>);
        /// The audio endpoint the bay carries changed.
        AudioEndpointChanged => on_audio_endpoint_changed(endpoint: u8);
        /// The bay's V2IP encoder was enabled or disabled.
        EncoderDisabledChanged => on_encoder_disabled_changed(disabled: bool);
        /// The bay's V2IP decoder was enabled or disabled.
        DecoderDisabledChanged => on_decoder_disabled_changed(disabled: bool);
    }
    bay_and_device {
        /// The bay was linked to a bay on another device.
        ///
        /// `linked_serial` is the serial of the device at the other end of the
        /// link, and `bay_name` the name of the bay whose link record changed:
        /// this bay on the device that reported the change, and the far bay on
        /// its peer. Both ends are told, so both fire.
        BayLinked => on_bay_linked(linked_serial: String, bay_name: String, features: LinkFeature);
        /// The bay's link to another device was removed.
        ///
        /// The arguments describe the link that was removed, and mean what
        /// they do on [`Event::BayLinked`].
        BayUnlinked => on_bay_unlinked(linked_serial: String, bay_name: String);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
    }

    impl Recorder {
        fn record(&self, what: &str) {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(what.to_owned());
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().map(|c| c.clone()).unwrap_or_default()
        }
    }

    impl EventHandler for Recorder {
        fn on_power_changed(&self, _bay: BayUid, power: PowerStatus) {
            self.record(&format!("power={power}"));
        }

        fn on_setup_status_changed(&self, _device: DeviceUid, completed: bool) {
            self.record(&format!("setup={completed}"));
        }

        fn on_bay_update(&self, _bay: BayUid) {
            self.record("bay_update");
        }

        fn on_device_update(&self, _device: DeviceUid) {
            self.record("device_update");
        }
    }

    const DEVICE: DeviceUid = DeviceUid::from_array([9; 16]);

    #[test]
    fn a_bay_event_fires_its_own_method_then_the_generic_bay_update() {
        let recorder = Recorder::default();
        Event::PowerChanged {
            bay: BayUid::new(DEVICE, 3),
            power: PowerStatus::On,
        }
        .dispatch(&recorder);
        assert_eq!(recorder.calls(), ["power=on", "bay_update"]);
    }

    #[test]
    fn a_device_event_fires_its_own_method_then_the_generic_device_update() {
        let recorder = Recorder::default();
        Event::SetupStatusChanged {
            device: DEVICE,
            completed: true,
        }
        .dispatch(&recorder);
        assert_eq!(recorder.calls(), ["setup=true", "device_update"]);
    }

    #[test]
    fn a_link_event_fires_both_generic_updates() {
        let recorder = Recorder::default();
        Event::BayUnlinked {
            bay: BayUid::new(DEVICE, 3),
            linked_serial: "AB1234".to_owned(),
            bay_name: "Output 1".to_owned(),
        }
        .dispatch(&recorder);
        assert_eq!(recorder.calls(), ["bay_update", "device_update"]);
    }

    #[test]
    fn an_event_a_handler_does_not_name_still_fires_the_generic_update() {
        let recorder = Recorder::default();
        Event::MonitoringPulse { device: DEVICE }.dispatch(&recorder);
        assert_eq!(
            recorder.calls(),
            ["device_update"],
            "the default no-op must not swallow the fan-in"
        );
    }
}
