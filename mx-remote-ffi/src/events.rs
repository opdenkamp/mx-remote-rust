// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The event bridge: a C table of function pointers, and the handler that
//! calls into it.
//!
//! Every member may be null, and a null member drops its event. Two of them
//! are worth naming first, because a program that sets only those two is
//! already complete: `on_device_update` fires after every device-level event
//! and `on_bay_update` after every bay-level one, so a caller that redraws
//! from `mxr_device()` and `mxr_bay()`
//! needs nothing else.
//!
//! The rest divide by what they can tell a caller that a snapshot cannot. An
//! event whose payload is state carries only the identifier, because the
//! snapshot is where that value lives and a copy in the callback could only be
//! staler. An event that is a request or a one-off - a key press, a reboot
//! demand, an IR blast - carries a struct, because nothing stores it.
//!
//! Callbacks run on the receive thread with no lock held, one at a time.
//! Calling back into the library from one is safe; blocking in one stalls
//! every device. Pointers handed to a callback are borrowed for the length of
//! that call, so anything needed afterwards must be copied.

use std::ffi::{c_char, c_void, CString};

use mx_remote::{
    ActionTransmitRequest, ArcStatus, AudioChangeSource, AudioClip, BayNameChange, BayUid,
    DeviceUid, EdidProfileChange, EdidRecord, EdidRequest, EventHandler, FactoryResetRequest,
    IrCapture, IrMeta, IrTransmitRequest, KeyTransmitRequest, LinkFeature, MultiviewerCommand,
    PowerStatus, RcAction, RcKey, RebootRequest, SetRouteRequest, V2ipBlacklistChange,
    V2ipPowerSaveRequest, VideoWallCommand, VolumeMuteStatus,
};

use crate::abi::{bay_or_zero, guard, mxr_bay_uid_t, mxr_tribool_t, mxr_uid_t};
use crate::info::{mxr_arc_status_t, mxr_power_status_t};

// ---- callback signatures ----

/// Names only the device the event concerns.
pub type mxr_device_cb = Option<extern "C" fn(userdata: *mut c_void, device: mxr_uid_t)>;
/// Names a device and a flag.
pub type mxr_device_bool_cb =
    Option<extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, value: bool)>;
/// Names a device and a second device.
pub type mxr_device_uid_cb =
    Option<extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, other: mxr_uid_t)>;
/// Names a device and a 16-bit value.
pub type mxr_device_u16_cb =
    Option<extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, value: u16)>;
/// Names a device, one of its audio endpoints, and a flag.
pub type mxr_endpoint_bool_cb =
    Option<extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, endpoint: u16, value: bool)>;
/// Names a device, one of its audio endpoints, and a 32-bit value.
pub type mxr_endpoint_u32_cb =
    Option<extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, endpoint: u16, value: u32)>;
/// Names a device, a status code and a message.
pub type mxr_system_status_cb = Option<
    extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, status: u16, message: *const c_char),
>;

/// Names only the bay the event concerns.
pub type mxr_bay_cb = Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t)>;
/// Names a bay and a flag.
pub type mxr_bay_bool_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, value: bool)>;
/// Names a bay and a string, borrowed for the call.
pub type mxr_bay_str_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, value: *const c_char)>;
/// Names a bay and another bay, the zero device standing for none.
pub type mxr_bay_bay_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, other: mxr_bay_uid_t)>;
/// Names a bay and an 8-bit value.
pub type mxr_bay_u8_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, value: u8)>;
/// Names a bay and a 16-bit value.
pub type mxr_bay_u16_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, value: u16)>;
/// Names a bay, its combined volume percentage and its mute state.
pub type mxr_volume_cb = Option<
    extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, volume: u8, muted: mxr_tribool_t),
>;
/// Names a bay and the power state of what is connected to it.
pub type mxr_power_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, power: mxr_power_status_t)>;
/// Names a bay and its audio return channel.
pub type mxr_arc_cb =
    Option<extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, arc: mxr_arc_status_t)>;
/// Names a bay and the link that was made to it.
pub type mxr_bay_linked_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        bay: mxr_bay_uid_t,
        linked_serial: *const c_char,
        bay_name: *const c_char,
        features: u32,
    ),
>;
/// Names a bay and the link that was removed from it.
pub type mxr_bay_unlinked_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        bay: mxr_bay_uid_t,
        linked_serial: *const c_char,
        bay_name: *const c_char,
    ),
>;

// ---- notification payloads ----

/// Asks a device, addressed by serial, to switch a sink.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_set_route_request_t {
    /// Serial of the device to act on, borrowed for the call.
    pub serial: *const c_char,
    /// Output bay to switch.
    pub sink_bay: u16,
    /// Source bay to switch it to.
    pub source_bay: u16,
    /// Whether to skip the power-on commands that normally accompany a switch.
    pub no_power_on: bool,
    /// Set when the request routes audio only.
    pub audio_only: bool,
}

/// Asks one device for its EDID.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_edid_request_t {
    /// The device being asked.
    pub target: mxr_uid_t,
    /// Whether the sink's EDID is wanted rather than the source's.
    pub output: bool,
}

