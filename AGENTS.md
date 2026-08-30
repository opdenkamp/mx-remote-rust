# AGENTS.md

## What this is

A Rust client library for Pulse-Eight MatrixOS devices (neo matrices, OneIP/V2IP
units, ProAmp8 amplifiers) over UDP multicast/broadcast.

It is meant to be *the* public implementation third parties integrate against: a
pure-Rust core, a C ABI on top of it for C/C++ consumers, and further language
bindings layered on the same core. Rust was chosen for that reason — it links
into a C++ program as a plain static archive, with no runtime to initialise, no
GC pointer rules, and no signal handlers taken from the host process.

It is a port of the Go client. This file carries the protocol knowledge itself
rather than pointing at that repository for it, so everything needed to work on
the wire format is here.

## Sources of truth

The firmware defines the wire format and settles every disagreement with this
crate. It is closed source and no copy of it ships here, so a layout this file
does not already pin has to be settled against a captured frame or the two
public clients below — never guessed from field widths.

Two older clients of the same protocol are public. Both are independent
implementations rather than bindings over a shared core.

- Python — <https://github.com/opdenkamp/mx-remote>. The oldest and most
  mature, and the one the byte-exact vectors in `wire/vectors.rs` came from.
  It is the only one still decoding the opcodes current firmware no longer
  sends; that is a decision rather than an oversight, and an opcode is not
  ported back just because another client carries it.
- Go — <https://github.com/opdenkamp/mx-remote-golang>. The client this crate
  was ported from.

## Wire format

`[0x50, 0x38, protocol(u16 LE), uid(16), opcode(u16 LE), length(u16 LE), payload]`

The stamped protocol is the per-opcode minimum from `stamp_for`, not the version
this library speaks. A receiver drops any frame stamped above its own version,
so stamping our own would make every device with a lower cap ignore us. These
minimums stay deliberately low: an opcode whose payload only ever grew trailing
fields keeps its original version.

Check the target's reported version before sending, not just the stamp. A
receiver drops a frame it cannot decode silently, with no NAK at any layer, so
the call would otherwise succeed while nothing happened. A ProAmp8 caps at
0x22 and one on 4.1.1 reports 0x11, below the floor of several opcodes here, so
this is not hypothetical. A device that has reported no version is let through:
not knowing is not the same as knowing it is too old.

`0x02` SYS_BAY_CONFIG, `0x03` SYS_LINKS and `0x23` SYS_BAY_CONFIG_SECONDARY are
paged across several frames whose record counts vary. Merge records into the
cache; never replace a cached list from one frame.

`0x3C` V2IP_DEVICE_CFG carries every field behind its own validity marker,
because a sender zeroes the payload and fills in only what it is writing. Fold a
frame onto the cached config field by field.

`0x49` V2IP_VIDEO_WALL is owned by a loadable module rather than MatrixOS, and
unlike `0x3C` it **replaces** rather than merges: no field carries a validity
marker, and a zero width or height means "clear the wall", not "unset". A revert
carries no window at all. `0x40` V2IP_TILING is not a substitute — on a sink
running that module a `0x40` write is transient, because the module's reconciler
pushes its own window back within about a second.

`0x08` MX_ROUTE is decoded but no MatrixOS build transmits it: the firmware's
route-broadcast helper is defined and never called. The decoder still has to be
right for third-party controllers, but do not expect one on a live mesh, and do
not treat its absence as a bug.

## Invariants

**Byte layouts must match the firmware exactly.** This talks to embedded devices
that will not be updated to suit us.

**Never decode by overlaying a `#[repr(C)]` struct.** Packed structs are the
exception in this protocol, not the rule: the firmware declares its protocol
structs packed, 8-byte-aligned and plain by turns, and only the packed ones can
be decoded by summing field widths. Elsewhere the compiler inserts padding. Read
each field explicitly at an offset derived from the declaration. Two recurring
traps:

- Tick timestamps are `uint_fast32_t`, so they align to 4 and pad whatever
  precedes them.
- Where a variable-length tail follows a struct, the tail starts after the
  struct's *own* trailing padding, not at the end of its last field (`0x48`
  RC_IR_TX: timings at 36, not 34).

