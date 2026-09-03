# AGENTS.md

## What this is

A Rust client library for Pulse-Eight MatrixOS devices (neo matrices, OneIP/V2IP
units, ProAmp8 amplifiers) over UDP multicast/broadcast.

It is meant to be *the* public implementation third parties integrate against: a
pure-Rust core in `mx-remote`, a C ABI over it in `mx-remote-ffi`, and further
language bindings layered on the same core. Rust was chosen for that reason — it
links into a C++ program as a plain static archive, with no runtime to
initialise, no GC pointer rules, and no signal handlers taken from the host
process.

## Sources of truth

The firmware defines the wire format and settles every disagreement with this
crate. It is closed source and no copy of it ships here, so a layout this
repository does not already encode has to be settled against a captured frame or
against the two public clients below — never guessed from field widths.

- Python — <https://github.com/opdenkamp/mx-remote>. The oldest and most mature,
  and where the byte-exact vectors in `wire/vectors.rs` came from.
- Go — <https://github.com/opdenkamp/mx-remote-golang>. The client this crate was
  ported from.

Both are independent implementations rather than bindings over a shared core.
Where a reference client and the firmware disagree, the firmware wins.

**Keep firmware internals out of this repository.** Unreleased behaviour,
internal build numbers and firmware-side implementation detail do not belong in
source, comments, tests or this file. Describe what is on the wire and what this
library does with it.

## Where the protocol knowledge lives

In the code, not in prose here:

- `wire/opcode.rs` — every opcode this library names, and the protocol version it
  stamps on each.
- `wire/frame.rs`, `wire/payload.rs` — the frame envelope, and every payload this
  library builds.
- `rx/` — one handler per opcode, each reading its fields at explicit offsets.
- `types/` — what a decoded value becomes for a caller.
- `wire/vectors.rs` — byte-exact frames, generated from the Python client rather
  than from this one, which is what makes them evidence.
- `rx/tests/` — what each handler is required to read, at which offset and width.

Read those before adding to this file. A layout restated in prose is a second
place to be wrong.

## Wire format

`[0x50, 0x38, protocol(u16 LE), uid(16), opcode(u16 LE), length(u16 LE), payload]`

**Little-endian throughout, with two exceptions that sit next to each other.** An
IPv4 address is stored in network byte order, so every stream slot puts a
big-endian address immediately in front of a little-endian port. Pin such a slot
with an address whose four bytes differ — a symmetric one survives being
reversed.

A uid is four 32-bit words and its printed form reverses each one, so parsing
that text as bytes yields a uid that fails a receiver's "is this me" test, and
the frame is dropped in silence like everything else on these paths.

The stamped protocol is the per-opcode version from `stamp_for`, not the version
this library speaks. A receiver drops any frame stamped above its own version.

## Working on the wire

**Decide a layout by payload length, never by the stamp.** A trailing field added
to an existing opcode is read from the length; the stamp is a version ceiling
rather than a layout selector. Where a handler does read the stamp, its opcode's
table entry is the number it tests.

**Check the addressee's reported version before sending, not just the stamp.** A
receiver drops a frame it cannot decode silently, with no NAK at any layer, so
the call would otherwise succeed while nothing happened. A device that has
reported no version is let through: not knowing is not knowing it is too old.

**Nothing on these paths is acknowledged.** A send that succeeds means a frame
left the socket. "Applied", "refused", "no handler for this opcode" and "wrong
target uid" are one observation from outside, so read the state back to tell them
apart, and validate a value before sending rather than expecting a rejection.

**Never decode by overlaying a `#[repr(C)]` struct.** The firmware declares its
protocol structs packed, aligned and plain by turns. Read each field explicitly at
an offset derived from the declaration, and size a payload with the compiler
rather than by summing field widths.

**Never widen a field to swallow its padding, and never assume a padding or
reserved byte is zero.** Mask to the field's real width. Padding carries live junk
on some senders and defined zeros on others, and the two look identical.

**Unknown values are unknown, never clamped.** Enums on the wire are newtypes over
the wire integer with named constants, not Rust `enum`s with a catch-all, so an
unrecognised value passes through as it arrived and an unhandled opcode is
ignored. Zero is usually valid, so a confidently wrong reading is worse than an
unrecognised one.

**Malformed input must not panic.** No direct slice indexing and no `unwrap` on
received bytes; a frame that does not parse is dropped.

