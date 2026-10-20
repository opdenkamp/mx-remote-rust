// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Sending an overlay sync, and answering the units that take it.
//!
//! A sync announces what it copies and how many settings it has. Each unit
//! fetches the logo images, then asks the source for the settings it lacks,
//! once a second until it has them all, and takes none it did not ask for.
//! So the settings go out only in answer to a request, a few at a time: a
//! unit drops one that arrives while it is still writing earlier ones, and
//! asks for it again.
//!
//! While this client holds a sync it also announces itself as the mesh's
//! overlay sync manager. A unit that wakes from power save, or that is told
//! to pull, then asks this client for a new sync instead of the controller.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::types::{
    OsdSyncContent, OsdSyncEntry, OsdSyncImage, OsdSyncRequest, OsdSyncSlot, OsdSyncValue,
    OSD_SYNC_FONT_MAX, OSD_SYNC_IMAGE_HEIGHT_MAX, OSD_SYNC_IMAGE_MAX, OSD_SYNC_IMAGE_WIDTH_MAX,
    OSD_SYNC_PERCENT_MAX, OSD_SYNC_RECORDS_MAX, OSD_SYNC_SLOTS, OSD_SYNC_TEXT_MAX,
};
use crate::wire::{
    build_osd_setting, build_osd_sync, crc32, op, Addressee, DeviceFeature, DeviceUid, OsdSyncWhat,
};

use super::control::ControlError;
use super::logo_server::{Images, LogoServer};
use super::{lock, Remote, Shared, SHUTDOWN_POLL};

/// How many syncs of the held content a request may still name. Each unit
/// that asks for a new sync is sent one of its own; a request naming one
/// that has aged out is answered with another.
const SEQS_KEPT: usize = 64;

/// How many settings go out together, and the gap after them, which a unit
/// keeps up with.
const SETTINGS_PER_PASS: usize = 4;
const PASS_GAP: Duration = Duration::from_millis(14);

/// The bytes every PNG opens with, and where its header gives the size.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_IHDR_AT: usize = 12;

/// What this client sends and answers.
pub(super) struct OsdSender {
    held: Option<Held>,
    next_seq: u16,
    queue: VecDeque<Pending>,
    server: Option<LogoServer>,
}

/// The sync this client holds.
struct Held {
    /// What it copies, with [`OsdSyncWhat::LOGO_SLOTS`] when it has images.
    what: OsdSyncWhat,
    entries: Arc<[OsdSyncEntry]>,
    slots: [OsdSyncSlot; OSD_SYNC_SLOTS],
    http_port: u16,
    /// The syncs sent of it, oldest first.
    seqs: VecDeque<u16>,
}

/// A setting a unit asked for, not yet sent.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Pending {
    unit: DeviceUid,
    seq: u16,
    index: u16,
}