**Never widen a field to swallow its padding, and never assume a padding or
reserved byte is zero.** Cortex-M builds with `-fshort-enums`, so a plain enum on
the wire is often one byte followed by padding — and the firmware `memcpy`s
uncleared stack locals over payloads, so that padding carries live junk that
differs frame to frame. The offsets still line up either way, which is why this
survives a cross-check between two implementations: only the field itself is
wrong, and it reads as a value that changes while the setting does not. Mask to
the field's real width rather than asserting the neighbouring bytes are zero
(the RC target enum at `0x45`+16 is one byte, not four).

**Malformed input must not panic.** Anything on the wire is attacker- or
bug-shaped. No direct slice indexing and no `unwrap` on received bytes; a frame
that does not parse is dropped.

**Unknown values are unknown, never clamped.** Enums are newtypes over the wire
integer with named constants, not Rust `enum`s with a catch-all — an unrecognised
value passes through as it arrived, and an unhandled opcode is ignored. Zero is
usually valid, so a confidently wrong reading is worse than an unrecognised one.
The protocol is meant to stay compatible in both directions, so a driver must not
break over a firmware update. Auditing this by searching for masks does not work:
extracting a bit field and folding a range look identical. The mask is almost
always right — look at what the extracted value is then converted into.

**Do not mirror a firmware receiver without asking whether its handling is
defensible.** Firmware predating the fix builds `0x3C` from an uninitialised
scaling-config struct and ORs flags onto stack garbage, so on a receiver-capable
unit bits 2..6 of the scaling flags are noise and the valid-mode bit can be set
spuriously, leaving the mode and refresh behind it uninitialised. The firmware's
own receiver copies the whole top nibble; this library carries bit 7 alone,
because caching noise as though it meant something is worse than matching the
reference.

**One frame constructor and one socket write, both private to their module.**
`wire::frame::build_frame` and `Conn::send` are reachable only from inside
`wire`, and `Tx::send` is the only thing between them. A send that skips the
protocol gate cannot be written, because there is nothing outside `wire` to
call. Do not open a `pub(crate)` escape hatch — that silently converts a
compile error back into a test's job. The payload builders are `pub(crate)`
and are not an exception: they assemble bytes and can reach no socket.

Put a check at the choke point rather than at each site. The per-site version of
this, in the client this was ported from, missed the one method that built its
own frame while its siblings delegated to a gated one, and shipped. Note what the
ordering costs: the gate runs after each method's own preconditions, so a call
that fails both reports the precondition. That is the more useful error, but it
means a test has to drive a method far enough to actually transmit before it
proves anything about the gate.

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

## Working on the protocol

**Ask where a wire value first becomes a typed thing, before asking where it is
decoded.** Those are rarely the same file, and the narrowing boundary is where
unknown values get lost — so auditing decoders alone systematically misses it.
Keep the raw value and convert late, which is what lets an unrecognised value
survive to the caller.

**Where the compiler can only get you halfway, assert the rest against the
source.** A type can force a decision to be made without forcing it to be made
correctly. A test that reads the crate's own source and checks each site must
also fail if its own pattern stops matching, since a regex that matches nothing
reports every file clean.

**A skip is not a pass, and a graceful degradation can hide the failure the test
exists to catch.** A socket test that skips when nothing arrives cannot fail in
the direction it is looking: a library that sent nothing produces a skip. Prove
the host's multicast loopback with a probe first; once the probe lands, silence
is the library's fault. Where a test tolerates an environment, make the
environment prove itself rather than inferring it from the absence of a result.

**Fixtures fail by being too simple to reach the thing under test.** Zero
padding hides a wrong field width; a bare bay fails a method's preconditions
before it can reach the send being tested; a one-sided assertion passes for a
guard that always fires. In each case the test is green because it never arrives
at the code it names, and simplicity is exactly what makes such a fixture look
trustworthy. Build the fixture that reaches the thing, then check it still fails
when the thing is broken.

**A scan has five separate questions, and fixing one does not answer the
others.** Each of these was a real hole in the client this was ported from,
found only when someone named it:

