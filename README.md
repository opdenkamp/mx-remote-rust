# mx-remote

A client for [Pulse-Eight](https://www.pulse-eight.com/) AV distribution
hardware: video and audio matrices, HDMI-over-IP encoders and decoders,
multiviewers and 8-zone amplifiers, all driven over the local network. It
covers discovery, video and audio routing, volume, remote-control key
passthrough, HDMI-over-IP streaming and multiviewer control.

If you want to drive Pulse-Eight **neo**, **OneIP** or **ProAmp8** hardware from
your own software or from a home automation system, this is the library for it.

## What is MX Remote?

MX Remote is the protocol these devices use to discover and control one another
over UDP, by multicast or by broadcast. They all run the same **MatrixOS**
firmware, which speaks it natively. This library is a client implementation of
that protocol, written to open the devices up to third-party software.

Devices announce themselves and their bays, report signal, audio, streaming and
power state as it changes, and accept routing and configuration commands. The
library discovers them, keeps a snapshot of what they have reported, and sends
those commands.

## Supported devices

- **[neo](https://www.pulse-eight.com/)**: HDBaseT video and audio matrices. The
  neo:4, neo:8 and neo:X, and the splitters.
- **[OneIP](https://www.pulse-eight.com/p/248/oneip-tx)**: HDMI-over-IP units.
  Transmitter (TX), Receiver (RX), Transceiver (TZ) and Multiviewer.
- **[ProAmp8](https://www.pulse-eight.com/p/219/proamp-8)**: an 8-zone audio
  amplifier with Dolby decoding.

## Three ways to use it

All three sit on the same core:

| Consumer | What you get                                     |
| -------- | ------------------------------------------------ |
| Rust     | the `mx-remote` crate                            |
| C        | `libmx_remote_ffi.a` and `include/mx_remote.h`   |
| C++      | the same archive, plus `include/mx_remote.hpp`   |

## Other languages

Two other clients speak the same protocol, each in its own repository:

- **Python**: <https://github.com/opdenkamp/mx-remote>, the oldest of the
  three.
- **Go**: <https://github.com/opdenkamp/mx-remote-golang>, the one this client
  was ported from.

Both carry more of the older opcodes than this client does. The three are
independent implementations rather than bindings over a shared core.

## Rust

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
the time it is built; the example fills the client in before starting, which is
before anything can call back.

Events say *what* moved, and the snapshot beside them says what it moved to.
Handler methods run on the receive thread, so they should return quickly.

`cargo run --example discover` is the same program, complete.

## C

```bash
cargo build -p mx-remote-ffi --release
cc -Iinclude prog.c target/release/libmx_remote_ffi.a -lpthread -ldl -lm
```

The archive has no runtime to initialise, takes no signal handlers from the
host process, and pulls in nothing but libc and pthreads.

```c
#include <mx_remote.h>

/* The client cannot be its own userdata: it does not exist when the table is
 * handed over, so it reaches the callbacks through a struct that does. */
struct app { mxr_remote_t *remote; };

static void on_device_update(void *userdata, mxr_uid_t device) {
    mxr_device_info_t info;
    if (mxr_device(((struct app *)userdata)->remote, device, &info) == MXR_OK)
        printf("%s %s\n", info.model, info.name);
}

int main(void) {
    struct app app = {0};
    mxr_callbacks_t cb = {0};
    cb.on_device_update = on_device_update;

    /* Zeroing a config asks for every default; NULL does the same. */
    app.remote = mxr_remote_new(NULL, &cb, &app);
    mxr_remote_start(app.remote);
    /* ... */
    mxr_remote_free(app.remote);
    return 0;
}
```

Three conventions run through the header, and knowing them is most of knowing
the API:

- **A device is addressed by value.** There is no handle for a device or a bay:
  a device is an `mxr_uid_t`, a bay is an `mxr_bay_uid_t`, and state is read by
  passing one in and having a caller-owned struct filled out.
- **Every call returns, whatever happens.** A panic becomes `MXR_ERR_PANIC`;
  nothing unwinds across the boundary.
- **A borrowed pointer lives for one call.** Strings and arrays handed to a
  callback point into memory the library owns and reuses.

Every failure code is negative and `MXR_OK` is zero, so `if (rc < 0)` is a
complete test.

`include/mx_remote.h` is generated from the Rust source by
`scripts/gen-header.sh` and checked in; CI fails if regenerating it produces a
diff.

## C++

`include/mx_remote.hpp` is a header-only layer over the same archive: a
move-only `mxr::Remote` that closes and joins in its destructor, `mxr::Uid` and
`mxr::BayUid` value types, and an `mxr::Handler` base class with a virtual
method per event.

```cpp
#include <mx_remote.hpp>

class Printer : public mxr::Handler {
public:
    mxr::Remote *remote = nullptr;

    void on_device_update(mxr::Uid device) override {
        mxr_device_info_t info;
        if (mxr_device(remote->get(), device, &info) == MXR_OK)
            std::cout << info.model << ' ' << info.name << '\n';
    }
};

Printer printer;                          // declared first, destroyed last
mxr::Remote remote = mxr::Remote::open(nullptr, &printer);
printer.remote = &remote;
remote.start();
```

Declare the handler before the client: the client's destructor joins the
threads that call into the handler, so the handler has to be destroyed second.

`examples/c` and `examples/cpp` hold the complete versions of both, built by
`make -C examples`.

## Requirements

- Rust 1.79 or newer, edition 2021. CI builds on that version and on stable.
- Linux, macOS or Windows. Selecting an interface that has no address of its
  own, such as a tagged VLAN, is Linux-only.
- For the C and C++ headers: any C99 and C++11 compiler.

## Layout

```
mx-remote/          the core crate: wire format, runtime, control surface
mx-remote-ffi/      the C ABI over it, and the only unsafe in the workspace
include/            the generated C header and the hand-written C++ one
examples/           the C and C++ examples; the Rust one is a cargo example
scripts/            gen-header.sh, which regenerates include/mx_remote.h
```

## Building

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

./scripts/gen-header.sh && git diff --exit-code include/mx_remote.h
cargo build -p mx-remote-ffi && make -C examples
```

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
