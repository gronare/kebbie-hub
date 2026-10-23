use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const REPORT_DOMAIN: &[u8] = b"kelpie-report-v1";
pub const REPORT_SIGN_DOMAIN: &[u8] = b"kelpie-report-sign-v1";
pub const REPORT_POW_DOMAIN: &[u8] = b"kelpie-report-pow-v1";
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_TEXT_BYTES: usize = 12 * 1024;
pub const MAX_FIELD_CHARS: usize = 64;
pub const MAX_DIFFICULTY: u8 = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BugReport {
    pub title: String,
    pub text: String,
    pub version: String,
    pub os: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BugReportError {
    BadTitle,
    TooLarge,
    BadField,
}

impl BugReportError {
    pub fn key(self) -> &'static str {
        match self {
            BugReportError::BadTitle => "bad-title",
            BugReportError::TooLarge => "too-large",
            BugReportError::BadField => "bad-field",
        }
    }
}

impl fmt::Display for BugReportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

impl std::error::Error for BugReportError {}

fn short_line(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit && !text.chars().any(char::is_control)
}

impl BugReport {
    pub fn check(&self) -> Result<(), BugReportError> {
        if self.title.chars().count() > MAX_TITLE_CHARS || self.text.len() > MAX_TEXT_BYTES {
            return Err(BugReportError::TooLarge);
        }
        if !short_line(&self.title, MAX_TITLE_CHARS) {
            return Err(BugReportError::BadTitle);
        }
        if !short_line(&self.version, MAX_FIELD_CHARS) || !short_line(&self.os, MAX_FIELD_CHARS) {
            return Err(BugReportError::BadField);
        }
        Ok(())
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = REPORT_DOMAIN.to_vec();
        for field in [&self.title, &self.text, &self.version, &self.os] {
            bytes.extend((field.len() as u32).to_be_bytes());
            bytes.extend(field.as_bytes());
        }
        bytes
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.bytes()).into()
    }
}

pub fn pow_hash(nonce: &[u8; 32], digest: &[u8; 32], counter: u64) -> [u8; 32] {
    Sha256::new()
        .chain_update(REPORT_POW_DOMAIN)
        .chain_update(nonce)
        .chain_update(digest)
        .chain_update(counter.to_be_bytes())
        .finalize()
        .into()
}

pub fn leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut bits = 0;
    for byte in hash {
        if *byte == 0 {
            bits += 8;
        } else {
            return bits + byte.leading_zeros();
        }
    }
    bits
}

pub fn meets(nonce: &[u8; 32], digest: &[u8; 32], counter: u64, difficulty: u8) -> bool {
    leading_zero_bits(&pow_hash(nonce, digest, counter)) >= u32::from(difficulty)
}

pub fn solve(nonce: &[u8; 32], digest: &[u8; 32], difficulty: u8) -> u64 {
    (0..u64::MAX)
        .find(|counter| meets(nonce, digest, *counter, difficulty))
        .expect("some counter meets any difficulty up to 64 bits")
}

fn signed_bytes(nonce: &[u8; 32], report: &BugReport) -> Vec<u8> {
    let mut bytes = REPORT_SIGN_DOMAIN.to_vec();
    bytes.extend(nonce);
    bytes.extend(report.digest());
    bytes
}

#[derive(Clone)]
pub struct InstallKey(SigningKey);

impl InstallKey {
    pub fn from_bytes(secret: [u8; 32]) -> InstallKey {
        InstallKey(SigningKey::from_bytes(&secret))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn public(&self) -> [u8; 32] {
        self.0.verifying_key().to_bytes()
    }

    pub fn sign(&self, nonce: &[u8; 32], report: &BugReport) -> [u8; 64] {
        self.0.sign(&signed_bytes(nonce, report)).to_bytes()
    }
}

impl fmt::Debug for InstallKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstallKey")
            .field("public", &self.public())
            .finish_non_exhaustive()
    }
}

pub fn verify_report(
    public: &[u8; 32],
    nonce: &[u8; 32],
    report: &BugReport,
    signature: &[u8; 64],
) -> bool {
    VerifyingKey::from_bytes(public).is_ok_and(|key| {
        key.verify_strict(
            &signed_bytes(nonce, report),
            &Signature::from_bytes(signature),
        )
        .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> BugReport {
        BugReport {
            title: "push hid git's error".to_string(),
            text: "Ran kelpie push; nothing showed.".to_string(),
            version: "0.26.0 (e58e84a)".to_string(),
            os: "macos aarch64".to_string(),
        }
    }

    #[test]
    fn leading_zero_bits_counts_across_bytes() {
        let mut hash = [0xffu8; 32];
        assert_eq!(leading_zero_bits(&hash), 0);
        hash[0] = 0;
        hash[1] = 0x1f;
        assert_eq!(leading_zero_bits(&hash), 11);
        assert_eq!(leading_zero_bits(&[0; 32]), 256);
    }

    #[test]
    fn a_solved_counter_meets_its_difficulty_and_the_one_before_does_not() {
        let digest = report().digest();
        let nonce = [7u8; 32];
        let counter = solve(&nonce, &digest, 12);
        assert!(meets(&nonce, &digest, counter, 12));
        assert!((0..counter).all(|earlier| !meets(&nonce, &digest, earlier, 12)));
    }

    #[test]
    fn a_signature_covers_the_nonce_and_every_field() {
        let key = InstallKey::from_bytes([3u8; 32]);
        let nonce = [9u8; 32];
        let signature = key.sign(&nonce, &report());
        assert!(verify_report(&key.public(), &nonce, &report(), &signature));
        assert!(!verify_report(
            &key.public(),
            &[1u8; 32],
            &report(),
            &signature
        ));
        for change in [
            |report: &mut BugReport| report.title.push('!'),
            |report: &mut BugReport| report.text.push('!'),
            |report: &mut BugReport| report.version.push('!'),
            |report: &mut BugReport| report.os.push('!'),
        ] {
            let mut changed = report();
            change(&mut changed);
            assert!(!verify_report(&key.public(), &nonce, &changed, &signature));
        }
        let other = InstallKey::from_bytes([4u8; 32]);
        assert!(!verify_report(
            &other.public(),
            &nonce,
            &report(),
            &signature
        ));
    }

    #[test]
    fn fields_are_length_prefixed_so_moving_text_between_them_changes_the_bytes() {
        let one = BugReport {
            title: "ab".to_string(),
            text: "c".to_string(),
            ..report()
        };
        let two = BugReport {
            title: "a".to_string(),
            text: "bc".to_string(),
            ..report()
        };
        assert_ne!(one.bytes(), two.bytes());
    }

    #[test]
    fn a_report_is_checked_for_size_and_shape() {
        assert_eq!(report().check(), Ok(()));
        assert_eq!(
            BugReport {
                text: "x".repeat(MAX_TEXT_BYTES + 1),
                ..report()
            }
            .check(),
            Err(BugReportError::TooLarge)
        );
        assert_eq!(
            BugReport {
                title: "x".repeat(MAX_TITLE_CHARS + 1),
                ..report()
            }
            .check(),
            Err(BugReportError::TooLarge)
        );
        assert_eq!(
            BugReport {
                title: " ".to_string(),
                ..report()
            }
            .check(),
            Err(BugReportError::BadTitle)
        );
        assert_eq!(
            BugReport {
                title: "a\nb".to_string(),
                ..report()
            }
            .check(),
            Err(BugReportError::BadTitle)
        );
        assert_eq!(
            BugReport {
                os: String::new(),
                ..report()
            }
            .check(),
            Err(BugReportError::BadField)
        );
    }
}