/// One EDID block from a device's reply.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_edid_record_t {
    /// True for a sink's EDID, false for a source's.
    pub output: bool,
    /// A base block plus one extension block, borrowed for the call.
    pub data: *const u8,
    /// Length of `data`, normally 256.
    pub data_len: usize,
}

/// Asks a device to rename one of its bays.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_bay_name_change_t {
    /// The device to act on.
    pub target: mxr_uid_t,
    /// The bay to rename.
    pub port: u16,
    /// The new name, borrowed for the call.
    pub name: *const c_char,
}

/// Asks a device to switch its input EDID profile.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_edid_profile_change_t {
    /// The device to act on.
    pub target: mxr_uid_t,
    /// The profile to switch to.
    pub profile: u16,
}

/// Asks peers to factory-reset.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_factory_reset_request_t {
    /// Set by the broadcast form, which targets every peer.
    pub all: bool,
    /// The single device addressed, zero when `all` is set or when the request
    /// addresses only its sender.
    pub target: mxr_uid_t,
}

/// Asks a sink to enter or leave power save.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_power_save_request_t {
    /// The sink to act on, zero on the broadcast form.
    pub target: mxr_uid_t,
    /// Whether power save is being entered.
    pub enabled: bool,
}

/// Asks one device to send a remote-control key on a bay.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_key_transmit_request_t {
    /// The device to act on.
    pub target: mxr_uid_t,
    /// Bay in the target's own numbering, which is not a port number.
    pub local_bay: u16,
    /// The key to send.
    pub key: u16,
}

/// Asks one device to perform a remote-control action.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_action_transmit_request_t {
    /// The device to act on.
    pub target: mxr_uid_t,
    /// Bay in the target's own numbering, which is not a port number.
    pub local_bay: u16,
    /// The action to perform.
    pub action: u16,
}

/// The metadata shared by the raw-IR capture and transmit frames.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_ir_meta_t {
    /// Tick length of the timing values.
    pub timer_resolution: u16,
    /// Carrier frequency in Hz.
    pub frequency: u16,
    /// Number of timing values that follow.
    pub nb_timings: u16,
    /// Index at which the repeat section starts.
    pub repeat_offset: u16,
    /// Capture status.
    pub status: u8,
}

impl From<IrMeta> for mxr_ir_meta_t {
    fn from(m: IrMeta) -> Self {
        Self {
            timer_resolution: m.timer_resolution,
            frequency: m.frequency,
            nb_timings: m.nb_timings,
            repeat_offset: m.repeat_offset,
            status: m.status,
        }
    }
}

/// Raw IR captured on a bay of the sending device.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_ir_capture_t {
    /// Sender clock at capture time.
    pub timestamp: u32,
    /// Sender clock at the last signal change.
    pub last_change: u32,
    /// Metadata for the timings.
    pub meta: mxr_ir_meta_t,
    /// The raw on/off timing blob, borrowed for the call.
    pub timings: *const u8,
    /// Length of `timings`.
    pub timings_len: usize,
}

/// Asks one device to blast raw IR on one of its local bays.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_ir_transmit_request_t {
    /// The device to act on.
    pub target: mxr_uid_t,
    /// Bay mode in the target's own numbering, which is not a port number.
    pub local_mode: u8,
    /// Bay number in the target's own numbering, which is not a port number.
    pub local_bay: u8,
    /// Sender clock at send time.
    pub timestamp: u32,
    /// Metadata for the timings.
    pub meta: mxr_ir_meta_t,
    /// The raw on/off timing blob, borrowed for the call.
    pub timings: *const u8,
    /// Length of `timings`.
    pub timings_len: usize,
}

/// Registers or unregisters a device on the source blacklist.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_blacklist_change_t {
    /// The device being listed.
    pub target: mxr_uid_t,
    /// Whether it is being registered rather than removed.
    pub registered: bool,
}

/// Asks one sink to crop its source to a wall window.
///
/// The window replaces the sink's outright: a zero width or height is the wire
/// spelling of "clear the wall and show the full frame", not of "unset". A
/// revert carries no window, and the geometry in it means nothing.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_video_wall_command_t {
    /// The sink to act on.
    pub target: mxr_uid_t,
    /// Window origin, horizontal.
    pub pos_x: u16,
    /// Window origin, vertical.
    pub pos_y: u16,
    /// Window width.
    pub width: u16,
    /// Window height.
    pub height: u16,
    /// Active picture width the window was authored against.
    pub raster_w: u16,
    /// Active picture height the window was authored against.
    pub raster_h: u16,
    /// 0 preview, 1 store, 2 revert.
    pub op: u8,
}

/// A command addressed to a multiviewer.
///
/// The parameters are raw: the opcode belongs to the multiviewer module rather
/// than to MatrixOS, so there is no firmware source here to pin per-sub-command
/// field semantics against.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_multiviewer_command_t {
    /// The multiviewer being addressed.
    pub target: mxr_uid_t,
    /// The sub-opcode. A value this library has no name for still arrives.
    pub op: u8,
    /// Everything after the envelope, borrowed for the call.
    pub params: *const u8,
    /// Length of `params`.
    pub params_len: usize,
}

