//! The human-facing friend code: a 16-character Crockford-Base32 string with a
//! trailing Verhoeff check symbol, encoding a 75-bit identifier.
//!
//! The identifier is derived server-side from the install's mTLS certificate
//! fingerprint and arrives over the wire as 10 raw bytes — the client never
//! computes it. All of the Crockford/Verhoeff encoding lives here, on the
//! client; the server never touches the alphabet or the check symbol.
//!
//! The check symbol is a Verhoeff digit generalized from decimal (Verhoeff's
//! original dihedral group D5) to our 32-symbol alphabet, i.e. the dihedral
//! group D16 of order 32 — one group element per Crockford symbol. Verhoeff is
//! built on a *non-abelian* group precisely so that adjacent transpositions are
//! detectable: in plain mod-32 arithmetic `a + b == b + a`, so a Luhn-style
//! cyclic check cannot see a swap, whereas here `x·y ≠ y·x` in general. The
//! scheme catches every single-symbol error and every adjacent transposition;
//! `mod tests` re-proves the underlying group/permutation properties
//! exhaustively.

/// Crockford Base32 alphabet (no I, L, O, U).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// 15 body symbols × 5 bits = 75 bits.
const BODY_SYMBOLS: usize = 15;
/// 15 body symbols + 1 Verhoeff check symbol.
const CODE_LEN: usize = BODY_SYMBOLS + 1;
/// Canonical wire width: the 75-bit value, big-endian, top 5 bits zero.
const BYTE_LEN: usize = 10;

/// A 75-bit friend code, stored as its canonical 10 big-endian bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FriendCode([u8; BYTE_LEN]);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FriendCodeError {
    #[error("friend code must be {CODE_LEN} symbols, got {0}")]
    WrongLength(usize),
    #[error("invalid friend code symbol: {0:?}")]
    BadSymbol(char),
    #[error("friend code checksum mismatch")]
    BadChecksum,
    #[error("friend code must be {BYTE_LEN} bytes, got {0}")]
    WrongByteLen(usize),
}

impl FriendCode {
    /// Build from a raw 75-bit value (higher bits are masked off).
    pub fn from_value(value: u128) -> Self {
        let value = value & ((1u128 << 75) - 1);
        let mut bytes = [0u8; BYTE_LEN];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (value >> (8 * (BYTE_LEN - 1 - i))) as u8;
        }
        FriendCode(bytes)
    }

    /// The 75-bit value.
    pub fn value(&self) -> u128 {
        self.0.iter().fold(0u128, |acc, &b| (acc << 8) | b as u128)
    }

    /// The canonical 10 wire bytes.
    pub fn as_bytes(&self) -> &[u8; BYTE_LEN] {
        &self.0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Parse the 10 raw wire bytes received from the server.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FriendCodeError> {
        let arr: [u8; BYTE_LEN] = bytes
            .try_into()
            .map_err(|_| FriendCodeError::WrongByteLen(bytes.len()))?;
        Ok(FriendCode(arr))
    }
}

/// Index of a symbol in the Crockford alphabet.
fn symbol_value(c: char) -> Option<u32> {
    CROCKFORD.iter().position(|&b| b as char == c).map(|p| p as u32)
}

/// Order parameter of the dihedral group D16: it has `2 * DIHEDRAL_N == 32`
/// elements, exactly one per Crockford symbol. Element `i < 16` is the rotation
/// rⁱ; element `16 + i` is a reflection.
const DIHEDRAL_N: u32 = 16;

/// Verhoeff permutation σ over the 32 group elements. A derangement chosen so
/// the derived check is *totally anti-symmetric* — `mul(x, σ(y)) != mul(y, σ(x))`
/// for every `x != y`. With Verhoeff's per-position powers σⁱ, that one property
/// is what guarantees detection of all single-symbol errors and all adjacent
/// transpositions; `mod tests` re-proves it (and that σ is a permutation) by
/// exhaustive enumeration, so this table is not a magic constant to be trusted.
const SIGMA: [u8; 32] = [
    1, 0, 4, 6, 2, 9, 3, 12, 14, 5, 16, 18, 7, 17, 8, 20,
    10, 11, 13, 15, 21, 19, 23, 24, 27, 28, 22, 31, 26, 30, 25, 29,
];