1. *Does the pattern still match?* — assert a minimum number of sites, or a
   rename leaves it matching nothing and reporting every file clean.
2. *Does every match get read?* — a site the detailed pattern cannot parse must
   fail with its location, never be skipped.
3. *Does every site get matched?* — find sites with a loose pattern too, and
   require each to be readable by the detailed one.
4. *Does this check still look at anything at all?* — and this one recurses. An
   assertion phrased as "not too many" passes when the search finds nothing.
   Phrase every check so that a search returning nothing fails it.
5. *Is the guarded thing still the only way through?* — a send scan assumes one
   function builds every frame and one writes every socket. Count them: a second
   builder or a second write bypasses the gate, and the scan cannot see it,
   because it only inspects sends it already recognises.

**A tool that perturbs code and reports on the result must assert that its
perturbation landed.** Such tools lie by silently not applying — a regex that
matches a type name instead of an offset, a replace that hits the wrong
occurrence of a repeated literal, a removal whose pattern did not match at all.
Every one written for this protocol has done it at least once, reporting
"nothing noticed this change" when the truth was "there was no change". A clean
sweep from a tool never shown to fail is unmeasured, not clean.

**Every layer that asserts something can encode the same mistake — including the
one added to catch it.** This has bitten in a decoder, in a fixture written to
pin that decoder, and in a hand-check written to validate the fixture. Treat a
red test as evidence that the test and the code disagree, not that the code is
wrong, and check which one moved.

**A symptom too cheap for the defect means something is compensating.** When a
decoder disagrees with its struct, that tells you a field is misnamed — not yet
that behaviour is broken. Check the consumers before moving offsets: two errors
in the same direction cancel, and there the fix is to move the decoder and every
consumer together, since correcting the offsets alone introduces the bug. A
four-field reversal that shows up only as a backwards log line is the tell.

**Assert both directions of a guard, and pick an input that can actually trip
it.** A test that only checks the refusal passes for a guard hardcoded to
refuse, and for a call failing for some unrelated reason — the paired "and this
one is allowed" is what makes the refusal mean what it says. Choosing the input
matters as much: a protocol-floor test using a device reporting 0x11, which sits
at or above most opcodes' floors, asserts a refusal that should never have
happened. An input that cannot trip the thing under test looks exactly like
coverage.

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

Ask what an instrument cannot say before believing what it does. Four of them,
each blind to something the next one sees:

| check | finds | blind to |
|---|---|---|
| offset-mutation sweep | fields no test exercises | wrong width, wrong branch |
| `poisoned()` fixtures | fields read at the wrong width | fields no test reaches |
| coverage over the handlers | opcodes nothing runs at all | anything a test runs but never asserts |
| builder/decoder round trip | fields decoded at the right offset but attributed to the wrong thing | a builder and decoder that are wrong *together* |

A handler with no test reports clean from the first two for the same reason an
empty file does. The round trip is the only one that tests *meaning* rather than
position: a source read as a target, or a left delay as a right one, is
positionally perfect and semantically inverted, and only a disagreement between
the two halves of the library exposes it. Where both halves are wrong together
it is clean, and nothing but an external reference closes that — the byte-exact
vectors in `wire/vectors.rs`, generated from the Python client rather than from
this one, or a captured frame.

Build test payloads with `testing::poisoned`, not a zero-filled buffer. A
zero-filled fixture cannot catch a field read at the right offset but the wrong
width: the padding beside it is zero, so the widened read returns the same answer
and every assertion still passes. That is the class an offset-mutation sweep
structurally misses — mutating slice bounds tests whether a *shifted* read is
caught, and a wrong-width read is not shifted.

Poison is the default for padding, not a blanket substitution. Bytes the sender
leaves undefined and bytes it defines as zero look identical in a fixture and
mean opposite things: a video wall revert carries a genuinely zero window on
purpose, and poisoning it would test something else.

A fixture also hides a shift whenever the neighbouring bytes carry the same
value: adjacent fields set to the same number, a short NUL-terminated string in a
wider field, or a port whose high byte is zero. Give every field a distinct
value, and assert that the bays a frame did *not* address were left alone — two
decode bugs in the ported client survived because the assertion passed on state
an earlier frame had set.