/// Which source endpoint an audio sink endpoint was switched to.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_audio_change_source_t {
    /// The device whose endpoint is being listened to.
    pub source_uid: mxr_uid_t,
    /// The endpoint being listened to.
    pub source_id: u16,
    /// The device doing the listening.
    pub target_uid: mxr_uid_t,
    /// The endpoint doing the listening.
    pub target_id: u16,
}

/// Carries a request addressed to a device.
pub type mxr_set_route_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        request: *const mxr_set_route_request_t,
    ),
>;
/// Carries a request for a device's EDID.
pub type mxr_edid_request_cb = Option<
    extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, request: *const mxr_edid_request_t),
>;
/// Carries one EDID block a device replied with.
pub type mxr_edid_record_cb = Option<
    extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, record: *const mxr_edid_record_t),
>;
/// Carries a request to rename a bay.
pub type mxr_bay_name_change_cb = Option<
    extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, change: *const mxr_bay_name_change_t),
>;
/// Carries a request to switch an EDID profile.
pub type mxr_edid_profile_change_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        change: *const mxr_edid_profile_change_t,
    ),
>;
/// Carries a factory-reset request.
pub type mxr_factory_reset_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        request: *const mxr_factory_reset_request_t,
    ),
>;
/// Carries a power-save request.
pub type mxr_power_save_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        request: *const mxr_power_save_request_t,
    ),
>;
/// Carries a request to send a remote-control key.
pub type mxr_key_transmit_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        request: *const mxr_key_transmit_request_t,
    ),
>;
/// Carries a request to perform a remote-control action.
pub type mxr_action_transmit_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        request: *const mxr_action_transmit_request_t,
    ),
>;
/// Carries a request to blast raw infrared.
pub type mxr_ir_transmit_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        request: *const mxr_ir_transmit_request_t,
    ),
>;
/// Carries a blacklist change.
pub type mxr_blacklist_cb = Option<
    extern "C" fn(userdata: *mut c_void, device: mxr_uid_t, change: *const mxr_blacklist_change_t),
>;
/// Carries a video wall command.
pub type mxr_video_wall_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        command: *const mxr_video_wall_command_t,
    ),
>;
/// Carries a multiviewer command.
pub type mxr_multiviewer_command_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        command: *const mxr_multiviewer_command_t,
    ),
>;
/// Carries an audio input selection.
pub type mxr_audio_select_cb = Option<
    extern "C" fn(
        userdata: *mut c_void,
        device: mxr_uid_t,
        change: *const mxr_audio_change_source_t,
    ),
>;
/// Carries raw infrared captured on a bay.
pub type mxr_ir_capture_cb = Option<
    extern "C" fn(userdata: *mut c_void, bay: mxr_bay_uid_t, capture: *const mxr_ir_capture_t),
>;

