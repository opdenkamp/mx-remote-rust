# AGENTS.md

## What this is

A Rust client library for Pulse-Eight MatrixOS devices (neo matrices, OneIP/V2IP
units, ProAmp8 amplifiers) over UDP multicast/broadcast.

It is meant to be *the* public implementation third parties integrate against: a
pure-Rust core, a C ABI on top of it for C/C++ consumers, and further language
bindings layered on the same core. Rust was chosen for that reason — it links
into a C++ program as a plain static archive, with no runtime to initialise, no
GC pointer rules, and no signal handlers taken from the host process.

It is a port of the Go client linked below, which stays the reference until
this crate replaces it. Both crates are published on crates.io, at 1.0.0.

## Sources of truth

The firmware headers `mx_remote_proto.h` and `mx_opcodes.h` define the wire
format. They are not public, and they settle every disagreement with this
crate.

Two older clients of the same protocol are public. Both are broader on legacy
opcodes than this crate; that is a decision rather than an oversight, and an
opcode is not ported back just because another client carries it.

- Python — <https://github.com/opdenkamp/mx-remote>. The oldest and most
  mature, and the one the byte-exact vectors in `wire/vectors.rs` came from.
- Go — <https://github.com/opdenkamp/mx-remote-golang>. Its `CLAUDE.md` is the
  accumulated protocol knowledge for this format and is not duplicated here:
  read it in full before touching wire code.

## Invariants

**Byte layouts must match the firmware exactly.** This talks to embedded devices
that will not be updated to suit us.

**Never decode by overlaying a `#[repr(C)]` struct.** `mx_remote_proto.h` mixes
`PACKED`, `ALIGN(8)` and plain structs, so field offsets do not follow from field
widths. Read each field explicitly at an offset derived from the declaration.

**Malformed input must not panic.** Anything on the wire is attacker- or
bug-shaped. No direct slice indexing and no `unwrap` on received bytes; a frame
that does not parse is dropped.

**Unknown values are unknown, never clamped.** Enums are newtypes over the wire
integer with named constants, not Rust `enum`s with a catch-all — an unrecognised
value passes through as it arrived. Zero is usually valid, so a confidently wrong
reading is worse than an unrecognised one.

**One frame constructor and one socket write, both private to their module.**
`wire::frame::build_frame` and `Conn::send` are reachable only from inside
`wire`, and `Tx::send` is the only thing between them. A send that skips the
protocol gate cannot be written, because there is nothing outside `wire` to
call. Do not open a `pub(crate)` escape hatch — that silently converts a
compile error back into a test's job. The payload builders are `pub(crate)`
and are not an exception: they assemble bytes and can reach no socket.

**A send names its recipient, in a word.** `Tx::send` takes an `Addressee`,
whose `Broadcast` variant is the only way to send without naming a device, so
"send to everyone" cannot be confused with a target nobody filled in. A nilable
target would leave that difference to a test that reads the source; the type
answers it, and so does the choke point above. Their absence from the suite is
not a hole.

**The gate checks what is stamped, not what the opcode table says.** A
receiver drops any frame stamped above its own version, so `stamp_for` decides
both the header field and the floor the addressee is measured against. They
differ for `V2IP_MULTIVIEWER`, stamped at 0x20 rather than its table's 0x16;
checking the table value there would let through exactly the frame the device
discards.

**Core is `#![forbid(unsafe_code)]`.** All `unsafe` lives in the FFI crate.

**Every `extern "C"` entry point wraps its body in `catch_unwind`.** Unwinding
into C is undefined behaviour.

**No `extern "C"` entry point comes out of a macro.** cbindgen parses the source
and expands nothing, so a macro-generated entry point is in the library and
absent from the header: callers link against a symbol the header never
declared. Shared bodies factor into an ordinary function the entry points call.

**The generated header is checked in, and CI fails on a diff.** Regenerate it
with `scripts/gen-header.sh`. Third parties read `include/mx_remote.h`, and one
that lags the library points them at an API that is not there.

**Every exported symbol is declared in the header, and CI proves it.**
`scripts/check-abi.sh` reads the archive with `nm` and compares. A diff against
the generated header cannot catch the macro case above, because a header
missing an entry point is exactly what cbindgen produces for one.

**The C++ header's event lists are size-checked against the C table.** A
`static_assert` in `include/mx_remote.hpp` compares the table's size against
what its own lists count, because an event the lists miss would be silently
dropped by every C++ handler and nothing else would notice.

**No async runtime.** A receive thread and a timer thread over `Arc<Mutex<_>>`.
Staying synchronous is what keeps the C ABI trivial.

**Events are collected under the lock and dispatched after it is dropped.** A
handler is free to call back into the library — reading the state its event
describes is the ordinary thing to do with one — and would deadlock against a
guard still held. Nothing in the type system says so; losing this is a runtime
hang, not a compile error.

**Public API carries `#![deny(missing_docs)]`.** Third parties read the docs, not
the source.

Every source file starts with the two-line author/copyright header followed by a
blank line.

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
# rather than from a crates.io that does not carry it yet. Packaging a
# workspace this way wants a recent cargo, newer than the MSRV the library
# itself builds on.
cargo package --workspace
```

## Publishing

Do not publish to crates.io, push, or open a pull request until a human has
reviewed and vouched for the changes. A crates.io release cannot be withdrawn,
and this crate is the public face of the protocol.

## Auditing the decode

Four instruments, each blind to something the next one sees:

- offset-mutation sweep — finds fields no test exercises
- `poisoned()` fixtures — finds fields read at the right offset, wrong width
- coverage over the handlers — finds opcodes nothing runs at all
- builder/decoder round trip — finds fields attributed to the wrong thing

None of them catches a builder and a decoder that are wrong together. Only the
byte-exact vectors in `wire/vectors.rs`, generated from the Python client
rather than from this one, and the firmware headers close that.

**A tool that perturbs code and reports on the result must assert that its
perturbation landed.** Such tools lie by silently not applying — a regex that
matches a type name instead of an offset, a replace that hits the wrong
occurrence of a repeated literal. Every one written for this protocol has done
it at least once. A clean sweep from a tool never shown to fail is unmeasured,
not clean.
