// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Short Video Descriptors and the detailed signal report that names one.

use core::fmt;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::event::Event;
use crate::state::State;
use crate::types::{BayAudioDetails, BaySignalDetails};
use crate::wire::{BayStatus, BayUid, MxrSignalType};

use super::handlers::{u16_at, u32_at};
use super::Rx;

/// The CTA-861 short video descriptor table, one line per descriptor.
const SVD_TABLE: &str = include_str!("../svd.csv");

/// A Short Video Descriptor: one standard video resolution and timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Svd {
    /// The descriptor's CTA-861 id.
    pub id: u16,
    /// Picture aspect ratio code.
    pub picture_aspect: u16,
    /// Pixel aspect ratio code.
    pub pixel_aspect: u16,
    /// Active pixels per line.
    pub horizontal_active: u16,
    /// Total pixels per line, including blanking.
    pub horizontal_total: u16,
    /// Active lines per frame.
    pub vertical_active: u16,
    /// Total lines per frame, including blanking.
    pub vertical_total: u16,
    /// Refresh rate in Hz.
    pub refresh: u16,
    /// Whether the format is interlaced.
    pub interlaced: bool,
    /// Pixel clock multiplier.
    pub multiplier: u16,
}

impl fmt::Display for Svd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{}@{}Hz",
            self.horizontal_active, self.vertical_active, self.refresh
        )
    }
}

fn table() -> &'static HashMap<u16, Svd> {
    static TABLE: OnceLock<HashMap<u16, Svd>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for line in SVD_TABLE.lines() {
            let fields: Vec<u16> = line
                .trim()
                .split(';')
                .filter_map(|f| f.trim().parse().ok())
                .collect();
            let Ok(n) = <[u16; 10]>::try_from(fields.as_slice()) else {
                continue;
            };
            map.insert(
                n[0],
                Svd {
                    id: n[0],
                    picture_aspect: n[1],
                    pixel_aspect: n[2],
                    horizontal_active: n[3],
                    horizontal_total: n[4],
                    vertical_active: n[5],
                    vertical_total: n[6],
                    refresh: n[7],
                    interlaced: n[8] == 1,
                    multiplier: n[9],
                },
            );
        }
        map
    })
}

/// Looks up the Short Video Descriptor with the given id.
pub fn lookup_svd(id: u16) -> Option<Svd> {
    table().get(&id).copied()
}

/// Names the colour space a signal report carries.
fn colour_space(v: u8) -> &'static str {
    match v {
        0 => "RGB",
        1 => "4:4:4",
        2 => "4:2:2",
        3 => "4:2:0",
        _ => "unknown",
    }
}

/// `av_details` wire layout, packed:
///
/// ```text
/// 0..8     header
/// 8..24    AVI infoframe
/// 24..40   audio
/// 40..56   video
/// 56..88   vsync
/// 88..100  HDMI link errors
/// 100..112 bay
/// ```
const AV_DETAILS_SIZE: usize = 112;

/// Bits of the stream-flags byte.
const STREAM_INTERLACED: u8 = 1 << 1;
const STREAM_NON_INTEGER_CLOCK: u8 = 1 << 3;
const STREAM_HDR: u8 = 1 << 4;

/// How firmware describes a bay with nothing on it, in the same field this
/// library fills in from a signal report.
const NO_SIGNAL: &str = "no signal";

/// Where the timing block gives the active width and height.
const VSYNC_H_ACTIVE: usize = 58;
const VSYNC_V_ACTIVE: usize = 68;

/// What a device calls a stream it detects but cannot take.
const INVALID_SIGNAL: &str = "invalid signal";

/// The support flag bit marking a stream as detected.
const SUPPORT_STREAM_DETECTED: u8 = 1 << 0;

/// The support-flags bit that says the stream block holds a real signal.
const SUPPORT_STREAM_VALID: u8 = 1 << 1;

/// The support-flags bit that says the source sent an audio infoframe.
const SUPPORT_AUDIO_INFOFRAME: u8 = 1 << 4;

/// The support-flags bit that says the audio block was filled in.
const SUPPORT_AUDIO_VALID: u8 = 1 << 5;

