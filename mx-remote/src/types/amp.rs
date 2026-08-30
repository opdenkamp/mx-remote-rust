// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! ProAmp8 zone and Dolby settings.

/// Number of EQ bands a zone carries (`AMP_DOLBYEQBANDS_MAX`).
pub const AMP_EQ_BANDS: usize = 5;

/// The neutral value for bass, treble and the EQ bands.
///
/// These are raw unsigned bytes; the firmware header describes a signed range,
/// which is wrong at both ends.
pub const AMP_TONE_FLAT: u8 = 128;

/// Lowest tone value the amp's own HTTP API accepts.
///
/// Nothing enforces this here: the mesh receive path copies these bytes
/// straight through without a range check, so a peer can put any value on the
/// wire and the amp will take it. This library reports what arrived. Clamping
/// to these bounds imposes one device's HTTP policy on a mesh peer - reasonable,
/// but a caller's decision to make knowingly.
pub const AMP_TONE_HTTP_MIN: u8 = 104;

/// Highest tone value the amp's own HTTP API accepts. See [`AMP_TONE_HTTP_MIN`].
pub const AMP_TONE_HTTP_MAX: u8 = 140;

/// A ProAmp8 zone's gain, delay, tone and power settings.
///
/// Gains and volume limits run 0-248 in 0.5dB steps, with 200 as 0dB. Gains are
/// `int16` inside the amp but a byte on the mesh, so a frame cannot carry the
/// amp's full internal range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmpZoneSettings {
    /// Left channel gain.
    pub gain_left: u8,
    /// Right channel gain.
    pub gain_right: u8,
    /// Lowest volume the zone may be set to.
    pub volume_min: u8,
    /// Highest volume the zone may be set to.
    pub volume_max: u8,
    /// Left channel delay, in 1/48000 second increments.
    pub delay_left: u32,
    /// Right channel delay, in 1/48000 second increments.
    pub delay_right: u32,
    /// Bass tone control, neutral at [`AMP_TONE_FLAT`].
    pub bass: u8,
    /// Treble tone control, neutral at [`AMP_TONE_FLAT`].
    pub treble: u8,
    /// 0 = normal, 1 = bridged.
    pub bridged: u8,
    /// Power-on mode.
    pub power_mode: u8,
    /// Signal level that switches the zone on automatically.
    pub power_level: u8,
    /// Idle time before the zone powers down, in seconds.
    pub power_timeout: u32,
    /// Left channel EQ, from 100Hz to 10KHz, neutral at [`AMP_TONE_FLAT`].
    pub eq_left: [u8; AMP_EQ_BANDS],
    /// Right channel EQ, from 100Hz to 10KHz, neutral at [`AMP_TONE_FLAT`].
    pub eq_right: [u8; AMP_EQ_BANDS],
}

impl Default for AmpZoneSettings {
    fn default() -> Self {
        Self {
            gain_left: 0,
            gain_right: 0,
            volume_min: 0,
            volume_max: 0,
            delay_left: 0,
            delay_right: 0,
            bass: AMP_TONE_FLAT,
            treble: AMP_TONE_FLAT,
            bridged: 0,
            power_mode: 0,
            power_level: 0,
            power_timeout: 0,
            eq_left: [AMP_TONE_FLAT; AMP_EQ_BANDS],
            eq_right: [AMP_TONE_FLAT; AMP_EQ_BANDS],
        }
    }
}

/// The Dolby processing state of a ProAmp8.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AmpDolbySettings {
    /// 0 = standard, 1 = 3-zone Dolby, 2 = 4-zone Dolby.
    pub mode: u8,
    /// Whether PCM is up-mixed to 5.1 rather than passed through.
    pub pcm_upmix: bool,
    /// Whether a Dolby stream was detected.
    pub dolby_detected: bool,
    /// Whether up-mixing is currently running.
    pub pcm_upmix_active: bool,
}