impl OsdSender {
    /// A sender numbering from the wall clock, so a restarted client does not
    /// reuse a recent number: requests and reports name a sync by its number,
    /// and a unit drops a sync repeating the last one's within two seconds.
    pub(super) fn new() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        Self {
            held: None,
            next_seq: (ms as u16).max(1),
            queue: VecDeque::new(),
            server: None,
        }
    }

    /// Whether a sync is held, which is when this client is the manager.
    pub(super) fn holds(&self) -> bool {
        self.held.is_some()
    }

    /// Numbers a new sync of the held content, copying `what`, which is
    /// some of what the content copies. Zero means "no sync", so the count
    /// skips it.
    fn sync_frame(&mut self, target: DeviceUid, what: OsdSyncWhat) -> Option<(u16, Vec<u8>)> {
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1).max(1);
        let held = self.held.as_mut()?;
        if held.seqs.len() == SEQS_KEPT {
            held.seqs.pop_front();
        }
        held.seqs.push_back(seq);
        let records = held.entries.len() as u16;
        let frame = build_osd_sync(seq, target, what, held.http_port, records, &held.slots);
        Some((seq, frame))
    }

    /// Replaces what is still to be sent to `unit` with the settings it
    /// lacks of sync `seq`.
    fn queue_missing(&mut self, unit: DeviceUid, request: &OsdSyncRequest) {
        let Some(held) = &self.held else {
            return;
        };
        self.queue.retain(|p| p.unit != unit);
        for index in 0..held.entries.len() as u16 {
            if request.is_missing(index) {
                self.queue.push_back(Pending {
                    unit,
                    seq: request.seq,
                    index,
                });
            }
        }
    }

    /// The next few settings to send, as their unit and frame.
    ///
    /// The units take turns: a unit's other settings go behind everyone
    /// else's each time one of its own is taken, so one asking for many does
    /// not hold the rest past the time a unit allows a sync.
    fn next_pass(&mut self) -> Vec<(DeviceUid, Vec<u8>)> {
        let mut pass = Vec::new();
        while pass.len() < SETTINGS_PER_PASS {
            let Some(p) = self.queue.pop_front() else {
                break;
            };
            let (theirs, others): (VecDeque<_>, VecDeque<_>) =
                self.queue.drain(..).partition(|q| q.unit == p.unit);
            self.queue = others;
            self.queue.extend(theirs);
            let Some(held) = &self.held else {
                break;
            };
            // A request for a sync since replaced, or aged out, is answered
            // no further: the unit asks again and gets a new sync.
            let Some(entry) = held.entries.get(usize::from(p.index)) else {
                continue;
            };
            if !held.seqs.contains(&p.seq) {
                continue;
            }
            let records = held.entries.len() as u16;
            pass.push((
                p.unit,
                build_osd_setting(p.seq, p.unit, p.index, records, entry),
            ));
        }
        pass
    }
}

impl Shared {
    fn addressee(&self, unit: DeviceUid) -> Option<Addressee> {
        self.read(|state| state.device(unit).map(|d| Addressee::device(d)))
    }

    /// Answers the overlay sync requests a frame raised.
    ///
    /// One asking for a new sync is sent one of its own, copying what it
    /// asks for of what this client holds. One naming a sync of the held
    /// content has the settings it lacks queued, in place of what was still
    /// queued for it. One naming any other sync of this client's is sent a
    /// new sync the same way, since the unit can take no other.
    pub(super) fn answer_osd_requests(&self, requests: Vec<OsdSyncRequest>) {
        for request in requests {
            let unit = request.target;
            let Some(to) = self.addressee(unit) else {
                continue;
            };
            let sync = {
                let mut osd = lock(&self.osd);
                let Some(held) = &osd.held else {
                    return;
                };
                let (held_what, current) = (held.what, held.seqs.contains(&request.seq));
                if request.seq != 0 && current {
                    osd.queue_missing(unit, &request);
                    self.osd_queued.notify_one();
                    None
                } else {
                    // A new sync copies what the request asks for, of what
                    // is held; a request asking for none of it gets none.
                    let what = request.what & held_what;
                    if what.is_empty() {
                        continue;
                    }
                    osd.sync_frame(unit, what)
                }
            };
            if let Some((_, frame)) = sync {
                let _ = self.send(&to, op::V2IP_OSD_SYNC, &frame);
            }
        }
    }

    /// Sends the next few settings units asked for, returning how many.
    pub(super) fn osd_pass(&self) -> usize {
        let pass = lock(&self.osd).next_pass();
        for (unit, frame) in &pass {
            if let Some(to) = self.addressee(*unit) {
                let _ = self.send(&to, op::V2IP_OSD_SYNC, frame);
            }
        }
        pass.len()
    }

    /// Sends settings as units ask for them, until the client is closing.
    pub(super) fn osd_loop(&self) {
        while !self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            if self.osd_pass() > 0 {
                std::thread::sleep(PASS_GAP);
                continue;
            }
            let osd = lock(&self.osd);
            if osd.queue.is_empty() {
                let _ = self.osd_queued.wait_timeout(osd, SHUTDOWN_POLL);
            }
        }
    }

    /// Drops the held sync and stops serving its images.
    pub(super) fn drop_osd_sync(&self) -> bool {
        let (held, server) = {
            let mut osd = lock(&self.osd);
            (osd.held.take().is_some(), osd.server.take())
        };
        // Joined outside the lock: the server thread never takes it, but a
        // join is no place to hold one.
        drop(server);
        held
    }
}