/// Decodes the audio block of a report, `None` where it carried none.
fn audio_details(audio: &[u8], support_flags: u8) -> Option<BayAudioDetails> {
    if support_flags & SUPPORT_AUDIO_VALID == 0 {
        return None;
    }
    let infoframe = support_flags & SUPPORT_AUDIO_INFOFRAME != 0;
    // Compressed audio commonly leaves the stream's count at zero and the
    // infoframe's at "refer to the stream header", so the stream's count is
    // taken first and the infoframe's, CTA-861 CC at bits 2:0 of its first
    // data byte, stands in for it.
    let channels = match audio[11] {
        0 if infoframe => (audio[1] & 0x07) + 1,
        n => n,
    };
    Some(BayAudioDetails {
        format: audio[10],
        channels,
        sample_rate: u32_at(audio, 12),
        // Without an infoframe the whole field is zero, and zero is also a
        // coding type a source can claim, so absence is the honest answer.
        coding: infoframe.then(|| audio[1] >> 4),
    })
}

/// Decodes a detailed AV signal report.
///
/// A report is answered one packet per bay: the port number in the bay block
/// at the tail is what names the reporting bay, so demultiplex on it. Because
/// that block sits behind the vsync and link-error tail, a report shorter than
/// the full struct cannot be attributed to a bay at all and is dropped, as the
/// firmware does.
///
/// An empty payload is a broadcast request for every device to report, and a
/// 16-byte payload requests a report from the one unit it addresses.
pub(super) fn signal_status(state: &mut State, rx: &Rx<'_>, ev: &mut Vec<Event>) {
    let p = rx.frame.payload();
    if p.len() < AV_DETAILS_SIZE {
        return;
    }
    let support_flags = p[2];
    let stream_flags = p[3];
    let stream_detected = support_flags & SUPPORT_STREAM_DETECTED != 0;
    let stream_valid = support_flags & SUPPORT_STREAM_VALID != 0;

    let bay_block = &p[100..112];
    let port = u16_at(bay_block, 0);
    let bay = BayUid::new(rx.sender(), port);
    if state.bay(bay).is_none() {
        return;
    }

    let video = &p[40..56];
    let svd_id = u16::from(video[0]);
    let mut frame_rate = f64::from(u16_at(video, 8));
    if stream_flags & STREAM_NON_INTEGER_CLOCK != 0 {
        frame_rate = (frame_rate * 1000.0 / 1001.0 * 100.0).round() / 100.0;
    }

    // A stream with no mode in the CTA table is described by the active
    // area its timing measures, as a device describes it.
    let (h, v) = match lookup_svd(svd_id) {
        Some(svd) if svd_id != 0 => (svd.horizontal_active, svd.vertical_active),
        _ => (u16_at(p, VSYNC_H_ACTIVE), u16_at(p, VSYNC_V_ACTIVE)),
    };
    let signal_type = if !stream_detected {
        NO_SIGNAL.to_owned()
    } else if !stream_valid {
        INVALID_SIGNAL.to_owned()
    } else {
        let mut description = format!("{h}x{v} / {} / {}bpp", colour_space(video[1]), video[2]);
        if stream_flags & STREAM_INTERLACED != 0 {
            description.push_str(" interlaced");
        }
        if stream_flags & STREAM_HDR != 0 {
            description.push_str(" HDR");
        }
        description.push_str(&format!(" / {frame_rate}Hz"));
        description
    };

    let details = BaySignalDetails {
        frame_rate,
        tmds_clock: u32_at(video, 10),
        status: BayStatus::from_bits(u32_at(bay_block, 2)),
        scaling: MxrSignalType::from_wire(u16_at(bay_block, 6)),
        clock_rate: u32_at(bay_block, 8),
        audio: audio_details(&p[24..40], support_flags),
    };

    // The bay's status word is applied as a bay configuration's is, which is
    // what says whether a signal is detected.
    if let Some(bay) = state.bay_mut(bay) {
        bay.apply_bay_status(details.status, ev);
        bay.set_signal_details(details);
        bay.set_signal_type(signal_type, ev);
    }
}