/// What to call when something happens.
///
/// Zero the whole struct and fill in only what is wanted: a null member drops
/// its event. `userdata` is whatever was passed to
/// `mxr_remote_new()` and is never examined here.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mxr_callbacks_t {
    /// Fires after every device-level event below.
    pub on_device_update: mxr_device_cb,
    /// Fires after every bay-level event below.
    pub on_bay_update: mxr_bay_cb,

    /// The device's configuration changed.
    pub on_device_config_changed: mxr_device_cb,
    /// The device has reported every part of its configuration.
    pub on_device_config_complete: mxr_device_cb,
    /// The device started or stopped answering.
    pub on_device_online_changed: mxr_device_bool_cb,
    /// The device reported new temperatures; read them with
    /// `mxr_device_temperatures()`.
    pub on_device_temperature_changed: mxr_device_cb,
    /// A firmware component reported its version; read it with
    /// `mxr_device_firmware()`.
    pub on_firmware_version_changed: mxr_device_cb,
    /// The device reported a status about itself.
    pub on_system_status_changed: mxr_system_status_cb,
    /// A network port reported its link state; read it with
    /// `mxr_network_status()`.
    pub on_network_status_changed: mxr_device_cb,
    /// The device reported V2IP statistics; read them with
    /// `mxr_v2ip_stats()`.
    pub on_v2ip_stats_changed: mxr_device_cb,
    /// The streams the device's source bays advertise changed; read them with
    /// `mxr_v2ip_sources()`.
    pub on_v2ip_sources_changed: mxr_device_cb,
    /// The device's V2IP encoder configuration changed; read it with
    /// `mxr_v2ip_details()`.
    pub on_v2ip_details_changed: mxr_device_cb,
    /// The streams the device's sink is subscribed to changed; read them with
    /// `mxr_v2ip_sink()`.
    ///
    /// A route request addressed to the device fires this as soon as it is
    /// seen, so this reports what the mesh now believes rather than what the
    /// device confirmed - it acknowledges nothing, and only its own
    /// configuration report, sent on its own schedule, settles a route.
    pub on_v2ip_sink_changed: mxr_device_cb,
    /// A multiviewer reported its state; read it with
    /// `mxr_multiviewer_status()`.
    pub on_multiviewer_status_changed: mxr_device_cb,
    /// The device reported its audio endpoint tree; read it with
    /// `mxr_audio_endpoints()`.
    pub on_audio_endpoints_changed: mxr_device_cb,
    /// The device reported its mesh master.
    pub on_mesh_master_changed: mxr_device_uid_cb,
    /// The device reported its view of the mesh topology; read it with
    /// `mxr_topology()`.
    pub on_topology_changed: mxr_device_cb,
    /// A ProAmp8 reported its Dolby settings; read them with
    /// `mxr_dolby_settings()`.
    pub on_amp_dolby_settings_changed: mxr_device_cb,
    /// Installer setup was completed or cleared.
    pub on_setup_status_changed: mxr_device_bool_cb,
    /// The installer identifier changed.
    pub on_installer_id_changed: mxr_device_u16_cb,
    /// The sink was told to show a window; read it with
    /// `mxr_v2ip_tiling()`.
    pub on_tiling_changed: mxr_device_cb,
    /// A source bay's remote-control configuration changed; read it with
    /// `mxr_rc_settings()`.
    pub on_rc_settings_changed: mxr_device_cb,
    /// A V2IP device was linked to a remote peer.
    pub on_v2ip_link_changed: mxr_device_uid_cb,
    /// A multiviewer command arrived.
    pub on_multiviewer_command: mxr_multiviewer_command_cb,
    /// An audio endpoint was switched to a new source.
    pub on_audio_select_input: mxr_audio_select_cb,
    /// An audio endpoint was muted or unmuted.
    pub on_audio_endpoint_mute: mxr_endpoint_bool_cb,
    /// An audio endpoint's trigger changed.
    pub on_audio_endpoint_trigger: mxr_endpoint_bool_cb,
    /// An audio endpoint's volume changed.
    pub on_audio_endpoint_volume: mxr_endpoint_u32_cb,
    /// A peer asked every device to announce itself.
    pub on_discover_request: mxr_device_cb,
    /// A peer asked a device to switch a sink.
    pub on_set_route_requested: mxr_set_route_cb,
    /// A peer asked a device for its EDID.
    pub on_edid_requested: mxr_edid_request_cb,
    /// A device answered with its EDID.
    pub on_edid_received: mxr_edid_record_cb,
    /// A peer asked a device to rename a bay.
    pub on_bay_name_change_requested: mxr_bay_name_change_cb,
    /// A peer asked a device to switch its EDID profile.
    pub on_edid_profile_change_requested: mxr_edid_profile_change_cb,
    /// A peer asked a device to reboot. The second identifier is the device
    /// being asked, which is not always the sender.
    pub on_reboot_requested: mxr_device_uid_cb,
    /// A peer asked devices to factory-reset.
    pub on_factory_reset_requested: mxr_factory_reset_cb,
    /// A device sent its monitoring pulse.
    pub on_monitoring_pulse: mxr_device_cb,
    /// A peer asked a device to upgrade its FPGA.
    pub on_upgrade_fpga_requested: mxr_device_cb,
    /// A peer asked a device to re-detect its bays.
    pub on_detect_bays_requested: mxr_device_cb,
    /// A peer asked a sink to enter or leave power save.
    pub on_power_save_requested: mxr_power_save_cb,
    /// A peer asked a device to send a remote-control key.
    pub on_key_transmit_requested: mxr_key_transmit_cb,
    /// A peer asked a device to perform a remote-control action.
    pub on_action_transmit_requested: mxr_action_transmit_cb,
    /// A peer asked a device to blast raw infrared.
    pub on_ir_transmit_requested: mxr_ir_transmit_cb,
    /// A device was added to or removed from the source blacklist.
    pub on_blacklist_changed: mxr_blacklist_cb,
    /// A video wall command arrived.
    pub on_video_wall_command: mxr_video_wall_cb,

    /// A bay was seen for the first time.
    pub on_bay_registered: mxr_bay_cb,
    /// The bay's routed video source changed, zero when it was unrouted.
    pub on_video_source_changed: mxr_bay_bay_cb,
    /// The bay's routed audio source changed, zero when it was unrouted.
    pub on_audio_source_changed: mxr_bay_bay_cb,
    /// The bay's volume or mute state changed.
    pub on_volume_changed: mxr_volume_cb,
    /// The attached device's power state changed.
    pub on_power_changed: mxr_power_cb,
    /// The bay was renamed.
    pub on_name_changed: mxr_bay_str_cb,
    /// A signal appeared or disappeared.
    pub on_signal_detected_changed: mxr_bay_bool_cb,
    /// The bay started or stopped reporting a fault.
    pub on_faulty_changed: mxr_bay_bool_cb,
    /// The bay was hidden or shown.
    pub on_hidden_changed: mxr_bay_bool_cb,
    /// Power over Ethernet started or stopped supplying the bay.
    pub on_poe_powered_changed: mxr_bay_bool_cb,
    /// The HDBaseT link came up or went down.
    pub on_hdbt_connected_changed: mxr_bay_bool_cb,
    /// The signal format description changed.
    pub on_signal_type_changed: mxr_bay_str_cb,
    /// Hot-plug detect was asserted or released.
    pub on_hpd_detected_changed: mxr_bay_bool_cb,
    /// A CEC device answered or stopped answering.
    pub on_cec_detected_changed: mxr_bay_bool_cb,
    /// The audio return channel changed.
    pub on_arc_changed: mxr_arc_cb,
    /// The input's EDID profile changed.
    pub on_edid_profile_changed: mxr_bay_u16_cb,
    /// The input's remote-control type changed.
    pub on_rc_type_changed: mxr_bay_u8_cb,
    /// A remote-control key was pressed on the bay.
    pub on_key_pressed: mxr_bay_u16_cb,
    /// A remote-control action was received on the bay.
    pub on_action_received: mxr_bay_u16_cb,
    /// The bay started or stopped mirroring another output, zero when it
    /// stopped.
    pub on_mirror_status_changed: mxr_bay_bay_cb,
    /// A ProAmp8 zone's settings changed; read them with
    /// `mxr_bay_amp_settings()`.
    pub on_amp_zone_settings_changed: mxr_bay_cb,
    /// A volume step was requested on the bay.
    pub on_volume_step: mxr_bay_bool_cb,
    /// The bay detected audio clipping, at the reported level.
    pub on_audio_clip: mxr_bay_u8_cb,
    /// Raw infrared was captured on the bay.
    pub on_ir_captured: mxr_ir_capture_cb,
    /// The devices filtered out of this sink's picker changed; read them with
    /// `mxr_bay_filtered()`.
    pub on_filtered_devices_changed: mxr_bay_cb,
    /// The audio endpoint the bay carries changed.
    pub on_audio_endpoint_changed: mxr_bay_u8_cb,
    /// The bay's V2IP encoder was enabled or disabled.
    pub on_encoder_disabled_changed: mxr_bay_bool_cb,
    /// The bay's V2IP decoder was enabled or disabled.
    pub on_decoder_disabled_changed: mxr_bay_bool_cb,

    /// The bay was linked to a bay on another device. Both ends are told, so
    /// both fire: `bay_name` names the bay whose link record changed, which is
    /// this bay on the device that reported the change and the far bay on its
    /// peer.
    pub on_bay_linked: mxr_bay_linked_cb,
    /// The bay's link to another device was removed. The arguments describe
    /// the link that went, and mean what they do on `on_bay_linked`.
    pub on_bay_unlinked: mxr_bay_unlinked_cb,
}