**A request addressed to one device is state to every device that sees it.** What a
handler writes to the registry follows what the mesh does with the frame, not
whether the frame is a request.

**A frame from a sender with no device record is dropped before its handler**,
hello and discover excepted — they are what closes that window.

## Invariants

**One frame constructor and one socket write, both private to `wire`.** A send
that skips the protocol gate cannot be written, because there is nothing outside
`wire` to call. Do not open a `pub(crate)` escape hatch; that converts a compile
error into a test's job. Payload builders are `pub(crate)` and reach no socket.

**An opcode with no table entry is refused, not given a default.** Too high and
every older receiver drops the frame; too low and one accepts a frame it reads at
the wrong layout. A test requires every declared opcode to have an entry and every
entry to name a declared opcode.

**Core is `#![forbid(unsafe_code)]`.** All `unsafe` lives in the FFI crate.

**Every `extern "C"` entry point wraps its body in `catch_unwind`.** Unwinding into
C is undefined behaviour.

**No `extern "C"` entry point comes out of a macro.** cbindgen expands nothing, so
a macro-generated entry point is in the library and absent from the header. Shared
bodies factor into an ordinary function the entry points call.

**The generated header is checked in and CI fails on a diff.** Regenerate it with
`scripts/gen-header.sh`. `scripts/check-abi.sh` proves every exported symbol
reached it, which a diff alone cannot.

**The C++ header's event lists are size-checked against the C table**, because an
event the lists miss would be dropped silently by every C++ handler.

**No async runtime.** A receive thread and a timer thread over `Arc<Mutex<_>>`.
Staying synchronous is what keeps the C ABI trivial.

**Events are collected under the lock and dispatched after it is dropped.** A
handler may call back into the library, and would deadlock against a held guard.

**Public API carries `#![deny(missing_docs)]`.** Third parties read the docs.

Every source file starts with the two-line author/copyright header, then a blank
line.

## Verifying a change

**Show the test fail.** Undo the fix, watch the test go red, restore it. A check
that has never been able to fail is unmeasured, not passing.

**A tool that perturbs code must assert its perturbation landed**, and must abort
on a site its pattern cannot address rather than print a skip — a skipped site
reports as covered. Record which named test caught each perturbation: one that
could fail for reasons of its own credits coverage that is not there.

**Offset and width are separate failure modes.** Distinct values per field catch a
read at a neighbour's offset; values a narrowed read cannot reproduce catch a
wrong width. A fixture with distinct-but-small values covers one and looks like it
covers both.

**Build payloads with `testing::poisoned`, not zeros.** A zero-filled fixture
cannot catch a field read at the right offset and the wrong width. Poison is the
default for padding, not a blanket substitution: a byte a sender defines as zero
means something, and poisoning it tests something else.

**A fixture must not supply the precondition the code is meant to establish**, and
one that is too helpful is harder to spot than one that is too simple.

**Ask of a fixture whether it predates the decoding it checks.** The vectors in
`wire/vectors.rs` do, which is what makes them evidence. One composed while the
decoder was written rules out a slip, never a shared misreading, and agreement
with it shows only that two things built from one description match. Say which
kind a fixture is where a reader meets it.

**A prose claim about the wire has no failing state.** Nothing in the suite
contradicts a doc comment that states the wrong thing, it survives every sweep,
and it is read before the code. Only the firmware, a captured frame, or another
implementation that asked can catch it.

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo doc --no-deps

# The C ABI: regenerate the header, check every exported symbol reached it,
# then build the C and C++ examples against the static archive. All in CI.
./scripts/gen-header.sh && git diff --exit-code include/mx_remote.h
cargo build -p mx-remote-ffi && ./scripts/check-abi.sh && make -C examples

# Both crates reach their licences and the C headers through symlinks to the
# repository root, which only packaging resolves. Packaging them together is
# what serves mx-remote-ffi's dependency from the sibling packaged beside it,
# rather than from a crates.io that does not carry it yet. Packaging a workspace
# this way wants a recent cargo, newer than the MSRV the library builds on.
cargo package --workspace
```

Run the gates on the MSRV toolchain as well as the default one, and give it its
own `CARGO_TARGET_DIR`: sharing one directory between toolchains corrupts crate
metadata, which surfaces as an unrelated build failure.

## Publishing

Do not publish to crates.io, push, or open a pull request until a human has
reviewed and vouched for the changes. A crates.io release cannot be withdrawn,
and this crate is the public face of the protocol.
