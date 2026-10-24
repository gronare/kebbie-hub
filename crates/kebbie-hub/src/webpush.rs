use std::io::Write;
use std::net::IpAddr;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::{PublicKey, SecretKey};
use sha2::{Digest, Sha256};
use webauthn_rs::prelude::Url;

use crate::auth::random;

pub const KNOWN_PUSH_HOSTS: &[&str] = &[
    "web.push.apple.com",
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    "*.notify.windows.com",
];
pub const MAX_PUSH_BODY: usize = 4096;
const SALT_LEN: usize = 16;
const POINT_LEN: usize = 65;
const AUTH_LEN: usize = 16;
const TAG_LEN: usize = 16;
const RECORD_SIZE: u32 = 4096;
const HEADER_LEN: usize = SALT_LEN + 4 + 1 + POINT_LEN;
pub const MAX_PUSH_PLAINTEXT: usize = MAX_PUSH_BODY - HEADER_LEN - 1 - TAG_LEN;
const MAX_ENDPOINT_CHARS: usize = 2048;
const MAX_TOPIC_CHARS: usize = 32;
const RESERVED_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "invalid",
    "test",
    "example",
    "internal",
    "lan",
    "home",
    "arpa",
];

#[derive(Clone)]
pub struct SubscriptionKeys {
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
}

fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

pub fn parse_keys(p256dh: &str, auth: &str) -> Option<SubscriptionKeys> {
    let p256dh = decode(p256dh)?;
    let auth = decode(auth)?;
    let valid = p256dh.len() == POINT_LEN
        && p256dh[0] == 4
        && PublicKey::from_sec1_bytes(&p256dh).is_ok()
        && auth.len() == AUTH_LEN;
    valid.then_some(SubscriptionKeys { p256dh, auth })
}

pub fn host_matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => host
            .strip_suffix(suffix)
            .and_then(|head| head.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty()),
        None => host == pattern,
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub fn check_endpoint(endpoint: &str, allowed: &[String]) -> Result<(), &'static str> {
    if endpoint.len() > MAX_ENDPOINT_CHARS {
        return Err("bad-endpoint");
    }
    let url = Url::parse(endpoint).map_err(|_| "bad-endpoint")?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("bad-endpoint");
    }
    let host = url.host_str().ok_or("bad-endpoint")?;
    let known = KNOWN_PUSH_HOSTS
        .iter()
        .any(|pattern| host_matches(pattern, host));
    let extra = allowed.iter().any(|pattern| host_matches(pattern, host));
    match url.scheme() {
        "https" if known && url.port().is_none() => Ok(()),
        "https" if extra => Ok(()),
        "http" if extra && is_loopback(host) => Ok(()),
        "http" if known || extra => Err("endpoint-not-https"),
        "https" | "http" => Err("endpoint-not-allowed"),
        _ => Err("bad-endpoint"),
    }
}

pub fn audience(endpoint: &str) -> Option<String> {
    Url::parse(endpoint)
        .ok()
        .map(|url| url.origin().ascii_serialization())
}

fn is_public_domain(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && host.parse::<IpAddr>().is_err()
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        && !RESERVED_SUFFIXES.contains(labels.last().expect("two labels or more"))
}

pub fn check_subject(subject: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "the VAPID subject must be a real mailto: address or https: URL, like mailto:you@example.com (got {subject:?}); push services such as Apple's refuse anything else"
        ))
    };
    if let Some(address) = subject.strip_prefix("mailto:") {
        return match address.split_once('@') {
            Some((local, domain))
                if !local.is_empty()
                    && !local.contains(char::is_whitespace)
                    && is_public_domain(domain) =>
            {
                Ok(())
            }
            _ => refuse(),
        };
    }
    match Url::parse(subject) {
        Ok(url) if url.scheme() == "https" && url.host_str().is_some_and(is_public_domain) => {
            Ok(())
        }
        _ => refuse(),
    }
}

pub fn topic(collapse: &str) -> String {
    if collapse.len() <= MAX_TOPIC_CHARS {
        return collapse.to_string();
    }
    let mut hashed = URL_SAFE_NO_PAD.encode(Sha256::digest(collapse.as_bytes()));
    hashed.truncate(MAX_TOPIC_CHARS);
    hashed
}