/// The caller's cookie, which this library carries and never reads.
///
/// # Safety
///
/// The contract on `mxr_remote_new()` is what makes
/// these impls sound: the caller keeps the pointer valid, and safe to use from
/// the library's threads, until the client is freed. Nothing here dereferences
/// it.
struct UserData(*mut c_void);

// SAFETY: an opaque pointer this library only ever copies. See the type's own
// docs for the contract the caller holds up.
unsafe impl Send for UserData {}
// SAFETY: as above.
unsafe impl Sync for UserData {}

/// Turns library events into calls through a C table.
pub(crate) struct Bridge {
    cb: mxr_callbacks_t,
    userdata: UserData,
}

impl Bridge {
    /// Copies the caller's table, so a caller may free or reuse theirs.
    pub(crate) fn new(table: &mxr_callbacks_t, userdata: *mut c_void) -> Self {
        Self {
            cb: *table,
            userdata: UserData(userdata),
        }
    }

    fn ud(&self) -> *mut c_void {
        self.userdata.0
    }
}

/// Calls one member of the table, if the caller set it.
///
/// The guard is here rather than at the caller because this is an exit point,
/// not an entry one: a panic while marshalling would otherwise unwind through
/// the receive thread and take the client down with it.
macro_rules! forward {
    ($self:ident.$field:ident( $($arg:expr),* $(,)? )) => {
        if let Some(f) = $self.cb.$field {
            guard((), || f($self.ud() $(, $arg)*));
        }
    };
}

/// Calls `body` with `text` as a C string that lives for the call.
fn with_cstr<R>(text: &str, body: impl FnOnce(*const c_char) -> R) -> R {
    // A NUL inside would cut the string short in C. Protocol strings are read
    // up to their first NUL, so this is a fallback, not a case that happens.
    let owned = CString::new(text).unwrap_or_else(|_| c"".to_owned());
    body(owned.as_ptr())
}

/// The identifier a request names, where the protocol's zero stands for
/// "unaddressed": a broadcast, or a request that addresses only its sender.
fn uid_or_zero(uid: Option<DeviceUid>) -> mxr_uid_t {
    uid.unwrap_or(DeviceUid::ZERO).into()
}

impl EventHandler for Bridge {
    fn on_device_update(&self, device: DeviceUid) {
        forward!(self.on_device_update(device.into()));
    }

    fn on_bay_update(&self, bay: BayUid) {
        forward!(self.on_bay_update(bay.into()));
    }

    // ---- device ----

    fn on_device_config_changed(&self, device: DeviceUid) {
        forward!(self.on_device_config_changed(device.into()));
    }

    fn on_device_config_complete(&self, device: DeviceUid) {
        forward!(self.on_device_config_complete(device.into()));
    }

    fn on_device_online_changed(&self, device: DeviceUid, online: bool) {
        forward!(self.on_device_online_changed(device.into(), online));
    }

