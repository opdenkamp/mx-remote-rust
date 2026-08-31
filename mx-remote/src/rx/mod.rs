// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The receive path: one handler per opcode, and the parsers they use.
//!
//! A handler mutates the registry and appends to the event queue. It never
//! dispatches: the queue is drained once the caller has released the state
//! lock.

mod amp;
mod audio;
mod commands;
mod handlers;
mod multiviewer;
mod network;
mod stats;
mod svd;

#[cfg(test)]
mod tests;

use std::net::Ipv4Addr;
use std::time::Instant;

use crate::event::Event;
use crate::state::State;
use crate::wire::{op, DeviceUid, Frame, Opcode};

pub use svd::{lookup_svd, Svd};

/// One received frame, with what the socket knew about it.
pub(crate) struct Rx<'a> {
    pub(crate) frame: Frame<'a>,
    /// Where the datagram came from, when the caller knew.
    pub(crate) address: Option<Ipv4Addr>,
    pub(crate) timestamp: Instant,
}

impl Rx<'_> {
    /// The device that sent this frame.
    pub(crate) fn sender(&self) -> DeviceUid {
        self.frame.remote_id()
    }

    /// A uid field, reading an absent or truncated one as the zero uid.
    ///
    /// Zero is the marker several payloads use for "not carried", so a
    /// truncated frame reaching the same conclusion is the intended reading.
    pub(crate) fn uid_or_zero(&self, idx: usize) -> DeviceUid {
        self.frame.uid(idx).unwrap_or(DeviceUid::ZERO)
    }
}

/// Decodes one datagram into events, updating the registry.
///
/// `timestamp` is when the datagram arrived, which is what the sending device
/// is recorded as last heard from. A frame that does not parse, or that this
/// client sent itself, produces nothing.
pub(crate) fn process_frame(
    state: &mut State,
    data: &[u8],
    address: Option<Ipv4Addr>,
    timestamp: Instant,
) -> Vec<Event> {
    let Ok(frame) = Frame::parse(data) else {
        return Vec::new();
    };
    let sender = frame.remote_id();
    if sender == state.uid {
        return Vec::new();
    }
    // Counted before the opcode is looked at, and only for frames some other
    // sender put on the wire. This client's own multicast is looped back by
    // the host whichever interface was selected, so counting that would answer
    // "did anything reach this interface" with yes on every interface.
    state.frames_received = state.frames_received.saturating_add(1);
    let rx = Rx {
        frame,
        address,
        timestamp,
    };
    let mut ev = Vec::new();
    dispatch(state, &rx, &mut ev);
    // Any frame from a known device proves it is alive. Online detection must
    // not rest on hello frames alone: a V2IP device is considered offline
    // after fifteen seconds of silence but announces itself only every thirty.
    if let Some(device) = state.device_mut(sender) {
        device.touch(timestamp, Instant::now(), &mut ev);
    }
    ev
}

