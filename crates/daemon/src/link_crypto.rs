//! Random secrets, hashes, and keyed code hashes for Machine links.

use crate::auth::{constant_time_eq, secure_random, AuthError};
use hmac::{Hmac, Mac};
use protocol::link::{PAIRING_CODE_ALPHABET, PAIRING_CODE_LENGTH};
use sha2::{Digest, Sha256};

/// A new 256-bit secret as 64 lowercase hex characters.
pub fn new_credential() -> Result<String, AuthError> {
    Ok(hex(&secure_random::<32>()?))
}

/// A non-secret handle such as `pc_9f2c1a0b7d3e4f51`.
pub fn new_handle(prefix: &str) -> Result<String, AuthError> {
    Ok(format!("{prefix}_{}", hex(&secure_random::<8>()?)))
}

/// A new 50-bit pairing code, normalized (10 symbols, no separator).
pub fn new_pairing_code() -> Result<String, AuthError> {
    // 10 random bytes, 5 bits each: every symbol is uniform over 32.
    let random = secure_random::<PAIRING_CODE_LENGTH>()?;
    Ok(random
        .iter()
        .map(|byte| char::from(PAIRING_CODE_ALPHABET[usize::from(byte & 0x1F)]))
        .collect())
}

/// `XXXXX-XXXXX` display form of a normalized code.
pub fn display_code(normalized: &str) -> String {
    let (head, tail) = normalized.split_at(normalized.len().min(5));
    format!("{head}-{tail}")
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(value: &str) -> String {
    hex(&Sha256::digest(value.as_bytes()))
}

/// Lowercase hex HMAC-SHA256 of `message` under `key`.
pub fn hmac_hex(key: &[u8], message: &str) -> String {
    // HMAC accepts keys of any length, so construction cannot fail.
    let mut mac = match Hmac::<Sha256>::new_from_slice(key) {
        Ok(mac) => mac,
        Err(_) => return String::new(),
    };
    mac.update(message.as_bytes());
    hex(&mac.finalize().into_bytes())
}

/// Whether `value` is a 64-character lowercase-hex credential.
pub fn is_credential(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Compare two hashes without early exit.
pub fn same_hash(left: &str, right: &str) -> bool {
    constant_time_eq(left, right)
}

/// Decode lowercase hex.
pub fn from_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(value.get(index..index + 2)?, 16).ok())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // RFC 4231 test case 2.
        assert_eq!(
            hmac_hex(b"Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn codes_and_credentials_have_the_promised_shape() {
        let code = new_pairing_code().unwrap();
        assert_eq!(code.len(), 10);
        assert_eq!(protocol::link::normalize_pairing_code(&code).unwrap(), code);
        assert_eq!(display_code(&code).len(), 11);
        assert!(is_credential(&new_credential().unwrap()));
        assert!(!is_credential("ABC"));
        assert_eq!(from_hex("00ff").unwrap(), vec![0, 255]);
    }
}
