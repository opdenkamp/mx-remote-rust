// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Discovers MX Remote devices and prints what they report.
//!
//! The shape here is the one most programs want: two handler methods that say
//! only which device or bay moved, and a snapshot read back for the detail.
//! Both run on the library's receive thread, so what they do is kept short.
//!
//! Usage: `cargo run --example discover [interface-address]`

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mx_remote::{BayUid, Config, DeviceUid, EventHandler, Remote};

/// The client the snapshots are read from.
///
/// A handler is handed to the client that will call it, so it cannot hold one
/// at the time it is built. It is filled in before the client is started,
/// which is before anything can call back.
static CLIENT: OnceLock<Arc<Remote>> = OnceLock::new();

struct Printer;

impl EventHandler for Printer {
    fn on_device_update(&self, device: DeviceUid) {
        let Some(remote) = CLIENT.get() else { return };
        let Some(info) = remote.device(device) else {
            return;
        };
        println!(
            "  {device}  {:<16} {:<12} {:<16} protocol {:#04x}, {} bays{}",
            info.model,
            info.serial,
            info.name,
            info.supported_protocol,
            info.bays.len(),
            if info.online { "" } else { " (offline)" },
        );
    }

    fn on_bay_update(&self, bay: BayUid) {
        let Some(remote) = CLIENT.get() else { return };
        let Some(info) = remote.bay(bay) else { return };
        println!(
            "    bay {} {:<16} {}",
            bay.port,
            info.user_name,
            match info.signal_detected {
                Some(true) => "signal",
                _ => "no signal",
            },
        );
    }
}

fn main() -> std::io::Result<()> {
    let mut config = Config::default();
    config.name = Some("discover".to_owned());
    if let Some(address) = std::env::args().nth(1) {
        config.local_ip = Some(address.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not an IPv4 address")
        })?);
    }

    let remote = Arc::new(Remote::new(config, Arc::new(Printer))?);
    let _ = CLIENT.set(Arc::clone(&remote));
    remote.start()?;

    if let Some(target) = remote.target() {
        println!("listening, sending to {target}. Ctrl-C to stop.");
    }

    let running = Arc::new(AtomicBool::new(true));
    let stop = Arc::clone(&running);
    ctrl_c(move || stop.store(false, Ordering::Relaxed));
    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }

    let devices = remote.devices();
    println!("\n{} device(s):", devices.len());
    for device in devices {
        Printer.on_device_update(device);
    }

    remote.close();
    Ok(())
}

/// Runs `f` on the first Ctrl-C.
///
/// The library takes no signal handler of its own - it is meant to link into a
/// host program that has its own - so an example that wants one installs it.
fn ctrl_c(f: impl FnOnce() + Send + 'static) {
    static HANDLER: OnceLock<()> = OnceLock::new();
    let _ = HANDLER.set(());
    std::thread::spawn(move || {
        let mut line = String::new();
        // Reading to end of input is the portable stand-in for a signal
        // handler: Ctrl-C closes the terminal's read on most shells, and
        // Ctrl-D ends it everywhere.
        let _ = std::io::stdin().read_line(&mut line);
        f();
    });
}