impl Remote {
    /// Has every unit copy `content`, and keeps it in place, returning the
    /// sync's number.
    ///
    /// Units take a sync from the mesh controller or a management
    /// application, which this client announces itself as. Each unit
    /// fetches the images from a server this client runs, then asks for the
    /// settings, which this client answers until the content is replaced or
    /// [`Remote::clear_osd_sync`]. Meanwhile it announces itself as the
    /// mesh's overlay sync manager, so a unit waking from power save takes
    /// this content too, rather than the controller's. The controller can
    /// still send its own over it; the last sync a unit took is what it
    /// shows. Each unit's
    /// [`Remote::osd_sync_report`] says how it went.
    ///
    /// Refused for content copying nothing, with more than
    /// [`OSD_SYNC_RECORDS_MAX`] settings, a caption font over
    /// [`OSD_SYNC_FONT_MAX`] or text over [`OSD_SYNC_TEXT_MAX`] bytes, a logo
    /// other than -1 to 4, or an image that is not a PNG within
    /// [`OSD_SYNC_IMAGE_MAX`] bytes and [`OSD_SYNC_IMAGE_WIDTH_MAX`] by
    /// [`OSD_SYNC_IMAGE_HEIGHT_MAX`] pixels, shown at over
    /// [`OSD_SYNC_PERCENT_MAX`] percent. Refused content leaves the held sync
    /// as it was; content that fails to go out leaves none held.
    pub fn send_osd_sync(&self, content: OsdSyncContent) -> Result<u16, ControlError> {
        validate(&content)?;
        self.hold_osd_sync(content)
            .inspect_err(|_| self.clear_osd_sync())
    }

    fn hold_osd_sync(&self, content: OsdSyncContent) -> Result<u16, ControlError> {
        let images = content.images.as_ref();
        let mut what =
            OsdSyncWhat::from_bits(content.what.bits() & !OsdSyncWhat::LOGO_SLOTS.bits());
        if images.is_some() {
            what |= OsdSyncWhat::LOGO_SLOTS;
        }
        let slots = images.map_or_else(Default::default, |images| {
            core::array::from_fn(|i| OsdSyncSlot {
                bytes: images[i].png.len() as u32,
                crc32: crc32(&images[i].png),
                percent: images[i].percent,
            })
        });
        let (was_held, sync) = {
            let mut osd = lock(&self.shared.osd);
            let http_port = match images {
                Some(images) => serve(&mut osd.server, self.shared.logo_port, images)?,
                None => 0,
            };
            let was_held = osd.held.is_some();
            osd.held = Some(Held {
                what,
                entries: content.entries.into(),
                slots,
                http_port,
                seqs: VecDeque::new(),
            });
            (was_held, osd.sync_frame(DeviceUid::ZERO, what))
        };
        let (seq, frame) = sync.ok_or(ControlError::InvalidRequest("no overlay sync is held"))?;
        // Announced first, so the controller has stood back by the time a
        // unit deferring this sync wakes and asks for it.
        if !was_held {
            self.shared.announce();
        }
        self.shared
            .send(&Addressee::Broadcast, op::V2IP_OSD_SYNC, &frame)?;
        Ok(seq)
    }

