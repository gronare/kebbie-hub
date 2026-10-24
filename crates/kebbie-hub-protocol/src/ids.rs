use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefix {
    Machine,
    Device,
    Command,
    Notice,
    Board,
    Alert,
    Blob,
}

impl Prefix {
    const ALL: [Prefix; 7] = [
        Prefix::Machine,
        Prefix::Device,
        Prefix::Command,
        Prefix::Notice,
        Prefix::Board,
        Prefix::Alert,
        Prefix::Blob,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Prefix::Machine => "mch",
            Prefix::Device => "dev",
            Prefix::Command => "cmd",
            Prefix::Notice => "ntc",
            Prefix::Board => "brd",
            Prefix::Alert => "alr",
            Prefix::Blob => "blb",
        }
    }

    fn accepts(self, body: &str) -> bool {
        match self {
            Prefix::Command => is_uuid_v7(body),
            Prefix::Blob => {
                body.len() == 64
                    && body
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            }
            _ => is_ulid(body),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdError(pub String);

impl fmt::Display for IdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "not a hub id: {:?}", self.0)
    }
}

impl std::error::Error for IdError {}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl Id {
    pub fn parse(text: &str) -> Result<Id, IdError> {
        let valid = text.split_once('_').is_some_and(|(tag, body)| {
            Prefix::ALL
                .iter()
                .any(|prefix| prefix.tag() == tag && prefix.accepts(body))
        });
        if valid {
            Ok(Id(text.to_string()))
        } else {
            Err(IdError(text.to_string()))
        }
    }

    pub fn prefix(&self) -> Prefix {
        let tag = self
            .0
            .split_once('_')
            .map(|(tag, _)| tag)
            .unwrap_or_default();
        Prefix::ALL
            .into_iter()
            .find(|prefix| prefix.tag() == tag)
            .expect("an Id is only built by parse")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn ulid(prefix: Prefix, millis: u64, random: [u8; 10]) -> Result<Id, IdError> {
        let mut value = u128::from(millis & 0xFFFF_FFFF_FFFF) << 80;
        for (index, byte) in random.iter().enumerate() {
            value |= u128::from(*byte) << (8 * (9 - index));
        }
        let body: String = (0..26)
            .rev()
            .map(|digit| CROCKFORD[((value >> (5 * digit)) & 31) as usize] as char)
            .collect();
        Id::parse(&format!("{}_{body}", prefix.tag()))
    }

    pub fn command(millis: u64, random: [u8; 10]) -> Id {
        let mut bytes = [0u8; 16];
        bytes[..6].copy_from_slice(&(millis & 0xFFFF_FFFF_FFFF).to_be_bytes()[2..]);
        bytes[6..].copy_from_slice(&random);
        bytes[6] = 0x70 | (bytes[6] & 0x0F);
        bytes[8] = 0x80 | (bytes[8] & 0x3F);
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Id(format!(
            "cmd_{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        ))
    }

    pub fn blob(ciphertext: &[u8]) -> Id {
        let hex: String = Sha256::digest(ciphertext)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Id(format!("blb_{hex}"))
    }

    pub fn new(prefix: Prefix) -> Result<Id, IdError> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or_default();
        let mut random = [0u8; 10];
        getrandom::fill(&mut random).expect("the system random source failed");
        match prefix {
            Prefix::Command => Ok(Id::command(millis, random)),
            _ => Id::ulid(prefix, millis, random),
        }
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for Id {
    type Error = IdError;

    fn try_from(text: String) -> Result<Id, IdError> {
        Id::parse(&text)
    }
}

impl From<Id> for String {
    fn from(id: Id) -> String {
        id.0
    }
}

fn is_ulid(body: &str) -> bool {
    body.len() == 26
        && body.as_bytes()[0] <= b'7'
        && body.bytes().all(|byte| CROCKFORD.contains(&byte))
}

fn is_uuid_v7(body: &str) -> bool {
    let bytes = body.as_bytes();
    let shaped = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        });
    shaped && bytes[14] == b'7' && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_time_part_follows_the_spec_example() {
        let id = Id::ulid(Prefix::Machine, 1_469_918_176_385, [0; 10]).unwrap();
        assert_eq!(&id.as_str()[4..14], "01ARYZ6S41");
        assert_eq!(id.prefix(), Prefix::Machine);
    }

    #[test]
    fn command_ids_are_uuid_v7() {
        let id = Id::command(0x0192_f0c4_7a1e, [0xFF; 10]);
        assert_eq!(id.as_str(), "cmd_0192f0c4-7a1e-7fff-bfff-ffffffffffff");
        assert_eq!(Id::parse(id.as_str()), Ok(id));
    }

    #[test]
    fn parse_refuses_wrong_bodies() {
        for text in [
            "dev_01JB8Q6Z3V9K2M4N5P6R7S8T9U",
            "mch_81JB8Q2W5X7Y8Z9A0B1C2D3E4F",
            "cmd_01JB8Q2W5X7Y8Z9A0B1C2D3E4F",
            "xyz_01JB8Q2W5X7Y8Z9A0B1C2D3E4F",
            "blb_ABC",
        ] {
            assert!(Id::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn ulid_refuses_prefixes_with_other_bodies() {
        assert!(Id::ulid(Prefix::Command, 0, [0; 10]).is_err());
    }
}