/// Dihedral group D16 multiplication. Elements are `s * 16 + k` for the
/// reflection flag `s ∈ {0, 1}` and rotation `k ∈ 0..16`, with the standard law
/// `rᵃ·s = s·r⁻ᵃ` folded in.
fn dihedral_mul(a: u32, b: u32) -> u32 {
    let (sa, ka) = (a / DIHEDRAL_N, a % DIHEDRAL_N);
    let (sb, kb) = (b / DIHEDRAL_N, b % DIHEDRAL_N);
    let s = (sa + sb) % 2;
    let k = if sa == 0 {
        (ka + kb) % DIHEDRAL_N
    } else {
        (ka + DIHEDRAL_N - kb) % DIHEDRAL_N
    };
    s * DIHEDRAL_N + k
}

/// Inverse in D16: rotations invert the angle, reflections are involutions.
fn dihedral_inv(a: u32) -> u32 {
    if a < DIHEDRAL_N {
        (DIHEDRAL_N - a) % DIHEDRAL_N
    } else {
        a
    }
}

/// σ applied `i` times to `x` (the per-position permutation power σⁱ).
fn sigma_pow(i: usize, mut x: u32) -> u32 {
    for _ in 0..i {
        x = SIGMA[x as usize] as u32;
    }
    x
}

/// Verhoeff (dihedral D16) check symbol over the alphabet-normalized body.
///
/// Folds the body right-to-left as `∏ σⁱ(symbolᵢ)`, where position `i` counts
/// from the check symbol: the check occupies position 0, so the last body
/// symbol sits at position 1. The check symbol is the group inverse of that
/// product, making the full code's product the identity `0`.
fn verhoeff_check(body: &str) -> char {
    let mut acc = 0u32;
    for (i, c) in body.chars().rev().enumerate() {
        let cp = symbol_value(c).expect("body is built from the alphabet");
        acc = dihedral_mul(acc, sigma_pow(i + 1, cp));
    }
    CROCKFORD[dihedral_inv(acc) as usize] as char
}

impl std::fmt::Display for FriendCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = self.value();
        let mut raw = String::with_capacity(CODE_LEN);
        for i in (0..BODY_SYMBOLS).rev() {
            raw.push(CROCKFORD[((value >> (i * 5)) & 31) as usize] as char);
        }
        raw.push(verhoeff_check(&raw));
        // Group into 4-character runs for readability: XXXX-XXXX-XXXX-XXXX.
        // FromStr strips the hyphens, so this still round-trips.
        let groups: Vec<&str> = raw
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        write!(f, "{}", groups.join("-"))
    }
}

impl std::fmt::Debug for FriendCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FriendCode({})", self)
    }
}

impl std::str::FromStr for FriendCode {
    type Err = FriendCodeError;

