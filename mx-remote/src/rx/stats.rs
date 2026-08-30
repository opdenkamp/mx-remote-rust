// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! V2IP encoder and decoder statistics.

use crate::event::Event;
use crate::state::State;
use crate::types::{V2ipDecoderState, V2ipDeviceStats, V2ipRxStats, V2ipTxStats};

use super::handlers::{byte, u32_at};
use super::Rx;

/// Block sizes of the statistics payload.
///
/// The transmit and receive blocks are 20 and 44 rather than 24 and 48 because
/// their alignment attribute sits ahead of the `struct` keyword, where GCC
/// ignores it. The 128-byte total is therefore stable by accident: correcting
/// those declarations would shift every block after the first while changing
/// nothing a reader of the header could detect, so pin the sizes rather than
/// only the field offsets.
const TX_STATS_SIZE: usize = 20;
const RX_STATS_SIZE: usize = 44;
const STATS_SIZE: usize = 2 * TX_STATS_SIZE + 2 * RX_STATS_SIZE;

/// The payload length of an enable/disable request, which carries no
/// statistics of its own.
const STATS_REQUEST_SIZE: usize = 17;

fn tx_stats(p: &[u8]) -> V2ipTxStats {
    V2ipTxStats {
        video: u32_at(p, 0),
        audio: u32_at(p, 4),
        anc: u32_at(p, 8),
        stream_down: u32_at(p, 12),
        overflow: u32_at(p, 16),
    }
}

fn rx_stats(p: &[u8]) -> V2ipRxStats {
    V2ipRxStats {
        video_total: u32_at(p, 0),
        video_dropped: u32_at(p, 4),
        video_seq_errors: u32_at(p, 8),
        wdt_timeout: u32_at(p, 12),
        audio_total: u32_at(p, 16),
        audio_dropped: u32_at(p, 20),
        audio_seq_errors: u32_at(p, 24),
        anc_total: u32_at(p, 28),
        anc_dropped: u32_at(p, 32),
        anc_seq_errors: u32_at(p, 36),
        decoder_state: V2ipDecoderState::from_wire(byte(p, 40)),
    }
}

pub(super) fn v2ip_stats(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() == STATS_REQUEST_SIZE || p.len() < STATS_SIZE {
        return;
    }
    let rx_base = 2 * TX_STATS_SIZE;
    let stats = V2ipDeviceStats {
        tx: tx_stats(&p[0..TX_STATS_SIZE]),
        tx_per_minute: tx_stats(&p[TX_STATS_SIZE..rx_base]),
        rx: rx_stats(&p[rx_base..rx_base + RX_STATS_SIZE]),
        rx_per_minute: rx_stats(&p[rx_base + RX_STATS_SIZE..rx_base + 2 * RX_STATS_SIZE]),
    };
    if let Some(device) = state.device_mut(rx.sender()) {
        device.set_v2ip_stats(stats, ev);
    }
}
