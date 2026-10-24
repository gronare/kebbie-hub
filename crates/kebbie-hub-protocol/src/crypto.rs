use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;

use aes_gcm::aead::{Aead, KeyInit, Payload as AeadInput};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hpke::aead::AesGcm128;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::envelope::{Envelope, Header, ShapeError};
use crate::ids::Id;
use crate::message::Payload;

type Suite = X25519HkdfSha256;

pub const HPKE_INFO: &[u8] = b"kelpie-hub content key v1";
pub const CLOCK_SKEW: u64 = 300;
const KEY_COMMIT_DOMAIN: &[u8] = b"kelpie-hub key commit v1";
const ENC_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct SecretKeys {
    signing: SigningKey,
    encryption: <Suite as Kem>::PrivateKey,
}

impl SecretKeys {
    pub fn generate() -> SecretKeys {
        let mut signing = [0u8; 32];
        let mut encryption = [0u8; 32];
        getrandom::fill(&mut signing).expect("the system random source failed");
        getrandom::fill(&mut encryption).expect("the system random source failed");
        SecretKeys::from_bytes(signing, encryption)
    }

    pub fn from_bytes(signing: [u8; 32], encryption: [u8; 32]) -> SecretKeys {
        let encryption = <Suite as Kem>::PrivateKey::from_bytes(&encryption)
            .expect("an X25519 private key is any 32 bytes");
        SecretKeys {
            signing: SigningKey::from_bytes(&signing),
            encryption,
        }
    }

    pub fn to_bytes(&self) -> ([u8; 32], [u8; 32]) {
        (self.signing.to_bytes(), self.encryption.to_bytes().into())
    }

    pub fn public(&self) -> PublicKeys {
        PublicKeys {
            signing: self.signing.verifying_key().to_bytes(),
            encryption: Suite::sk_to_pk(&self.encryption).to_bytes().into(),
        }
    }
}

impl fmt::Debug for SecretKeys {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretKeys")
            .field("public", &self.public())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeys {
    #[serde(with = "b64_32")]
    pub signing: [u8; 32],
    #[serde(with = "b64_32")]
    pub encryption: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct Randomness {
    pub content_key: [u8; 32],
    pub nonce: [u8; NONCE_LEN],
    pub ephemeral: BTreeMap<Id, [u8; 32]>,
}

impl Randomness {
    fn draw(recipients: &[(Id, PublicKeys)]) -> Randomness {
        let mut randomness = Randomness {
            content_key: [0; 32],
            nonce: [0; NONCE_LEN],
            ephemeral: BTreeMap::new(),
        };
        getrandom::fill(&mut randomness.content_key).expect("the system random source failed");
        getrandom::fill(&mut randomness.nonce).expect("the system random source failed");
        for (id, _) in recipients {
            let mut ikm = [0u8; 32];
            getrandom::fill(&mut ikm).expect("the system random source failed");
            randomness.ephemeral.insert(id.clone(), ikm);
        }
        randomness
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SealError {
    Shape(ShapeError),
    KindMismatch,
    NoRecipients,
    DuplicateRecipient(Id),
    MissingRandomness(Id),
}

impl fmt::Display for SealError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SealError::Shape(shape) => write!(formatter, "the header is not valid: {shape:?}"),
            SealError::KindMismatch => {
                formatter.write_str("the payload does not match the header's kind")
            }
            SealError::NoRecipients => {
                formatter.write_str("an envelope needs at least one recipient")
            }
            SealError::DuplicateRecipient(id) => write!(formatter, "{id} is listed twice"),
            SealError::MissingRandomness(id) => {
                write!(formatter, "no ephemeral key material for {id}")
            }
        }
    }
}

impl std::error::Error for SealError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    UnsupportedVersion,
    UnsupportedSchema,
    Malformed,
    WrongSender,
    BadSignature,
    Expired,
    NotYetValid,
    NotARecipient,
    Undecryptable,
    KeyMismatch,
    KindMismatch,
}

impl OpenError {
    pub fn key(self) -> &'static str {
        match self {
            OpenError::UnsupportedVersion => "unsupported-version",
            OpenError::UnsupportedSchema => "unsupported-schema",
            OpenError::Malformed => "malformed",
            OpenError::WrongSender => "wrong-sender",
            OpenError::BadSignature => "bad-signature",
            OpenError::Expired => "expired",
            OpenError::NotYetValid => "not-yet-valid",
            OpenError::NotARecipient => "not-a-recipient",
            OpenError::Undecryptable => "undecryptable",
            OpenError::KeyMismatch => "key-mismatch",
            OpenError::KindMismatch => "kind-mismatch",
        }
    }
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

impl std::error::Error for OpenError {}

impl From<ShapeError> for OpenError {
    fn from(shape: ShapeError) -> OpenError {
        match shape {
            ShapeError::UnsupportedVersion => OpenError::UnsupportedVersion,
            ShapeError::UnsupportedSchema => OpenError::UnsupportedSchema,
            ShapeError::Malformed(_) => OpenError::Malformed,
        }
    }
}

