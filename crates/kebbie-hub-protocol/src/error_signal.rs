use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SIGNAL_DOMAIN: &[u8] = b"kelpie-signal-v1";
pub const SIGNAL_SIGN_DOMAIN: &[u8] = b"kelpie-signal-sign-v1";
pub const MAX_NAME_CHARS: usize = 48;
pub const MAX_COMMIT_CHARS: usize = 40;
pub const MAX_SIGNAL_ROWS: usize = 64;
pub const MAX_SIGNAL_COUNT: u32 = 10_000;

// The only text a signal carries: lowercase ASCII letters and digits, with . _ + - between them, so no sentence, path or message fits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Name(String);

impl Name {
    pub fn new(text: &str) -> Option<Name> {
        let first = text.chars().next()?;
        let last = text.chars().next_back()?;
        let inner = |character: char| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '+' | '-')
        };
        let end = |character: char| character.is_ascii_lowercase() || character.is_ascii_digit();
        (text.len() <= MAX_NAME_CHARS && end(first) && end(last) && text.chars().all(inner))
            .then(|| Name(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Name {
    type Error = String;

    fn try_from(text: String) -> Result<Name, String> {
        Name::new(&text).ok_or_else(|| "not a signal name".to_string())
    }
}

impl From<Name> for String {
    fn from(name: Name) -> String {
        name.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignalKind {
    Panic,
    InternalError,
    CorruptState,
}

impl SignalKind {
    pub const ALL: [SignalKind; 3] = [
        SignalKind::Panic,
        SignalKind::InternalError,
        SignalKind::CorruptState,
    ];

    pub fn key(self) -> &'static str {
        match self {
            SignalKind::Panic => "panic",
            SignalKind::InternalError => "internal-error",
            SignalKind::CorruptState => "corrupt-state",
        }
    }

    pub fn parse(key: &str) -> Option<SignalKind> {
        SignalKind::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalCount {
    pub component: Name,
    pub kind: SignalKind,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorSignals {
    pub version: Name,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<Name>,
    pub os: Name,
    pub arch: Name,
    pub counts: Vec<SignalCount>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalsError {
    Empty,
    TooMany,
    BadCount,
    Repeated,
    BadCommit,
}

impl SignalsError {
    pub fn key(self) -> &'static str {
        match self {
            SignalsError::Empty => "no-signals",
            SignalsError::TooMany => "too-many-signals",
            SignalsError::BadCount => "bad-count",
            SignalsError::Repeated => "repeated-signal",
            SignalsError::BadCommit => "bad-commit",
        }
    }
}

impl fmt::Display for SignalsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

impl std::error::Error for SignalsError {}

fn field(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend((text.len() as u32).to_be_bytes());
    bytes.extend(text.as_bytes());
}

impl ErrorSignals {
    pub fn check(&self) -> Result<(), SignalsError> {
        if self.counts.is_empty() {
            return Err(SignalsError::Empty);
        }
        if self.counts.len() > MAX_SIGNAL_ROWS {
            return Err(SignalsError::TooMany);
        }
        if self
            .counts
            .iter()
            .any(|row| row.count == 0 || row.count > MAX_SIGNAL_COUNT)
        {
            return Err(SignalsError::BadCount);
        }
        let mut seen = HashSet::new();
        if !self
            .counts
            .iter()
            .all(|row| seen.insert((&row.component, row.kind)))
        {
            return Err(SignalsError::Repeated);
        }
        if self.commit.as_ref().is_some_and(|commit| {
            commit.as_str().len() > MAX_COMMIT_CHARS
                || !commit
                    .as_str()
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
        }) {
            return Err(SignalsError::BadCommit);
        }
        Ok(())
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = SIGNAL_DOMAIN.to_vec();
        field(&mut bytes, self.version.as_str());
        field(&mut bytes, self.commit.as_ref().map_or("", Name::as_str));
        field(&mut bytes, self.os.as_str());
        field(&mut bytes, self.arch.as_str());
        bytes.extend((self.counts.len() as u32).to_be_bytes());
        for row in &self.counts {
            field(&mut bytes, row.component.as_str());
            field(&mut bytes, row.kind.key());
            bytes.extend(row.count.to_be_bytes());
        }
        bytes
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.bytes()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bug_report::{InstallKey, verify_signals};

    fn signals() -> ErrorSignals {
        ErrorSignals {
            version: Name::new("0.26.3").unwrap(),
            commit: Name::new("16983b9"),
            os: Name::new("macos").unwrap(),
            arch: Name::new("aarch64").unwrap(),
            counts: vec![SignalCount {
                component: Name::new("keeper.push").unwrap(),
                kind: SignalKind::InternalError,
                count: 3,
            }],
        }
    }

    fn strings(count: usize) -> Vec<String> {
        let alphabet: Vec<char> = "abcz09._+-/ ~:@\\\"'\n\tABZ{}éå€😀\u{0}\u{1b}=,;<>|$%?#*&()[]!"
            .chars()
            .collect();
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..count)
            .map(|_| {
                let length = (next() % 70) as usize;
                (0..length)
                    .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                    .collect()
            })
            .collect()
    }

    fn name_shaped(text: &str) -> bool {
        let bytes = text.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= MAX_NAME_CHARS
            && bytes[0].is_ascii_alphanumeric()
            && !bytes[0].is_ascii_uppercase()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && !bytes[bytes.len() - 1].is_ascii_uppercase()
            && bytes.iter().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._+-".contains(byte)
            })
    }

    #[test]
    fn no_string_with_a_space_path_quote_capital_or_control_becomes_a_name() {
        for text in strings(20_000).into_iter().chain(
            [
                "push failed: git said no",
                "/Users/carl/work",
                "~/.ssh",
                "carl@example.com",
                "Board.Draw",
                ".hidden",
                "trailing-",
                "a\nb",
                &"a".repeat(MAX_NAME_CHARS + 1),
            ]
            .map(str::to_string),
        ) {
            assert_eq!(Name::new(&text).is_some(), name_shaped(&text), "{text:?}");
            let json = serde_json::to_string(&text).unwrap();
            assert_eq!(
                serde_json::from_str::<Name>(&json).is_ok(),
                name_shaped(&text),
                "{text:?} on the wire"
            );
        }
    }

    #[test]
    fn a_payload_takes_no_field_beyond_its_own_and_no_free_text_in_any() {
        let good = serde_json::to_value(signals()).unwrap();
        assert_eq!(
            serde_json::from_value::<ErrorSignals>(good.clone()).unwrap(),
            signals()
        );
        let mut extra = good.clone();
        extra["message"] = "push failed in ~/work/app".into();
        assert!(serde_json::from_value::<ErrorSignals>(extra).is_err());
        let mut row_extra = good.clone();
        row_extra["counts"][0]["detail"] = "x".into();
        assert!(serde_json::from_value::<ErrorSignals>(row_extra).is_err());
        let mut texted = good.clone();
        texted["counts"][0]["component"] = "keeper.push: git refused /Users/carl/app".into();
        assert!(serde_json::from_value::<ErrorSignals>(texted).is_err());
        let mut kind = good.clone();
        kind["counts"][0]["kind"] = "something-else".into();
        assert!(serde_json::from_value::<ErrorSignals>(kind).is_err());
        let mut os = good;
        os["os"] = "macOS 15.1 (carl's laptop)".into();
        assert!(serde_json::from_value::<ErrorSignals>(os).is_err());
    }

    #[test]
    fn a_payload_is_checked_for_rows_counts_repeats_and_its_commit() {
        assert_eq!(signals().check(), Ok(()));
        let with = |change: fn(&mut ErrorSignals)| {
            let mut changed = signals();
            change(&mut changed);
            changed.check()
        };
        assert_eq!(
            with(|signals| signals.counts.clear()),
            Err(SignalsError::Empty)
        );
        assert_eq!(
            with(|signals| signals.counts[0].count = 0),
            Err(SignalsError::BadCount)
        );
        assert_eq!(
            with(|signals| signals.counts[0].count = MAX_SIGNAL_COUNT + 1),
            Err(SignalsError::BadCount)
        );
        assert_eq!(
            with(|signals| signals.counts.push(signals.counts[0].clone())),
            Err(SignalsError::Repeated)
        );
        assert_eq!(
            with(|signals| signals.commit = Name::new("not-hex")),
            Err(SignalsError::BadCommit)
        );
        assert_eq!(
            with(|signals| {
                signals.counts = (0..=MAX_SIGNAL_ROWS)
                    .map(|index| SignalCount {
                        component: Name::new(&format!("c{index}")).unwrap(),
                        kind: SignalKind::Panic,
                        count: 1,
                    })
                    .collect()
            }),
            Err(SignalsError::TooMany)
        );
    }

    #[test]
    fn a_signal_signature_covers_every_field_and_is_no_report_signature() {
        let key = InstallKey::from_bytes([5u8; 32]);
        let nonce = [2u8; 32];
        let signature = key.sign_signals(&nonce, &signals());
        assert!(verify_signals(
            &key.public(),
            &nonce,
            &signals(),
            &signature
        ));
        for change in [
            |signals: &mut ErrorSignals| signals.counts[0].count += 1,
            |signals: &mut ErrorSignals| signals.counts[0].kind = SignalKind::Panic,
            |signals: &mut ErrorSignals| signals.version = Name::new("0.26.4").unwrap(),
            |signals: &mut ErrorSignals| signals.commit = None,
            |signals: &mut ErrorSignals| signals.arch = Name::new("x86_64").unwrap(),
        ] {
            let mut changed = signals();
            change(&mut changed);
            assert!(!verify_signals(&key.public(), &nonce, &changed, &signature));
        }
        assert!(!verify_signals(
            &key.public(),
            &[1u8; 32],
            &signals(),
            &signature
        ));
        assert_ne!(
            &signals().bytes()[..SIGNAL_DOMAIN.len()],
            crate::REPORT_DOMAIN
        );
    }
}