#[cfg(unix)]
fn shared_with_others(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map_or(true, |meta| meta.permissions().mode() & 0o077 != 0)
}

pub struct Vapid {
    key: SigningKey,
    public: String,
}

impl Vapid {
    pub fn from_secret(secret: &[u8]) -> Option<Vapid> {
        let key = SigningKey::from_slice(secret).ok()?;
        let public = URL_SAFE_NO_PAD.encode(key.verifying_key().to_sec1_point(false).as_bytes());
        Some(Vapid { key, public })
    }

    pub fn load_or_create(path: &Path) -> Result<Vapid, String> {
        let fail = |error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
        match std::fs::read_to_string(path) {
            #[cfg(unix)]
            Ok(_) if shared_with_others(path) => Err(fail(
                &"the VAPID key file is readable by group or others; chmod 600 it, or delete it to make a new key if it may have leaked",
            )),
            Ok(text) => decode(text.trim())
                .and_then(|secret| Vapid::from_secret(&secret))
                .ok_or_else(|| fail(&"not a VAPID key written by kebbie-hub")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let (vapid, secret) = loop {
                    let secret = random::<32>();
                    if let Some(vapid) = Vapid::from_secret(&secret) {
                        break (vapid, secret);
                    }
                };
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| fail(&error))?;
                }
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
                let mut file = options.open(path).map_err(|error| fail(&error))?;
                writeln!(file, "{}", URL_SAFE_NO_PAD.encode(secret))
                    .and_then(|()| file.sync_all())
                    .map_err(|error| fail(&error))?;
                Ok(vapid)
            }
            Err(error) => Err(fail(&error)),
        }
    }

    pub fn public_key(&self) -> &str {
        &self.public
    }

    pub fn authorization(&self, audience: &str, subject: &str, expires_at: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({ "aud": audience, "exp": expires_at, "sub": subject });
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signing_input = format!("{header}.{claims}");
        let signature: Signature = self.key.sign(signing_input.as_bytes());
        format!(
            "vapid t={signing_input}.{}, k={}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes()),
            self.public
        )
    }
}

pub fn encrypt(keys: &SubscriptionKeys, plaintext: &[u8]) -> Option<Vec<u8>> {
    let sender = loop {
        if let Ok(secret) = SecretKey::from_slice(&random::<32>()) {
            break secret;
        }
    };
    encrypt_with(keys, plaintext, &sender, random())
}

