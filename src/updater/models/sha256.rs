//! SHA-256, incremental, in about a hundred lines of safe Rust.
//!
//! # Why this is not the `sha2` crate
//!
//! `sha2` is in `Cargo.lock` already — but only as a *build* dependency, via
//! `rust-embed-utils`. Nothing of it is linked into the shipped binary today,
//! so taking it would have been a genuinely new runtime dependency and a
//! `Cargo.lock` edit on a project where the lock is the only pin on four git
//! dependencies (`AGENTS.md`), for an algorithm that is 64 constants and two
//! loops and comes with published test vectors.
//!
//! The judgement is narrower than "hand-rolled crypto is fine". What this
//! computes is an **integrity** check against a hash fetched over HTTPS from the
//! same origin as the archive — it is not a signature, and it is not a defence
//! against an attacker who controls the manifest (that is what the manifest's
//! reserved `signature` field is for; see `docs/release.md`). A bug here fails
//! *closed*: a wrong digest refuses the install. And a wrong digest is exactly
//! what the NIST FIPS 180-4 vectors in this module's tests detect, including the
//! multi-block and unaligned-chunk cases a streaming implementation gets wrong.
//!
//! # Streaming
//!
//! [`Sha256::update`] takes arbitrary chunk boundaries and buffers the partial
//! block itself, so hashing a 12 MB archive costs one 64-byte buffer and never
//! holds the archive in memory. That is the whole reason it is incremental
//! rather than a one-shot over a `&[u8]`.

/// The first 32 bits of the fractional parts of the cube roots of the first 64
/// primes — FIPS 180-4 §4.2.2.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// The first 32 bits of the fractional parts of the square roots of the first
/// eight primes — FIPS 180-4 §5.3.3.
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

const BLOCK: usize = 64;

/// An incremental SHA-256.
pub struct Sha256 {
    state: [u32; 8],
    /// The partial block not yet compressed. Never longer than [`BLOCK`].
    buffer: [u8; BLOCK],
    buffered: usize,
    /// Total message length in bytes. The padding encodes it in *bits*, so this
    /// is what bounds the input at 2^61 bytes — two exabytes, which is not a
    /// bound any download will meet.
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: H0,
            buffer: [0; BLOCK],
            buffered: 0,
            length: 0,
        }
    }

    /// Feeds the next chunk. Chunk boundaries are free: the partial block is
    /// carried between calls, so `update(b"ab")` twice and `update(b"abab")`
    /// once produce the same digest.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);

        if self.buffered > 0 {
            let take = (BLOCK - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];

            // Still a partial block: `data` is necessarily empty (the `min`
            // above took all of it), and falling through would run the tail
            // path below, whose `self.buffered = rest.len()` would set the
            // count back to zero and silently discard everything buffered so
            // far. Returning here is the whole reason `update` is chunk-safe;
            // `a_million_letters_hash_the_same_however_they_are_chunked` is
            // what catches its absence.
            if self.buffered < BLOCK {
                return;
            }

            let block = self.buffer;
            self.compress(&block);
            self.buffered = 0;
        }

        let mut chunks = data.chunks_exact(BLOCK);
        for block in &mut chunks {
            let mut fixed = [0u8; BLOCK];
            fixed.copy_from_slice(block);
            self.compress(&fixed);
        }

        let rest = chunks.remainder();
        self.buffer[..rest.len()].copy_from_slice(rest);
        self.buffered = rest.len();
    }

    /// The digest, as lowercase hex — the form `update.json` and `SHA256SUMS`
    /// both use, so a comparison never has to normalise case on the hot path.
    pub fn finalize_hex(mut self) -> String {
        // FIPS 180-4 §5.1.1: append 0x80, then zeroes, then the 64-bit
        // big-endian bit length, so the total is a multiple of 64 bytes.
        //
        // The zero count is computed rather than looped towards: a `while
        // self.buffered != 56` would not terminate at all if `update` ever
        // stopped advancing, which is exactly the failure mode the bug fixed
        // above produced.
        let bits = self.length.wrapping_mul(8);
        self.update_no_count(&[0x80]);
        let zeros = (56 + BLOCK - self.buffered) % BLOCK;
        self.update_no_count(&vec![0u8; zeros]);
        debug_assert_eq!(self.buffered, 56, "padding did not land on the length slot");
        self.update_no_count(&bits.to_be_bytes());

        let mut hex = String::with_capacity(64);
        for word in self.state {
            hex.push_str(&format!("{word:08x}"));
        }
        hex
    }

    /// Padding bytes must not extend the length the padding itself encodes.
    fn update_no_count(&mut self, data: &[u8]) {
        let before = self.length;
        self.update(data);
        self.length = before;
    }

    fn compress(&mut self, block: &[u8; BLOCK]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        for (slot, value) in self
            .state
            .iter_mut()
            .zip([a, b, c, d, e, f, g, h].into_iter())
        {
            *slot = slot.wrapping_add(value);
        }
    }
}

/// Whether two hex digests are the same, ignoring case and surrounding space.
///
/// Not constant-time, and deliberately: both operands are public values (a
/// digest of a public archive against a digest from a public manifest), so
/// there is no secret for a timing side channel to leak.
pub fn digests_match(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

/// Whether a string is a well-formed SHA-256 digest: 64 hex digits, nothing
/// else. Used to refuse a manifest entry before anything is downloaded against
/// it, so a truncated or placeholder hash fails at parse time rather than after
/// 12 MB of transfer.
pub fn is_hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{Sha256, digests_match, is_hex_digest};

    fn hash(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize_hex()
    }

    /// FIPS 180-4's own published vectors, plus the empty string. These are
    /// what make a hand-written implementation defensible.
    #[test]
    fn matches_the_published_nist_vectors() {
        assert_eq!(
            hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 448 bits — the case that needs a second padding block.
        assert_eq!(
            hash(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // 896 bits, two full blocks plus padding.
        assert_eq!(
            hash(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    /// The million-'a' vector, fed in awkward chunk sizes. This is the test that
    /// catches a streaming bug: buffering, block carry-over and the length
    /// counter all have to survive boundaries that fall nowhere near 64 bytes.
    #[test]
    fn a_million_letters_hash_the_same_however_they_are_chunked() {
        const EXPECTED: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

        for chunk in [1usize, 7, 63, 64, 65, 1000, 4096] {
            let mut hasher = Sha256::new();
            let data = vec![b'a'; chunk];
            let mut written = 0usize;
            while written < 1_000_000 {
                let take = chunk.min(1_000_000 - written);
                hasher.update(&data[..take]);
                written += take;
            }
            assert_eq!(hasher.finalize_hex(), EXPECTED, "chunk size {chunk}");
        }
    }

    #[test]
    fn one_byte_changed_changes_the_digest() {
        assert_ne!(hash(b"abc"), hash(b"abd"));
        assert_ne!(hash(b"abc"), hash(b"abc "));
    }

    #[test]
    fn digest_comparison_ignores_case_and_surrounding_space() {
        let lower = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(digests_match(lower, &lower.to_uppercase()));
        assert!(digests_match(lower, &format!("  {lower}\n")));
        assert!(!digests_match(lower, &lower.replace('a', "b")));
    }

    #[test]
    fn a_malformed_digest_is_rejected_before_anything_is_downloaded() {
        assert!(is_hex_digest(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        ));
        assert!(!is_hex_digest(""), "empty");
        assert!(!is_hex_digest("ba7816bf"), "truncated");
        assert!(
            !is_hex_digest("zzzz16bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            "non-hex characters"
        );
        assert!(
            !is_hex_digest("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015adx"),
            "too long"
        );
    }
}
