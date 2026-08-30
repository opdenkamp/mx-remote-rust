# mx-remote

A client for [Pulse-Eight](https://www.pulse-eight.com/) AV distribution
hardware: neo HDBaseT matrices, OneIP HDMI-over-IP units and multiviewers, and
the ProAmp8 8-zone amplifier. They all run the same MatrixOS firmware and speak
one protocol, MX Remote, over UDP multicast or broadcast.

Devices announce themselves and their bays, report signal, audio, streaming and
power state as it changes, and accept routing and configuration commands. This
crate discovers them, keeps a snapshot of what they have reported, and sends
those commands: video and audio routing, volume, remote-control key
passthrough, HDMI-over-IP streaming and multiviewer control.

```rust
use std::sync::{Arc, OnceLock};

use mx_remote::{Config, DeviceUid, EventHandler, Remote};

static CLIENT: OnceLock<Arc<Remote>> = OnceLock::new();

struct Printer;

impl EventHandler for Printer {
    fn on_device_update(&self, device: DeviceUid) {
        let Some(info) = CLIENT.get().and_then(|c| c.device(device)) else {
            return;
        };
        println!("{device} {} {}", info.model, info.name);
    }
}

let remote = Arc::new(Remote::new(Config::default(), Arc::new(Printer))?);
let _ = CLIENT.set(Arc::clone(&remote));
remote.start()?;
```

A handler is handed to the client that will call it, so it cannot hold one at
the time it is built; the example above fills the client in before starting,
which is before anything can call back.

Events say *what* moved, and the snapshot beside them says what it moved to.
Handler methods run on the receive thread, so they should return quickly.

`cargo run --example discover` is the same program, complete.

## Values arrive as they were sent

Every enumeration is a newtype over its wire integer with named constants
rather than a closed set, so a value from firmware newer than this crate
reaches the caller unchanged instead of being folded onto a neighbour. An
`Option` is a field the device has not reported yet, which is a different
answer from one reported as zero or false.

## Requirements

Rust 1.79 or newer, edition 2021. No async runtime: a receive thread and a
timer thread.

Linux, macOS and Windows. Selecting an interface that has no address of its
own, such as a tagged VLAN, is Linux-only.

## C and C++

The `mx-remote-ffi` crate is a C ABI over this one, with a generated C header
and a header-only C++ layer beside it.

## Other languages

Two other clients speak the same protocol, independently of this crate:
[Python](https://github.com/opdenkamp/mx-remote), the oldest of the three, and
[Go](https://github.com/opdenkamp/mx-remote-golang), the one this crate was
ported from. Both carry more of the older opcodes than this crate does.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
