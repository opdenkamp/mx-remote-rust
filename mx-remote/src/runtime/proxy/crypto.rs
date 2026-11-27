// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The driver proxy's cryptography: the join proofs, the session keys and
//! the sealing of `DATA`. Integers are big endian throughout.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::wire::DeviceUid;

/// The driver key, as the controller's web interface shows it.
pub(crate) type Key = [u8; 32];

/// A join nonce.
pub(crate) type JoinNonce = [u8; 16];

/// A proof: an HMAC-SHA256.
pub(crate) type Proof = [u8; 32];

/// The tag sealing a `DATA` frame.
pub(crate) const TAG_LEN: usize = 16;

fn hmac(key: &Key, parts: &[&[u8]]) -> Proof {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes a key of any length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// The driver's proof that it holds the key:
/// `HMAC-SHA256(K, "drv" ‖ Nd ‖ Nc ‖ Ud ‖ Uc)`.
pub(crate) fn driver_proof(
    key: &Key,
    nd: &JoinNonce,
    nc: &JoinNonce,
    driver: DeviceUid,
    controller: DeviceUid,
) -> Proof {
    hmac(
        key,
        &[b"drv", nd, nc, driver.as_bytes(), controller.as_bytes()],
    )
}

/// The controller's proof that it holds the key, binding the lease it
/// grants: `HMAC-SHA256(K, "ctl" ‖ Nd ‖ Nc ‖ Ud ‖ Uc ‖ lease)`.
pub(crate) fn controller_proof(
    key: &Key,
    nd: &JoinNonce,
    nc: &JoinNonce,
    driver: DeviceUid,
    controller: DeviceUid,
    lease: u16,
) -> Proof {
    hmac(
        key,
        &[
            b"ctl",
            nd,
            nc,
            driver.as_bytes(),
            controller.as_bytes(),
            &lease.to_be_bytes(),
        ],
    )
}

/// Compares two proofs in time independent of where they differ.
pub(crate) fn proofs_match(a: &Proof, b: &Proof) -> bool {
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// One direction's session key: HKDF-SHA256 of the driver key, salted with
/// `Nd ‖ Nc`, for `"mxr-proxy" ‖ Ud ‖ Uc ‖ direction`.
pub(crate) fn session_key(
    key: &Key,
    nd: &JoinNonce,
    nc: &JoinNonce,
    driver: DeviceUid,
    controller: DeviceUid,
    direction: &[u8; 3],
) -> Key {
    let mut salt = [0u8; 32];
    salt[..16].copy_from_slice(nd);
    salt[16..].copy_from_slice(nc);
    let mut info = Vec::with_capacity(9 + 32 + 3);
    info.extend_from_slice(b"mxr-proxy");
    info.extend_from_slice(driver.as_bytes());
    info.extend_from_slice(controller.as_bytes());
    info.extend_from_slice(direction);
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&salt), key)
        .expand(&info, &mut out)
        .expect("32 bytes is within what HKDF-SHA256 gives");
    out
}

fn nonce(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    n
}

/// Seals `inner` under `key` at `counter`, authenticating `aad` with it:
/// the ciphertext followed by its tag.
pub(crate) fn seal(key: &Key, counter: u64, aad: &[u8], inner: &[u8]) -> Vec<u8> {
    Aes256Gcm::new(key.into())
        .encrypt(
            Nonce::from_slice(&nonce(counter)),
            Payload { msg: inner, aad },
        )
        .expect("AES-GCM seals any frame a datagram holds")
}

/// Opens what [`seal`] made, `None` when it fails its check.
pub(crate) fn open(key: &Key, counter: u64, aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    Aes256Gcm::new(key.into())
        .decrypt(
            Nonce::from_slice(&nonce(counter)),
            Payload { msg: sealed, aad },
        )
        .ok()
}

/// The counters of one direction a receiver has taken: each once, and none
/// older than the 64 below the highest.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Replay {
    highest: u64,
    /// Bit `i` stands for `highest - i`.
    seen: u64,
}

