use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use kelpie_hub_api::{Bytes32, PushKeys, PushSubscription};
use p256::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const SALT_LEN: usize = 16;
const POINT_LEN: usize = 65;
const TAG_LEN: usize = 16;

#[derive(Clone, Serialize, Deserialize)]
pub struct PushSecret {
    pub endpoint: String,
    private: Bytes32,
    auth: String,
}

impl PushSecret {
    pub fn generate(endpoint: &str) -> PushSecret {
        let private = loop {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).expect("the system random source failed");
            if SecretKey::from_slice(&bytes).is_ok() {
                break bytes;
            }
        };
        let mut auth = [0u8; 16];
        getrandom::fill(&mut auth).expect("the system random source failed");
        PushSecret {
            endpoint: endpoint.to_string(),
            private: Bytes32(private),
            auth: URL_SAFE_NO_PAD.encode(auth),
        }
    }

    fn secret(&self) -> SecretKey {
        SecretKey::from_slice(&self.private.0).expect("a stored push key is valid")
    }

    pub fn public(&self) -> Vec<u8> {
        self.secret().public_key().to_sec1_bytes().to_vec()
    }

    pub fn subscription(&self) -> PushSubscription {
        PushSubscription {
            endpoint: self.endpoint.clone(),
            keys: PushKeys {
                p256dh: URL_SAFE_NO_PAD.encode(self.public()),
                auth: self.auth.clone(),
            },
        }
    }

    pub fn decrypt(&self, body: &[u8]) -> Result<Vec<u8>, &'static str> {
        let auth = URL_SAFE_NO_PAD
            .decode(&self.auth)
            .map_err(|_| "the stored auth secret is not base64url")?;
        decrypt(&self.secret(), &auth, body)
    }
}

fn decrypt(receiver: &SecretKey, auth: &[u8], body: &[u8]) -> Result<Vec<u8>, &'static str> {
    if body.len() < SALT_LEN + 5 {
        return Err("shorter than an aes128gcm header");
    }
    let salt = &body[..SALT_LEN];
    let record_size = u32::from_be_bytes(body[SALT_LEN..SALT_LEN + 4].try_into().expect("4 bytes"));
    let id_len = usize::from(body[SALT_LEN + 4]);
    if id_len != POINT_LEN || body.len() < SALT_LEN + 5 + id_len + TAG_LEN + 1 {
        return Err("the key id is not an uncompressed P-256 point");
    }
    let sender_public = &body[SALT_LEN + 5..SALT_LEN + 5 + id_len];
    let record = &body[SALT_LEN + 5 + id_len..];
    if record.len() > usize::try_from(record_size).map_err(|_| "record size")? {
        return Err("more than one record");
    }
    let sender = PublicKey::from_sec1_bytes(sender_public).map_err(|_| "not a P-256 point")?;
    let receiver_public = receiver.public_key().to_sec1_bytes();
    let shared = receiver.diffie_hellman(&sender);
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend(receiver_public.iter());
    key_info.extend(sender_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .map_err(|_| "hkdf")?;
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| "hkdf")?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| "hkdf")?;
    let mut padded = Aes128Gcm::new(&Key::<Aes128Gcm>::from(cek))
        .decrypt(&Nonce::from(nonce), record)
        .map_err(|_| "the record does not decrypt")?;
    while padded.last() == Some(&0) {
        padded.pop();
    }
    match padded.pop() {
        Some(2) => Ok(padded),
        _ => Err("the record is not marked last"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn decrypts_the_rfc_8291_section_5_message() {
        let receiver = SecretKey::from_slice(&hex(
            "ab5757a70dd4a53e553a6bbf71ffefea2874ec07a6b379e3c48f895a02dc33de",
        ))
        .unwrap();
        let plaintext = decrypt(
            &receiver,
            &hex("05305932a1c7eabe13b6cec9fda48882"),
            &hex(
                "0c6bfaadad67958803092d454676f397000010004104fe33f4ab0dea71914db55823f73b54948f41306d920732dbb9a59a53286482200e597a7b7bc260ba1c227998580992e93973002f3012a28ae8f06bbb78e5ec0ff297de5b429bba7153d3a4ae0caa091fd425f3b4b5414add8ab37a19c1bbb05cf5cb5b2a2e0562d558635641ec52812c6c8ff42e95ccb86be7cd",
            ),
        )
        .unwrap();
        assert_eq!(plaintext, b"When I grow up, I want to be a watermelon");
    }
}