    fn on_device_temperature_changed(&self, device: DeviceUid, _temperatures: Vec<u8>) {
        forward!(self.on_device_temperature_changed(device.into()));
    }

    fn on_firmware_version_changed(&self, device: DeviceUid, _version: mx_remote::FirmwareVersion) {
        forward!(self.on_firmware_version_changed(device.into()));
    }

    fn on_system_status_changed(&self, device: DeviceUid, status: u16, message: String) {
        with_cstr(&message, |m| {
            forward!(self.on_system_status_changed(device.into(), status, m));
        });
    }

    fn on_network_status_changed(&self, device: DeviceUid, _status: mx_remote::NetworkPortStatus) {
        forward!(self.on_network_status_changed(device.into()));
    }

    fn on_v2ip_stats_changed(&self, device: DeviceUid, _stats: mx_remote::V2ipDeviceStats) {
        forward!(self.on_v2ip_stats_changed(device.into()));
    }

    fn on_v2ip_sources_changed(
        &self,
        device: DeviceUid,
        _sources: Vec<mx_remote::V2ipStreamSources>,
    ) {
        forward!(self.on_v2ip_sources_changed(device.into()));
    }

    fn on_v2ip_details_changed(&self, device: DeviceUid, _details: mx_remote::DeviceV2ipDetails) {
        forward!(self.on_v2ip_details_changed(device.into()));
    }

    fn on_v2ip_sink_changed(&self, device: DeviceUid, _sink: mx_remote::DeviceV2ipSink) {
        forward!(self.on_v2ip_sink_changed(device.into()));
    }

    fn on_multiviewer_status_changed(
        &self,
        device: DeviceUid,
        _status: mx_remote::MultiviewerStatus,
    ) {
        forward!(self.on_multiviewer_status_changed(device.into()));
    }

    fn on_audio_endpoints_changed(&self, device: DeviceUid, _endpoints: mx_remote::AudioEndpoints) {
        forward!(self.on_audio_endpoints_changed(device.into()));
    }

    fn on_mesh_master_changed(&self, device: DeviceUid, master: DeviceUid) {
        forward!(self.on_mesh_master_changed(device.into(), master.into()));
    }

    fn on_topology_changed(&self, device: DeviceUid, _topology: Vec<mx_remote::TopologyEntry>) {
        forward!(self.on_topology_changed(device.into()));
    }

    fn on_amp_dolby_settings_changed(
        &self,
        device: DeviceUid,
        _settings: mx_remote::AmpDolbySettings,
    ) {
        forward!(self.on_amp_dolby_settings_changed(device.into()));
    }

    fn on_setup_status_changed(&self, device: DeviceUid, completed: bool) {
        forward!(self.on_setup_status_changed(device.into(), completed));
    }

    fn on_installer_id_changed(&self, device: DeviceUid, installer_id: u16) {
        forward!(self.on_installer_id_changed(device.into(), installer_id));
    }

    fn on_tiling_changed(&self, device: DeviceUid, _tiling: mx_remote::V2ipTilingConfig) {
        forward!(self.on_tiling_changed(device.into()));
    }

    fn on_rc_settings_changed(&self, device: DeviceUid, _settings: mx_remote::RcSettings) {
        forward!(self.on_rc_settings_changed(device.into()));
    }

    fn on_v2ip_link_changed(&self, device: DeviceUid, target: DeviceUid) {
        forward!(self.on_v2ip_link_changed(device.into(), target.into()));
    }

    fn on_multiviewer_command(&self, device: DeviceUid, command: MultiviewerCommand) {
        let payload = mxr_multiviewer_command_t {
            target: command.target.into(),
            op: command.op,
            params: command.params.as_ptr(),
            params_len: command.params.len(),
        };
        forward!(self.on_multiviewer_command(device.into(), &payload));
    }

    fn on_audio_select_input(&self, device: DeviceUid, change: AudioChangeSource) {
        let payload = mxr_audio_change_source_t {
            source_uid: change.source_uid.into(),
            source_id: change.source_id,
            target_uid: change.target_uid.into(),
            target_id: change.target_id,
        };
        forward!(self.on_audio_select_input(device.into(), &payload));
    }

    fn on_audio_endpoint_mute(&self, device: DeviceUid, endpoint: u16, muted: bool) {
        forward!(self.on_audio_endpoint_mute(device.into(), endpoint, muted));
    }

    fn on_audio_endpoint_trigger(&self, device: DeviceUid, endpoint: u16, active: bool) {
        forward!(self.on_audio_endpoint_trigger(device.into(), endpoint, active));
    }

    fn on_audio_endpoint_volume(&self, device: DeviceUid, endpoint: u16, volume: u32) {
        forward!(self.on_audio_endpoint_volume(device.into(), endpoint, volume));
    }

    fn on_discover_request(&self, device: DeviceUid) {
        forward!(self.on_discover_request(device.into()));
    }

    fn on_set_route_requested(&self, device: DeviceUid, request: SetRouteRequest) {
        with_cstr(&request.serial, |serial| {
            let payload = mxr_set_route_request_t {
                serial,
                sink_bay: request.sink_bay,
                source_bay: request.source_bay,
                no_power_on: request.no_power_on,
                audio_only: request.audio_only,
            };
            forward!(self.on_set_route_requested(device.into(), &payload));
        });
    }