pub fn key_commitment(content_key: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(
        Sha256::new()
            .chain_update(KEY_COMMIT_DOMAIN)
            .chain_update(content_key)
            .finalize(),
    )
}

pub fn seal(
    header: Header,
    payload: &Payload,
    sender: &SecretKeys,
    recipients: &[(Id, PublicKeys)],
) -> Result<Envelope, SealError> {
    seal_with(
        header,
        payload,
        sender,
        recipients,
        &Randomness::draw(recipients),
    )
}

pub(crate) fn seal_with(
    mut header: Header,
    payload: &Payload,
    sender: &SecretKeys,
    recipients: &[(Id, PublicKeys)],
    randomness: &Randomness,
) -> Result<Envelope, SealError> {
    header.kc = key_commitment(&randomness.content_key);
    header.check_shape().map_err(SealError::Shape)?;
    if payload.kind() != header.kind {
        return Err(SealError::KindMismatch);
    }
    if recipients.is_empty() {
        return Err(SealError::NoRecipients);
    }
    for (index, (id, _)) in recipients.iter().enumerate() {
        if recipients[..index].iter().any(|(earlier, _)| earlier == id) {
            return Err(SealError::DuplicateRecipient(id.clone()));
        }
        if !randomness.ephemeral.contains_key(id) {
            return Err(SealError::MissingRandomness(id.clone()));
        }
    }
    let plaintext = serde_json::to_vec(payload).expect("a payload always serializes");
    Ok(seal_unchecked(
        header, &plaintext, sender, recipients, randomness,
    ))
}

pub(crate) fn seal_unchecked(
    header: Header,
    plaintext: &[u8],
    sender: &SecretKeys,
    recipients: &[(Id, PublicKeys)],
    randomness: &Randomness,
) -> Envelope {
    let aad = header.bytes();
    let keys = recipients
        .iter()
        .map(|(id, public)| {
            (
                id.clone(),
                URL_SAFE_NO_PAD.encode(wrap_key(
                    &randomness.content_key,
                    &public.encryption,
                    &aad,
                    randomness.ephemeral[id],
                )),
            )
        })
        .collect();
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(randomness.content_key));
    let sealed = cipher
        .encrypt(
            &Nonce::from(randomness.nonce),
            AeadInput {
                msg: plaintext,
                aad: &aad,
            },
        )
        .expect("AES-GCM sealing cannot fail for a payload this size");
    let mut ct = randomness.nonce.to_vec();
    ct.extend(sealed);
    let sig = sender.signing.sign(&signed_bytes(&aad, &ct));
    Envelope {
        header,
        keys,
        ct: URL_SAFE_NO_PAD.encode(&ct),
        sig: URL_SAFE_NO_PAD.encode(sig.to_bytes()),
    }
}

pub(crate) fn wrap_key(
    content_key: &[u8; 32],
    recipient: &[u8; 32],
    aad: &[u8],
    ikm: [u8; 32],
) -> Vec<u8> {
    let recipient = <Suite as Kem>::PublicKey::from_bytes(recipient)
        .expect("an X25519 public key is any 32 bytes");
    let mut ephemeral = OneShot(Some(ikm));
    let (enc, wrapped) = hpke::single_shot_seal_with_rng::<AesGcm128, HkdfSha256, Suite>(
        &OpModeS::Base,
        &recipient,
        HPKE_INFO,
        content_key,
        aad,
        &mut ephemeral,
    )
    .expect("HPKE sealing to a valid X25519 key cannot fail");
    let mut row = enc.to_bytes().to_vec();
    row.extend(wrapped);
    row
}

pub fn open(
    envelope: &Envelope,
    recipient: &Id,
    keys: &SecretKeys,
    sender: &Id,
    sender_keys: &PublicKeys,
    now: u64,
) -> Result<Payload, OpenError> {
    verify(envelope, sender, sender_keys, now)?;
    decrypt(envelope, recipient, keys)
}

pub fn verify(
    envelope: &Envelope,
    sender: &Id,
    sender_keys: &PublicKeys,
    now: u64,
) -> Result<(), OpenError> {
    verify_signature(envelope, sender, sender_keys)?;
    check_time(&envelope.header, now)
}

pub(crate) fn verify_signature(
    envelope: &Envelope,
    sender: &Id,
    sender_keys: &PublicKeys,
) -> Result<(), OpenError> {
    let header = &envelope.header;
    header.check_shape()?;
    if &header.from != sender {
        return Err(OpenError::WrongSender);
    }
    let ct = decode(&envelope.ct)?;
    let sig: [u8; 64] = decode(&envelope.sig)?
        .try_into()
        .map_err(|_| OpenError::Malformed)?;
    let key =
        VerifyingKey::from_bytes(&sender_keys.signing).map_err(|_| OpenError::BadSignature)?;
    key.verify_strict(
        &signed_bytes(&header.bytes(), &ct),
        &Signature::from_bytes(&sig),
    )
    .map_err(|_| OpenError::BadSignature)
}