    /// Has one unit copy the sync this client holds again, returning the
    /// new sync's number. As [`Remote::send_osd_sync`].
    ///
    /// Refused while no sync is held, and for a unit that does not announce
    /// [`OSD_SYNC`](DeviceFeature::OSD_SYNC), as one that does not take
    /// overlay syncs.
    pub fn resend_osd_sync(&self, device: DeviceUid) -> Result<u16, ControlError> {
        let to = self
            .shared
            .addressee(device)
            .ok_or(ControlError::UnknownDevice(device))?;
        let takes = self.shared.read(|state| {
            state
                .device(device)
                .is_some_and(|d| d.hello.features.has(DeviceFeature::OSD_SYNC))
        });
        if !takes {
            return Err(ControlError::Unsupported("the unit takes no overlay sync"));
        }
        let (seq, frame) = {
            let mut osd = lock(&self.shared.osd);
            let what = osd.held.as_ref().map(|h| h.what);
            what.and_then(|what| osd.sync_frame(device, what))
                .ok_or(ControlError::InvalidRequest("no overlay sync is held"))?
        };
        self.shared.send(&to, op::V2IP_OSD_SYNC, &frame)?;
        Ok(seq)
    }

    /// Stops holding the sync this client sent, and hands the overlay sync
    /// manager's part back to the controller.
    ///
    /// Units keep what they took. One still taking it fails, as its
    /// requests go unanswered.
    pub fn clear_osd_sync(&self) {
        if self.shared.drop_osd_sync() {
            self.shared.announce();
        }
    }
}

/// Starts the image server if it is not running, and serves `images`,
/// returning its port.
fn serve(
    server: &mut Option<LogoServer>,
    port: Option<u16>,
    images: &[OsdSyncImage; OSD_SYNC_SLOTS],
) -> Result<u16, ControlError> {
    let running = match server.take() {
        Some(running) => running,
        None => LogoServer::start(port.unwrap_or(0))
            .map_err(|e| ControlError::Send(crate::wire::SendError::Io(e)))?,
    };
    let images: Images = core::array::from_fn(|i| Arc::from(images[i].png.as_slice()));
    running.set_images(images);
    let port = running.port();
    *server = Some(running);
    Ok(port)
}

fn validate(content: &OsdSyncContent) -> Result<(), ControlError> {
    let invalid = |what| Err(ControlError::InvalidRequest(what));
    let copies = OsdSyncWhat::from_bits(content.what.bits() & !OsdSyncWhat::LOGO_SLOTS.bits());
    if copies.is_empty() && content.images.is_none() {
        return invalid("an overlay sync copying something");
    }
    if content.entries.len() > OSD_SYNC_RECORDS_MAX {
        return invalid("an overlay sync of at most 128 settings");
    }
    for entry in &content.entries {
        match &entry.value {
            OsdSyncValue::Caption(c) if c.font.len() > OSD_SYNC_FONT_MAX => {
                return invalid("a caption font of at most 31 bytes");
            }
            OsdSyncValue::Caption(c) if c.text.len() > OSD_SYNC_TEXT_MAX => {
                return invalid("a caption text of at most 360 bytes");
            }
            OsdSyncValue::Logo(l) if !(-1..=4).contains(&l.logo) => {
                return invalid("a logo of -1 to 4");
            }
            _ => {}
        }
    }
    for image in content.images.iter().flatten() {
        if image.percent > OSD_SYNC_PERCENT_MAX {
            return invalid("a logo size of 0 to 100 percent");
        }
        if !image.png.is_empty() && !fits(&image.png) {
            return invalid("a PNG of at most 512 KB and 1920 by 1080 pixels");
        }
    }
    Ok(())
}

/// Whether `png` is a PNG a unit takes, by its size and the header's.
fn fits(png: &[u8]) -> bool {
    let be32 = |at: usize| {
        png.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (Some(width), Some(height)) = (be32(PNG_IHDR_AT + 4), be32(PNG_IHDR_AT + 8)) else {
        return false;
    };
    png.len() <= OSD_SYNC_IMAGE_MAX
        && png.starts_with(PNG_SIGNATURE)
        && png.get(PNG_IHDR_AT..PNG_IHDR_AT + 4) == Some(b"IHDR")
        && (1..=OSD_SYNC_IMAGE_WIDTH_MAX).contains(&width)
        && (1..=OSD_SYNC_IMAGE_HEIGHT_MAX).contains(&height)
}

#[cfg(test)]
#[path = "osd_tests.rs"]
mod tests;