    fn on_edid_requested(&self, device: DeviceUid, request: EdidRequest) {
        let payload = mxr_edid_request_t {
            target: request.target.into(),
            output: request.output,
        };
        forward!(self.on_edid_requested(device.into(), &payload));
    }

    fn on_edid_received(&self, device: DeviceUid, edid: EdidRecord) {
        let payload = mxr_edid_record_t {
            output: edid.output,
            data: edid.data.as_ptr(),
            data_len: edid.data.len(),
        };
        forward!(self.on_edid_received(device.into(), &payload));
    }

    fn on_bay_name_change_requested(&self, device: DeviceUid, change: BayNameChange) {
        with_cstr(&change.name, |name| {
            let payload = mxr_bay_name_change_t {
                target: change.target.into(),
                port: change.port,
                name,
            };
            forward!(self.on_bay_name_change_requested(device.into(), &payload));
        });
    }

    fn on_edid_profile_change_requested(&self, device: DeviceUid, change: EdidProfileChange) {
        let payload = mxr_edid_profile_change_t {
            target: change.target.into(),
            profile: change.profile.to_wire(),
        };
        forward!(self.on_edid_profile_change_requested(device.into(), &payload));
    }

    fn on_reboot_requested(&self, device: DeviceUid, request: RebootRequest) {
        forward!(self.on_reboot_requested(device.into(), request.target.into()));
    }

    fn on_factory_reset_requested(&self, device: DeviceUid, request: FactoryResetRequest) {
        let payload = mxr_factory_reset_request_t {
            all: request.all,
            target: uid_or_zero(request.target),
        };
        forward!(self.on_factory_reset_requested(device.into(), &payload));
    }

    fn on_monitoring_pulse(&self, device: DeviceUid) {
        forward!(self.on_monitoring_pulse(device.into()));
    }

    fn on_upgrade_fpga_requested(&self, device: DeviceUid) {
        forward!(self.on_upgrade_fpga_requested(device.into()));
    }

    fn on_detect_bays_requested(&self, device: DeviceUid) {
        forward!(self.on_detect_bays_requested(device.into()));
    }

    fn on_power_save_requested(&self, device: DeviceUid, request: V2ipPowerSaveRequest) {
        let payload = mxr_power_save_request_t {
            target: uid_or_zero(request.target),
            enabled: request.enabled,
        };
        forward!(self.on_power_save_requested(device.into(), &payload));
    }

    fn on_key_transmit_requested(&self, device: DeviceUid, request: KeyTransmitRequest) {
        let payload = mxr_key_transmit_request_t {
            target: request.target.into(),
            local_bay: request.local_bay,
            key: request.key.to_wire(),
        };
        forward!(self.on_key_transmit_requested(device.into(), &payload));
    }

    fn on_action_transmit_requested(&self, device: DeviceUid, request: ActionTransmitRequest) {
        let payload = mxr_action_transmit_request_t {
            target: request.target.into(),
            local_bay: request.local_bay,
            action: request.action.to_wire(),
        };
        forward!(self.on_action_transmit_requested(device.into(), &payload));
    }

    fn on_ir_transmit_requested(&self, device: DeviceUid, request: IrTransmitRequest) {
        let payload = mxr_ir_transmit_request_t {
            target: request.target.into(),
            local_mode: request.local_mode,
            local_bay: request.local_bay,
            timestamp: request.timestamp,
            meta: request.meta.into(),
            timings: request.timings.as_ptr(),
            timings_len: request.timings.len(),
        };
        forward!(self.on_ir_transmit_requested(device.into(), &payload));
    }

    fn on_blacklist_changed(&self, device: DeviceUid, change: V2ipBlacklistChange) {
        let payload = mxr_blacklist_change_t {
            target: change.target.into(),
            registered: change.registered,
        };
        forward!(self.on_blacklist_changed(device.into(), &payload));
    }

    fn on_video_wall_command(&self, device: DeviceUid, command: VideoWallCommand) {
        let payload = mxr_video_wall_command_t {
            target: command.target.into(),
            pos_x: command.pos_x,
            pos_y: command.pos_y,
            width: command.width,
            height: command.height,
            raster_w: command.raster_w,
            raster_h: command.raster_h,
            op: command.op.to_wire(),
        };
        forward!(self.on_video_wall_command(device.into(), &payload));
    }

    // ---- bay ----

    fn on_bay_registered(&self, bay: BayUid) {
        forward!(self.on_bay_registered(bay.into()));
    }

    fn on_video_source_changed(&self, bay: BayUid, source: Option<BayUid>) {
        forward!(self.on_video_source_changed(bay.into(), bay_or_zero(source)));
    }

    fn on_audio_source_changed(&self, bay: BayUid, source: Option<BayUid>) {
        forward!(self.on_audio_source_changed(bay.into(), bay_or_zero(source)));
    }

    fn on_volume_changed(&self, bay: BayUid, volume: VolumeMuteStatus) {
        forward!(self.on_volume_changed(bay.into(), volume.volume(), volume.muted().into()));
    }