impl Replay {
    const WINDOW: u64 = 64;

    /// Whether `counter` may be taken: never zero, not taken before, and
    /// within the window.
    pub(crate) fn fresh(&self, counter: u64) -> bool {
        if counter == 0 {
            return false;
        }
        if counter > self.highest {
            return true;
        }
        let behind = self.highest - counter;
        behind < Self::WINDOW && self.seen & (1 << behind) == 0
    }

    /// Records `counter` as taken. Call only for one [`Self::fresh`] passed
    /// and whose frame opened.
    pub(crate) fn take(&mut self, counter: u64) {
        if counter > self.highest {
            let ahead = counter - self.highest;
            self.seen = if ahead >= Self::WINDOW {
                0
            } else {
                self.seen << ahead
            };
            self.seen |= 1;
            self.highest = counter;
        } else {
            self.seen |= 1 << (self.highest - counter);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Decodes a hex fixture.
    pub(crate) fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    fn array<const N: usize>(s: &str) -> [u8; N] {
        hex(s).try_into().expect("fixture length")
    }

    // The driver proxy contract's published inputs (§8).
    pub(crate) const K: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    pub(crate) const ND: &str = "101112131415161718191a1b1c1d1e1f";
    pub(crate) const NC: &str = "202122232425262728292a2b2c2d2e2f";
    pub(crate) const UD: DeviceUid = DeviceUid::from_array([0xD0; 16]);
    pub(crate) const UC: DeviceUid = DeviceUid::from_array([0xC0; 16]);

    pub(crate) fn key() -> Key {
        array(K)
    }
    pub(crate) fn nd() -> JoinNonce {
        array(ND)
    }
    pub(crate) fn nc() -> JoinNonce {
        array(NC)
    }

    #[test]
    fn the_published_proofs_hold() {
        assert_eq!(
            driver_proof(&key(), &nd(), &nc(), UD, UC).to_vec(),
            hex("f22e793df352fff4d76660875d67c080378813eb426ed89d739ac2eac0c6fa2b")
        );
        assert_eq!(
            controller_proof(&key(), &nd(), &nc(), UD, UC, 60).to_vec(),
            hex("e8dd2bdcf883b25ff4bf51be09abbd2a50cdf385707d5fea765b97767c54a528")
        );
    }

    #[test]
    fn the_published_session_keys_hold() {
        assert_eq!(
            session_key(&key(), &nd(), &nc(), UD, UC, b"d2c").to_vec(),
            hex("e33e3f4294553040df679722fe9dddb24e1f546bc9ff5df1f02e194b541d9c14")
        );
        assert_eq!(
            session_key(&key(), &nd(), &nc(), UD, UC, b"c2d").to_vec(),
            hex("bb764997d518d7239d36f9fad4c4488642515bcc35ecc6a106831bcbd6c6b543")
        );
    }

    #[test]
    fn proofs_match_only_when_equal() {
        let a = [7u8; 32];
        let mut b = a;
        assert!(proofs_match(&a, &b));
        b[31] ^= 1;
        assert!(!proofs_match(&a, &b));
        b = a;
        b[0] ^= 0x80;
        assert!(!proofs_match(&a, &b));
    }

    /// Each counter is taken once, in any order within the window, and none
    /// older than it.
    #[test]
    fn a_counter_is_taken_once_within_the_window() {
        let mut r = Replay::default();
        assert!(!r.fresh(0), "zero is no counter");
        for c in [1, 3, 2] {
            assert!(r.fresh(c), "{c}");
            r.take(c);
            assert!(!r.fresh(c), "{c} again");
        }
        r.take(100);
        assert!(r.fresh(37), "63 behind is in the window");
        assert!(!r.fresh(36), "64 behind is out of it");
        r.take(37);
        assert!(!r.fresh(37));
        assert!(r.fresh(99));
        r.take(300);
        assert!(
            !r.fresh(100),
            "a jump past the window forgets nothing it should"
        );
        assert!(r.fresh(299));
    }
}