pub(crate) fn check_time(header: &Header, now: u64) -> Result<(), OpenError> {
    if now >= header.exp {
        return Err(OpenError::Expired);
    }
    if header.at > now + CLOCK_SKEW {
        return Err(OpenError::NotYetValid);
    }
    Ok(())
}

pub(crate) fn decrypt(
    envelope: &Envelope,
    recipient: &Id,
    keys: &SecretKeys,
) -> Result<Payload, OpenError> {
    let aad = envelope.header.bytes();
    let row = decode(
        envelope
            .keys
            .get(recipient)
            .ok_or(OpenError::NotARecipient)?,
    )?;
    if row.len() <= ENC_LEN {
        return Err(OpenError::Malformed);
    }
    let enc = <Suite as Kem>::EncappedKey::from_bytes(&row[..ENC_LEN])
        .map_err(|_| OpenError::Malformed)?;
    let content_key: [u8; 32] = hpke::single_shot_open::<AesGcm128, HkdfSha256, Suite>(
        &OpModeR::Base,
        &keys.encryption,
        &enc,
        HPKE_INFO,
        &row[ENC_LEN..],
        &aad,
    )
    .map_err(|_| OpenError::Undecryptable)?
    .try_into()
    .map_err(|_| OpenError::Undecryptable)?;
    if key_commitment(&content_key) != envelope.header.kc {
        return Err(OpenError::KeyMismatch);
    }
    let ct = decode(&envelope.ct)?;
    if ct.len() <= NONCE_LEN {
        return Err(OpenError::Malformed);
    }
    let nonce: [u8; NONCE_LEN] = ct[..NONCE_LEN].try_into().expect("the length was checked");
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(content_key));
    let plaintext = cipher
        .decrypt(
            &Nonce::from(nonce),
            AeadInput {
                msg: &ct[NONCE_LEN..],
                aad: &aad,
            },
        )
        .map_err(|_| OpenError::Undecryptable)?;
    let payload: Payload = serde_json::from_slice(&plaintext).map_err(|_| OpenError::Malformed)?;
    if payload.kind() != envelope.header.kind {
        return Err(OpenError::KindMismatch);
    }
    Ok(payload)
}

fn signed_bytes(header: &[u8], ct: &[u8]) -> Vec<u8> {
    [header, ct].concat()
}

fn decode(text: &str) -> Result<Vec<u8>, OpenError> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| OpenError::Malformed)
}

struct OneShot(Option<[u8; 32]>);

impl rand_core::TryRng for OneShot {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        unreachable!("HPKE draws its ephemeral key as one 32-byte fill")
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        unreachable!("HPKE draws its ephemeral key as one 32-byte fill")
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        let ikm = self
            .0
            .take()
            .expect("HPKE draws its ephemeral key once per recipient");
        assert_eq!(
            dst.len(),
            ikm.len(),
            "HPKE draws its ephemeral key as one 32-byte fill"
        );
        dst.copy_from_slice(&ikm);
        Ok(())
    }
}

impl rand_core::TryCryptoRng for OneShot {}

mod b64_32 {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 32], D::Error> {
        let text = String::deserialize(deserializer)?;
        URL_SAFE_NO_PAD
            .decode(&text)
            .map_err(D::Error::custom)?
            .try_into()
            .map_err(|_| D::Error::custom("expected 32 bytes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Kind;
    use crate::ids::Prefix;
    use crate::message::RunRoutine;

    #[test]
    fn seal_holds_the_ttl_cap_and_needs_distinct_recipients() {
        let machine = Id::ulid(Prefix::Machine, 1, [1; 10]).unwrap();
        let device = Id::ulid(Prefix::Device, 1, [2; 10]).unwrap();
        let keys = SecretKeys::generate();
        let payload = Payload::Routine(RunRoutine {
            id: "nightly-deps".into(),
        });
        let header = |ttl: u64| {
            let mut header = Header::new(
                Id::new(Prefix::Command).unwrap(),
                Kind::Routine,
                device.clone(),
                machine.clone(),
                1_000,
            );
            header.exp = header.at + ttl;
            header
        };
        let to_machine = [(machine.clone(), keys.public())];
        assert!(
            seal(
                header(Kind::Routine.max_ttl()),
                &payload,
                &keys,
                &to_machine
            )
            .is_ok()
        );
        assert!(matches!(
            seal(
                header(Kind::Routine.max_ttl() + 1),
                &payload,
                &keys,
                &to_machine
            ),
            Err(SealError::Shape(_))
        ));
        assert_eq!(
            seal(header(60), &payload, &keys, &[]),
            Err(SealError::NoRecipients)
        );
        let twice = [
            (machine.clone(), keys.public()),
            (machine.clone(), keys.public()),
        ];
        assert_eq!(
            seal(header(60), &payload, &keys, &twice),
            Err(SealError::DuplicateRecipient(machine.clone()))
        );
    }
}
