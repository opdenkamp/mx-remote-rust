// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! When to announce this client, and when to ask the network to describe
//! itself again.

use std::time::{Duration, Instant};

use super::DISCOVER_INTERVAL;

/// Announcement cadence, matching the firmware's own: a 2.5s base plus up to
/// 2.5s of jitter, re-drawn after each send so a mesh full of clients does not
/// fall into step.
///
/// The probe thread ticks once a second, so the interval actually observed is
/// the draw rounded up to the next tick - effectively 3, 4 or 5 seconds rather
/// than a continuous 2.5 to 5. Coarser than the firmware, and left that way:
/// the jitter exists to stop announcers colliding, and three values across
/// clients whose ticks start at different moments is enough for that.
pub(super) const HELLO_BASE: Duration = Duration::from_millis(2500);

/// The width of the announcement jitter. See [`HELLO_BASE`].
pub(super) const HELLO_JITTER: Duration = Duration::from_millis(2500);

/// Draws the next announcement interval, in `HELLO_BASE..=HELLO_BASE + HELLO_JITTER`.
///
/// A draw that cannot be made falls in the middle of the range, which still
/// announces on time and only gives up the collision avoidance.
pub(super) fn next_hello_interval() -> Duration {
    let mut bytes = [0u8; 2];
    if getrandom::getrandom(&mut bytes).is_err() {
        return HELLO_BASE + HELLO_JITTER / 2;
    }
    let fraction = u64::from(u16::from_be_bytes(bytes));
    let jitter = HELLO_JITTER.as_nanos().saturating_mul(u128::from(fraction)) / 65536;
    HELLO_BASE + Duration::from_nanos(jitter as u64)
}

/// The two timers the background thread drives.
#[derive(Debug)]
pub(super) struct Schedule {
    /// When this client last announced itself, or `None` before it ever has.
    last_hello: Option<Instant>,
    hello_interval: Duration,
    last_discover: Option<Instant>,
}

impl Schedule {
    pub(super) fn new() -> Self {
        Self {
            last_hello: None,
            hello_interval: next_hello_interval(),
            last_discover: None,
        }
    }

    /// Whether the announcement interval has elapsed. A client that has never
    /// announced is always due.
    pub(super) fn announce_due(&self, now: Instant) -> bool {
        self.last_hello.map_or(true, |last| {
            now.saturating_duration_since(last) >= self.hello_interval
        })
    }

    /// Records an announcement and draws the next interval.
    pub(super) fn announced(&mut self, now: Instant) {
        self.last_hello = Some(now);
        self.hello_interval = next_hello_interval();
    }

    pub(super) fn discover_due(&self, now: Instant) -> bool {
        self.last_discover.map_or(true, |last| {
            now.saturating_duration_since(last) >= DISCOVER_INTERVAL
        })
    }

    pub(super) fn discovered(&mut self, now: Instant) {
        self.last_discover = Some(now);
    }

    /// The announcement timer as it stands, for a caller checking whether a
    /// send re-armed it.
    #[cfg(test)]
    pub(super) fn hello_timer(&self) -> (Option<Instant>, Duration) {
        (self.last_hello, self.hello_interval)
    }

    /// Rewinds the announcement timer, so the next tick announces.
    #[cfg(test)]
    pub(super) fn set_hello_timer(&mut self, last: Option<Instant>, interval: Duration) {
        self.last_hello = last;
        self.hello_interval = interval;
    }
}