/// Routes a frame to the handler for its opcode.
fn dispatch(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    match rx.frame.opcode() {
        op::SYS_HELLO => handlers::hello(state, rx, ev),
        op::SYS_DISCOVER => commands::discover_request(state, rx, ev),
        op::SYS_BAY_CONFIG | op::SYS_BAY_CONFIG_SECONDARY => handlers::bay_config(state, rx, ev),
        op::SYS_LINKS => handlers::links(state, rx, ev),
        op::DEV_CONNECT => handlers::connect_status(state, rx, ev),
        op::DEV_POWER_CHANGE => handlers::power_change(state, rx, ev),
        op::DEV_EDID => commands::edid(state, rx, ev),
        op::MX_ROUTE => handlers::routing_change(state, rx, ev),
        op::MX_SET_ROUTE => commands::set_route(state, rx, ev),
        op::RC_IR => commands::ir_capture(state, rx, ev),
        op::RC_KEY => handlers::rc_key(state, rx, ev),
        op::RC_TX_KEY => commands::tx_key(state, rx, ev),
        op::RC_ACTION => handlers::rc_action(state, rx, ev),
        op::RC_TX_ACTION => commands::tx_action(state, rx, ev),
        op::AUDIO_VOLUME_UP => commands::volume_step(state, rx, ev, true),
        op::AUDIO_VOLUME_DOWN => commands::volume_step(state, rx, ev, false),
        op::AUDIO_CLIP => commands::audio_clip(state, rx, ev),
        op::AUDIO_VOLUME_MUTE => commands::volume_mute(state, rx, ev),
        op::AUDIO_SET_ROUTE => commands::audio_set_route(state, rx, ev),
        op::AUDIO_SET_VOLUME => handlers::volume_set(state, rx, ev),
        op::SYS_TEMPERATURE => handlers::temperature(state, rx, ev),
        op::PDU_STATE => commands::pdu_state(state, rx, ev),
        op::V2IP_SOURCE_SWITCH => handlers::v2ip_source_switch(state, rx, ev),
        op::V2IP_LINK_REMOTE => commands::v2ip_link_remote(state, rx, ev),
        op::V2IP_DETECT_BAYS => commands::detect_bays(state, rx, ev),
        op::CHANGE_BAY_NAME => commands::change_bay_name(state, rx, ev),
        op::V2IP_MANUAL_SRC_SWITCH => handlers::v2ip_manual_source_switch(state, rx, ev),
        op::SYS_BAY_V2IP_SOURCES => handlers::v2ip_sources(state, rx, ev),
        op::BAY_HIDE => handlers::bay_hide(state, rx, ev),
        op::SYS_REBOOT => commands::reboot(state, rx, ev),
        op::NET_LINK_STATUS => network::network_status(state, rx, ev),
        op::FIRMWARE_VERSION => handlers::firmware_version(state, rx, ev),
        op::SYS_MONITORING_PULSE => commands::monitoring_pulse(state, rx, ev),
        op::V2IP_UPGRADE_FPGA => commands::upgrade_fpga(state, rx, ev),
        op::V2IP_BLIST_REGISTER => commands::blacklist(state, rx, ev, true),
        op::V2IP_BLIST_UNREGISTER => commands::blacklist(state, rx, ev, false),
        op::TOPOLOGY => handlers::topology(state, rx, ev),
        op::BAY_SIGNAL_STATUS => svd::signal_status(state, rx, ev),
        op::BAY_MIRROR_STATUS => handlers::mirror_status(state, rx, ev),
        op::BAY_EDID_PROFILE => commands::edid_profile(state, rx, ev),
        op::SETUP_STATUS => commands::setup_status(state, rx, ev),
        op::SET_INSTALLER => commands::set_installer(state, rx, ev),
        op::BAY_FILTER_STATUS => commands::filter_status(state, rx, ev),
        op::BAY_STATUS => handlers::bay_status(state, rx, ev),
        op::SYS_FACTORY_RESET => commands::factory_reset(state, rx, ev),
        op::MESH_OPERATION => handlers::mesh_operation(state, rx, ev),
        op::V2IP_DEVICE_CFG => handlers::v2ip_device_configuration(state, rx, ev),
        op::AMP_ZONE_SETTINGS => amp::zone_settings(state, rx, ev),
        op::AMP_DOLBY_STATE => amp::dolby_settings(state, rx, ev),
        op::V2IP_STATS => stats::v2ip_stats(state, rx, ev),
        op::V2IP_TILING => commands::v2ip_tiling(state, rx, ev),
        op::V2IP_POWER_SAVE => commands::v2ip_power_save(state, rx, ev),
        op::V2IP_MULTIVIEWER => multiviewer::multiviewer(state, rx, ev),
        op::V2IP_AUDIO => audio::audio(state, rx, ev),
        op::V2IP_BAY_MAPPINGS => handlers::v2ip_bay_mapping(state, rx, ev),
        op::RC_SETTINGS => commands::rc_settings(state, rx, ev),
        op::SYS_STATUS => handlers::system_status(state, rx, ev),
        op::RC_IR_TX => commands::ir_transmit(state, rx, ev),
        op::V2IP_VIDEO_WALL => commands::video_wall(state, rx, ev),
        Opcode(_) => {}
    }
}