    fn from_str(s: &str) -> Result<Self, FriendCodeError> {
        // Normalize: drop separators, uppercase, fold the ambiguous Crockford
        // input aliases (I/L -> 1, O -> 0).
        let mut symbols = String::with_capacity(CODE_LEN);
        for c in s.chars() {
            let c = c.to_ascii_uppercase();
            match c {
                '-' | ' ' | '_' => continue,
                'I' | 'L' => symbols.push('1'),
                'O' => symbols.push('0'),
                _ if symbol_value(c).is_some() => symbols.push(c),
                _ => return Err(FriendCodeError::BadSymbol(c)),
            }
        }
        if symbols.len() != CODE_LEN {
            return Err(FriendCodeError::WrongLength(symbols.len()));
        }
        let (body, check) = symbols.split_at(BODY_SYMBOLS);
        if verhoeff_check(body) != check.chars().next().unwrap() {
            return Err(FriendCodeError::BadChecksum);
        }
        let value = body
            .chars()
            .fold(0u128, |acc, c| (acc << 5) | symbol_value(c).unwrap() as u128);
        Ok(FriendCode::from_value(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn value_roundtrips_through_string() {
        for v in [0u128, 1, 42, (1u128 << 75) - 1, 0x1234_5678_9abc_def0] {
            let fc = FriendCode::from_value(v);
            let s = fc.to_string();
            // 16 symbols grouped 4-4-4-4 with 3 hyphens.
            assert_eq!(s.len(), CODE_LEN + 3);
            assert_eq!(s.chars().filter(|&c| c == '-').count(), 3);
            let parsed = FriendCode::from_str(&s).unwrap();
            assert_eq!(parsed, fc);
            assert_eq!(parsed.value(), v & ((1u128 << 75) - 1));
        }
    }

    #[test]
    fn bytes_roundtrip() {
        let fc = FriendCode::from_value(0xdead_beef_cafe);
        assert_eq!(FriendCode::from_bytes(fc.as_bytes()).unwrap(), fc);
        assert_eq!(FriendCode::from_bytes(&[0u8; 3]), Err(FriendCodeError::WrongByteLen(3)));
    }

    #[test]
    fn rejects_corrupted_check_symbol() {
        let s = FriendCode::from_value(123_456_789).to_string();
        // The check symbol is the final character (no trailing hyphen).
        let mut chars: Vec<char> = s.chars().collect();
        let last_idx = chars.len() - 1;
        chars[last_idx] = if chars[last_idx] == '0' { '1' } else { '0' };
        let corrupted: String = chars.into_iter().collect();
        assert_eq!(FriendCode::from_str(&corrupted), Err(FriendCodeError::BadChecksum));
    }

    /// D16 must actually be a group: 0 is the identity and every element has an
    /// inverse on both sides. The Verhoeff guarantees rest on this.
    #[test]
    fn dihedral_group_laws() {
        for x in 0..32u32 {
            assert_eq!(dihedral_mul(0, x), x);
            assert_eq!(dihedral_mul(x, 0), x);
            assert_eq!(dihedral_mul(x, dihedral_inv(x)), 0);
            assert_eq!(dihedral_mul(dihedral_inv(x), x), 0);
        }
    }

    /// σ is a permutation of all 32 elements and a derangement (no fixed point).
    #[test]
    fn sigma_is_a_derangement_permutation() {
        let mut seen = [false; 32];
        for (i, &v) in SIGMA.iter().enumerate() {
            assert_ne!(v as usize, i, "σ fixes {i}");
            assert!(!seen[v as usize], "σ is not a permutation: {v} repeats");
            seen[v as usize] = true;
        }
    }

    /// The single property carrying all the detection guarantees: for every
    /// distinct pair, `x·σ(y) != y·σ(x)`. Re-proven here so the SIGMA table can
    /// never silently drift to a non-detecting permutation.
    #[test]
    fn sigma_is_totally_anti_symmetric() {
        let sigma = |z: u32| SIGMA[z as usize] as u32;
        for x in 0..32u32 {
            for y in 0..32u32 {
                if x != y {
                    assert_ne!(
                        dihedral_mul(x, sigma(y)),
                        dihedral_mul(y, sigma(x)),
                        "anti-symmetry fails at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// Every adjacent transposition of two *distinct* symbols — including the
    /// swap straddling the body/check-symbol boundary — must be rejected. This
    /// is exactly what Luhn could not guarantee.
    #[test]
    fn detects_adjacent_transpositions() {
        for v in [0u128, 1, 0x1234_5678_9abc_def0, (1u128 << 75) - 1, 0x0f0f_0f0f_0f0f] {
            let canonical = FriendCode::from_value(v).to_string();
            let mut symbols: Vec<char> = canonical.chars().filter(|&c| c != '-').collect();
            assert_eq!(symbols.len(), CODE_LEN);
            for i in 0..symbols.len() - 1 {
                if symbols[i] == symbols[i + 1] {
                    continue; // swapping equal symbols is a no-op
                }
                symbols.swap(i, i + 1);
                let swapped: String = symbols.iter().collect();
                assert_eq!(
                    FriendCode::from_str(&swapped),
                    Err(FriendCodeError::BadChecksum),
                    "transposition at {i} of {canonical} slipped through"
                );
                symbols.swap(i, i + 1); // restore for the next position
            }
        }
    }

    /// Every single-symbol substitution must be rejected.
    #[test]
    fn detects_single_symbol_errors() {
        for v in [0u128, 7, 0x1234_5678_9abc_def0, (1u128 << 75) - 1] {
            let canonical = FriendCode::from_value(v).to_string();
            let symbols: Vec<char> = canonical.chars().filter(|&c| c != '-').collect();
            for i in 0..symbols.len() {
                for &repl in CROCKFORD.iter() {
                    let repl = repl as char;
                    if repl == symbols[i] {
                        continue;
                    }
                    let mut corrupted = symbols.clone();
                    corrupted[i] = repl;
                    let s: String = corrupted.iter().collect();
                    assert_eq!(
                        FriendCode::from_str(&s),
                        Err(FriendCodeError::BadChecksum),
                        "single error at {i} ({repl}) of {canonical} slipped through"
                    );
                }
            }
        }
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(matches!(
            FriendCode::from_str("0123456789ABCD"),
            Err(FriendCodeError::WrongLength(14))
        ));
    }

    #[test]
    fn normalizes_separators_and_case() {
        let fc = FriendCode::from_value(0xdead_beef);
        let s = fc.to_string();
        assert!(s.contains('-'));
        // Lowercase and alternate separators both normalize back.
        assert_eq!(FriendCode::from_str(&s.to_lowercase()).unwrap(), fc);
        assert_eq!(FriendCode::from_str(&s.replace('-', " ")).unwrap(), fc);
    }
}
