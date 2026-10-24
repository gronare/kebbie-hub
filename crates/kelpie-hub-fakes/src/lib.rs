pub mod browser;
pub mod client;
pub mod machine;
pub mod passkey;
pub mod phone;
pub mod push;

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use kelpie_hub_api::{Bytes32, PairingFailure, PairingState};
use kelpie_hub_protocol::SecretKeys;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use client::{HubClient, HubError};

#[derive(Clone, Serialize, Deserialize)]
pub struct KeyFile {
    signing: Bytes32,
    encryption: Bytes32,
}

impl KeyFile {
    pub fn generate() -> KeyFile {
        KeyFile::from_keys(&SecretKeys::generate())
    }

    pub fn from_keys(keys: &SecretKeys) -> KeyFile {
        let (signing, encryption) = keys.to_bytes();
        KeyFile {
            signing: Bytes32(signing),
            encryption: Bytes32(encryption),
        }
    }

    pub fn keys(&self) -> SecretKeys {
        SecretKeys::from_bytes(self.signing.0, self.encryption.0)
    }
}

#[derive(Debug)]
pub enum Error {
    Hub(HubError),
    File(String),
    Protocol(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Hub(error) => error.fmt(formatter),
            Error::File(message) | Error::Protocol(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<HubError> for Error {
    fn from(error: HubError) -> Error {
        Error::Hub(error)
    }
}

#[derive(Debug)]
pub enum PairError {
    Hub(HubError),
    BadCode,
    Mismatch,
    Declined,
    Timeout,
    Ended {
        state: PairingState,
        reason: Option<PairingFailure>,
    },
    Incomplete(&'static str),
    WrongMachine,
}

impl fmt::Display for PairError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairError::Hub(error) => error.fmt(formatter),
            PairError::BadCode => formatter.write_str("that is not a pairing code"),
            PairError::Mismatch => formatter.write_str(
                "pairing refused: the device revealed keys it did not commit to; start again",
            ),
            PairError::Declined => formatter.write_str("pairing refused: the digits did not match"),
            PairError::Timeout => formatter.write_str("pairing timed out; start again"),
            PairError::Ended { state, reason } => match reason {
                Some(reason) => write!(formatter, "pairing {}: {}", state.name(), reason.name()),
                None => write!(formatter, "pairing {}", state.name()),
            },
            PairError::Incomplete(what) => write!(formatter, "the hub left out {what}"),
            PairError::WrongMachine => formatter.write_str(
                "the hub offered keys of another machine than the one that opened the link; not trusted",
            ),
        }
    }
}

impl std::error::Error for PairError {}

impl From<HubError> for PairError {
    fn from(error: HubError) -> PairError {
        PairError::Hub(error)
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T, Error> {
    let text = fs::read_to_string(path)
        .map_err(|error| Error::File(format!("{}: {error}", path.display())))?;
    serde_json::from_str(&text).map_err(|error| Error::File(format!("{}: {error}", path.display())))
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let fail = |error: std::io::Error| Error::File(format!("{}: {error}", path.display()));
    let text = serde_json::to_string_pretty(value).expect("state always serializes");
    let staging = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&staging).map_err(fail)?;
    file.write_all(text.as_bytes()).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    fs::rename(&staging, path).map_err(fail)
}
