//! The human-facing friend code: a 16-character Crockford-Base32 string with a
//! trailing Luhn mod-32 check symbol, encoding a 75-bit identifier.
//!
//! The identifier is derived server-side from the install's mTLS certificate
//! fingerprint and arrives over the wire as 10 raw bytes — the client never
//! computes it. All of the Crockford/Luhn encoding lives here, on the client;
//! the server never touches the alphabet or the check symbol.

/// Crockford Base32 alphabet (no I, L, O, U).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// 15 body symbols × 5 bits = 75 bits.
const BODY_SYMBOLS: usize = 15;
/// 15 body symbols + 1 Luhn check symbol.
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

/// Luhn mod-32 check symbol over the (already alphabet-normalized) body.
fn luhn_check(body: &str) -> char {
    let n = 32i64;
    let mut factor = 2i64;
    let mut sum = 0i64;
    for c in body.chars().rev() {
        let cp = symbol_value(c).expect("body is built from the alphabet") as i64;
        let mut addend = factor * cp;
        factor = if factor == 2 { 1 } else { 2 };
        addend = addend / n + addend % n;
        sum += addend;
    }
    CROCKFORD[((n - (sum % n)) % n) as usize] as char
}

impl std::fmt::Display for FriendCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = self.value();
        let mut raw = String::with_capacity(CODE_LEN);
        for i in (0..BODY_SYMBOLS).rev() {
            raw.push(CROCKFORD[((value >> (i * 5)) & 31) as usize] as char);
        }
        raw.push(luhn_check(&raw));
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
        if luhn_check(body) != check.chars().next().unwrap() {
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
