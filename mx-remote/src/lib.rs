// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Client library for Pulse-Eight MatrixOS devices (neo matrices, OneIP/V2IP
//! units, ProAmp8 amplifiers) over UDP multicast/broadcast.
//!
//! Devices announce themselves and their bays, report signal, audio, streaming
//! and power state as it changes, and accept routing and configuration
//! commands. A [`Remote`] discovers them, keeps a snapshot of what they have
//! reported, and sends those commands.
//!
//! # Getting started
//!
//! ```no_run
//! use std::sync::{Arc, OnceLock};
//!
//! use mx_remote::{Config, DeviceUid, EventHandler, Remote};
//!
//! // A handler is handed to the client that will call it, so it cannot hold
//! // one at the time it is built. It is filled in before the client starts,
//! // which is before anything can call back.
//! static CLIENT: OnceLock<Arc<Remote>> = OnceLock::new();
//!
//! struct Printer;
//!
//! impl EventHandler for Printer {
//!     fn on_device_update(&self, device: DeviceUid) {
//!         let Some(info) = CLIENT.get().and_then(|c| c.device(device)) else {
//!             return;
//!         };
//!         println!("{device} {} {}", info.model, info.name);
//!     }
//! }
//!
//! let remote = Arc::new(Remote::new(Config::default(), Arc::new(Printer))?);
//! let _ = CLIENT.set(Arc::clone(&remote));
//! remote.start()?;
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! [`Config::default`] discovers over multicast on the interface the host
//! picks, which is the right answer on a single-homed machine and arbitrary on
//! any other. `cargo run --example discover` is the program above, complete.
//!
//! # Events say what moved
//!
//! Every method on [`EventHandler`] has a default that does nothing, so a
//! caller implements only what it acts on. Most carry just the identifier of
//! the device or bay that changed: the snapshot read back from [`Remote`] is
//! the same state the event was derived from, and can only be fresher.
//! [`EventHandler::on_device_update`] and [`EventHandler::on_bay_update`] fire
//! after every event at their level, which is enough for a caller that only
//! wants to know that something moved.
//!
//! Handler methods run on the receive thread, so they should return quickly.
//!
//! # Values arrive as they were sent
//!
//! Every enumeration here is a newtype over its wire integer with named
//! constants rather than a closed set, so a value from firmware newer than
//! this library reaches the caller unchanged instead of being folded onto a
//! neighbour.
//!
//! An [`Option`] on a snapshot is a field the device has not reported yet,
//! which is a different answer from one reported as zero or false. Zero is a
//! valid reading for most of them, so a confidently wrong value would be worse
//! than an absent one.
//!
//! # Threads
//!
//! [`Remote::start`] takes a receive thread and a timer thread, and
//! [`Remote::close`] - or dropping the `Remote` - stops and joins them. There
//! is no async runtime.

mod event;
mod runtime;

mod rx;
mod state;
#[cfg(test)]
mod testing;
mod types;
mod wire;

pub use event::{Event, EventHandler};
pub use runtime::{BayInfo, Config, ControlError, DeviceInfo, Remote};
pub use rx::{lookup_svd, Svd};
pub use types::*;
pub use wire::*;