fn encrypt_with(
    keys: &SubscriptionKeys,
    plaintext: &[u8],
    sender: &SecretKey,
    salt: [u8; SALT_LEN],
) -> Option<Vec<u8>> {
    if plaintext.len() > MAX_PUSH_PLAINTEXT {
        return None;
    }
    let receiver = PublicKey::from_sec1_bytes(&keys.p256dh).ok()?;
    let sender_public = sender.public_key().to_sec1_bytes();
    let shared = sender.diffie_hellman(&receiver);
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend(&keys.p256dh);
    key_info.extend(sender_public.iter());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&keys.auth), shared.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .ok()?;
    let prk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .ok()?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
    let mut record = plaintext.to_vec();
    record.push(2);
    let sealed = Aes128Gcm::new(&Key::<Aes128Gcm>::from(cek))
        .encrypt(&Nonce::from(nonce), record.as_slice())
        .ok()?;
    let mut body = salt.to_vec();
    body.extend(RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(sender_public.len()).ok()?);
    body.extend(sender_public.iter());
    body.extend(sealed);
    Some(body)
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
    fn encryption_matches_rfc_8291_section_5() {
        let keys = SubscriptionKeys {
            p256dh: hex(
                "042571b2becdfde360551aaf1ed0f4cd366c11cebe555f89bcb7b186a53339173168ece2ebe018597bd30479b86e3c8f8eced577ca59187e9246990db682008b0e",
            ),
            auth: hex("05305932a1c7eabe13b6cec9fda48882"),
        };
        let sender = SecretKey::from_slice(&hex(
            "c9f58f89813e9f8e872e71f42aa64e1757c9254dcc62b72ddc010bb4043ea11c",
        ))
        .unwrap();
        let salt: [u8; 16] = hex("0c6bfaadad67958803092d454676f397").try_into().unwrap();
        let body = encrypt_with(
            &keys,
            b"When I grow up, I want to be a watermelon",
            &sender,
            salt,
        )
        .unwrap();
        assert_eq!(
            body,
            hex(
                "0c6bfaadad67958803092d454676f397000010004104fe33f4ab0dea71914db55823f73b54948f41306d920732dbb9a59a53286482200e597a7b7bc260ba1c227998580992e93973002f3012a28ae8f06bbb78e5ec0ff297de5b429bba7153d3a4ae0caa091fd425f3b4b5414add8ab37a19c1bbb05cf5cb5b2a2e0562d558635641ec52812c6c8ff42e95ccb86be7cd"
            )
        );
    }

    #[test]
    fn the_largest_plaintext_fills_the_push_limit_exactly() {
        let receiver = SecretKey::from_slice(&[7; 32]).unwrap();
        let keys = SubscriptionKeys {
            p256dh: receiver.public_key().to_sec1_bytes().to_vec(),
            auth: vec![1; 16],
        };
        let body = encrypt(&keys, &vec![b'x'; MAX_PUSH_PLAINTEXT]).unwrap();
        assert_eq!(body.len(), MAX_PUSH_BODY);
        assert!(encrypt(&keys, &vec![b'x'; MAX_PUSH_PLAINTEXT + 1]).is_none());
    }

    #[test]
    fn endpoints_must_be_https_on_a_known_push_service() {
        let none: Vec<String> = Vec::new();
        for endpoint in [
            "https://web.push.apple.com/QGuQyavXutnMH",
            "https://fcm.googleapis.com/fcm/send/abc:def",
            "https://updates.push.services.mozilla.com/wpush/v2/gAAAA",
            "https://wns2-par02p.notify.windows.com/w/?token=AwYAAA",
        ] {
            assert_eq!(check_endpoint(endpoint, &none), Ok(()), "{endpoint}");
        }
        for (endpoint, refusal) in [
            ("http://fcm.googleapis.com/fcm/send/x", "endpoint-not-https"),
            (
                "https://fcm.googleapis.com.evil.example/x",
                "endpoint-not-allowed",
            ),
            ("https://notify.windows.com/x", "endpoint-not-allowed"),
            ("https://fcm.googleapis.com:8443/x", "endpoint-not-allowed"),
            ("https://user@fcm.googleapis.com/x", "bad-endpoint"),
            ("ftp://fcm.googleapis.com/x", "bad-endpoint"),
            ("http://127.0.0.1:9000/push", "endpoint-not-allowed"),
            ("not a url", "bad-endpoint"),
        ] {
            assert_eq!(check_endpoint(endpoint, &none), Err(refusal), "{endpoint}");
        }
        let local = vec!["127.0.0.1".to_string(), "push.example.org".to_string()];
        assert_eq!(check_endpoint("http://127.0.0.1:9000/push", &local), Ok(()));
        assert_eq!(
            check_endpoint("https://push.example.org:8443/x", &local),
            Ok(())
        );
        assert_eq!(
            check_endpoint("http://push.example.org/x", &local),
            Err("endpoint-not-https")
        );
    }

    #[test]
    fn the_vapid_subject_is_a_real_address() {
        for subject in [
            "mailto:carl@example.com",
            "https://hub.example.com",
            "https://github.com/gronare/kebbie-hub",
        ] {
            assert_eq!(check_subject(subject), Ok(()), "{subject}");
        }
        for subject in [
            "mailto:admin@herdr.local",
            "mailto:carl@localhost",
            "mailto:@example.com",
            "carl@example.com",
            "http://hub.example.com",
            "https://192.0.2.1",
            "https://hub.internal",
        ] {
            assert!(check_subject(subject).is_err(), "{subject}");
        }
    }

    #[test]
    fn long_collapse_keys_become_a_32_character_topic() {
        assert_eq!(topic("short_tag-1"), "short_tag-1");
        let long = "a".repeat(43);
        let hashed = topic(&long);
        assert_eq!(hashed.len(), 32);
        assert!(
            hashed
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
        assert_ne!(hashed, topic(&"b".repeat(43)));
    }
}
