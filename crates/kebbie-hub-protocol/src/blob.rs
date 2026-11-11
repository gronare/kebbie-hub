use std::fmt;

use aes_gcm::aead::{Aead, KeyInit, Payload as AeadInput};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::board::BlobRef;
use crate::ids::Id;

pub const BLOB_DOMAIN: &[u8] = b"kebbie-hub blob v1";
pub const MAX_BLOB_BYTES: usize = 256 * 1024;
pub const BLOB_OVERHEAD: usize = NONCE_LEN + TAG_LEN;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlobError {
    WrongId,
    WrongSize,
    BadKey,
    Undecryptable,
}

impl BlobError {
    pub fn key(self) -> &'static str {
        match self {
            BlobError::WrongId => "wrong-id",
            BlobError::WrongSize => "wrong-size",
            BlobError::BadKey => "bad-key",
            BlobError::Undecryptable => "undecryptable",
        }
    }
}

impl fmt::Display for BlobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

impl std::error::Error for BlobError {}

pub fn seal_blob(plaintext: &[u8], media_type: &str) -> (BlobRef, Vec<u8>) {
    let mut key = [0u8; 32];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut key).expect("the system random source failed");
    getrandom::fill(&mut nonce).expect("the system random source failed");
    seal_blob_with(plaintext, media_type, key, nonce)
}

pub fn seal_blob_with(
    plaintext: &[u8],
    media_type: &str,
    key: [u8; 32],
    nonce: [u8; NONCE_LEN],
) -> (BlobRef, Vec<u8>) {
    let sealed = Aes256Gcm::new(&Key::<Aes256Gcm>::from(key))
        .encrypt(
            &Nonce::from(nonce),
            AeadInput {
                msg: plaintext,
                aad: BLOB_DOMAIN,
            },
        )
        .expect("AES-GCM sealing cannot fail for a blob this size");
    let mut ciphertext = nonce.to_vec();
    ciphertext.extend(sealed);
    let blob = BlobRef {
        id: Id::blob(&ciphertext),
        key: URL_SAFE_NO_PAD.encode(key),
        media_type: media_type.to_string(),
        bytes: plaintext.len() as u64,
    };
    (blob, ciphertext)
}

pub fn open_blob(blob: &BlobRef, ciphertext: &[u8]) -> Result<Vec<u8>, BlobError> {
    if Id::blob(ciphertext) != blob.id {
        return Err(BlobError::WrongId);
    }
    if ciphertext.len() != blob.bytes as usize + BLOB_OVERHEAD {
        return Err(BlobError::WrongSize);
    }
    let key: [u8; 32] = URL_SAFE_NO_PAD
        .decode(&blob.key)
        .ok()
        .and_then(|key| key.try_into().ok())
        .ok_or(BlobError::BadKey)?;
    let (nonce, sealed) = ciphertext.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().expect("split at the nonce length");
    Aes256Gcm::new(&Key::<Aes256Gcm>::from(key))
        .decrypt(
            &Nonce::from(nonce),
            AeadInput {
                msg: sealed,
                aad: BLOB_DOMAIN,
            },
        )
        .map_err(|_| BlobError::Undecryptable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_blob_opens_with_its_ref_and_only_its_own_ciphertext() {
        let plan = "## Steps\n1. Add the gateway".as_bytes();
        let (blob, ciphertext) = seal_blob(plan, "text/markdown");
        assert_eq!(blob.bytes, plan.len() as u64);
        assert_eq!(ciphertext.len(), plan.len() + BLOB_OVERHEAD);
        assert_eq!(open_blob(&blob, &ciphertext).unwrap(), plan);
        let mut flipped = ciphertext.clone();
        flipped[20] ^= 1;
        assert_eq!(open_blob(&blob, &flipped), Err(BlobError::WrongId));
        let (other, _) = seal_blob(plan, "text/markdown");
        let wrong_key = BlobRef { key: other.key, ..blob.clone() };
        assert_eq!(open_blob(&wrong_key, &ciphertext), Err(BlobError::Undecryptable));
        let wrong_size = BlobRef { bytes: 3, ..blob };
        assert_eq!(open_blob(&wrong_size, &ciphertext), Err(BlobError::WrongSize));
    }
}