    fn on_power_changed(&self, bay: BayUid, power: PowerStatus) {
        forward!(self.on_power_changed(bay.into(), Some(power).into()));
    }

    fn on_name_changed(&self, bay: BayUid, name: String) {
        with_cstr(&name, |n| forward!(self.on_name_changed(bay.into(), n)));
    }

    fn on_signal_detected_changed(&self, bay: BayUid, detected: bool) {
        forward!(self.on_signal_detected_changed(bay.into(), detected));
    }

    fn on_faulty_changed(&self, bay: BayUid, faulty: bool) {
        forward!(self.on_faulty_changed(bay.into(), faulty));
    }

    fn on_hidden_changed(&self, bay: BayUid, hidden: bool) {
        forward!(self.on_hidden_changed(bay.into(), hidden));
    }

    fn on_poe_powered_changed(&self, bay: BayUid, powered: bool) {
        forward!(self.on_poe_powered_changed(bay.into(), powered));
    }

    fn on_hdbt_connected_changed(&self, bay: BayUid, connected: bool) {
        forward!(self.on_hdbt_connected_changed(bay.into(), connected));
    }

    fn on_signal_type_changed(&self, bay: BayUid, signal_type: String) {
        with_cstr(&signal_type, |s| {
            forward!(self.on_signal_type_changed(bay.into(), s))
        });
    }

    fn on_hpd_detected_changed(&self, bay: BayUid, detected: bool) {
        forward!(self.on_hpd_detected_changed(bay.into(), detected));
    }

    fn on_cec_detected_changed(&self, bay: BayUid, detected: bool) {
        forward!(self.on_cec_detected_changed(bay.into(), detected));
    }

    fn on_arc_changed(&self, bay: BayUid, arc: ArcStatus) {
        forward!(self.on_arc_changed(bay.into(), arc.into()));
    }

    fn on_edid_profile_changed(&self, bay: BayUid, profile: mx_remote::EdidProfile) {
        forward!(self.on_edid_profile_changed(bay.into(), profile.to_wire()));
    }

    fn on_rc_type_changed(&self, bay: BayUid, rc_type: mx_remote::RcType) {
        forward!(self.on_rc_type_changed(bay.into(), rc_type.to_wire()));
    }

    fn on_key_pressed(&self, bay: BayUid, key: RcKey) {
        forward!(self.on_key_pressed(bay.into(), key.to_wire()));
    }

    fn on_action_received(&self, bay: BayUid, action: RcAction) {
        forward!(self.on_action_received(bay.into(), action.to_wire()));
    }

    fn on_mirror_status_changed(&self, bay: BayUid, mirror: mx_remote::BayMirrorStatus) {
        forward!(self.on_mirror_status_changed(bay.into(), bay_or_zero(mirror.target)));
    }

    fn on_amp_zone_settings_changed(&self, bay: BayUid, _settings: mx_remote::AmpZoneSettings) {
        forward!(self.on_amp_zone_settings_changed(bay.into()));
    }

    fn on_volume_step(&self, bay: BayUid, up: bool) {
        forward!(self.on_volume_step(bay.into(), up));
    }

    fn on_audio_clip(&self, bay: BayUid, clip: AudioClip) {
        forward!(self.on_audio_clip(bay.into(), clip.clip));
    }

    fn on_ir_captured(&self, bay: BayUid, capture: IrCapture) {
        let payload = mxr_ir_capture_t {
            timestamp: capture.timestamp,
            last_change: capture.last_change,
            meta: capture.meta.into(),
            timings: capture.timings.as_ptr(),
            timings_len: capture.timings.len(),
        };
        forward!(self.on_ir_captured(bay.into(), &payload));
    }

    fn on_filtered_devices_changed(&self, bay: BayUid, _filtered: Vec<DeviceUid>) {
        forward!(self.on_filtered_devices_changed(bay.into()));
    }

    fn on_audio_endpoint_changed(&self, bay: BayUid, endpoint: u8) {
        forward!(self.on_audio_endpoint_changed(bay.into(), endpoint));
    }

    fn on_encoder_disabled_changed(&self, bay: BayUid, disabled: bool) {
        forward!(self.on_encoder_disabled_changed(bay.into(), disabled));
    }

    fn on_decoder_disabled_changed(&self, bay: BayUid, disabled: bool) {
        forward!(self.on_decoder_disabled_changed(bay.into(), disabled));
    }

    // ---- both ----

    fn on_bay_linked(
        &self,
        bay: BayUid,
        linked_serial: String,
        bay_name: String,
        features: LinkFeature,
    ) {
        with_cstr(&linked_serial, |serial| {
            with_cstr(&bay_name, |name| {
                forward!(self.on_bay_linked(bay.into(), serial, name, features.bits()));
            });
        });
    }

    fn on_bay_unlinked(&self, bay: BayUid, linked_serial: String, bay_name: String) {
        with_cstr(&linked_serial, |serial| {
            with_cstr(&bay_name, |name| {
                forward!(self.on_bay_unlinked(bay.into(), serial, name));
            });
        });
    }
}
