use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{Id, Prefix};
use crate::{PROTOCOL_VERSION, SCHEMA};

const HEADER_DOMAIN: &[u8] = b"kelpie-hub envelope\0";
const MAX_COLLAPSE_CHARS: usize = 64;
const KEY_COMMIT_CHARS: usize = 43;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Board,
    Notice,
    Result,
    Answer,
    Push,
    Start,
    Routine,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Board => "board",
            Kind::Notice => "notice",
            Kind::Result => "result",
            Kind::Answer => "answer",
            Kind::Push => "push",
            Kind::Start => "start",
            Kind::Routine => "routine",
        }
    }

    pub fn is_command(self) -> bool {
        matches!(
            self,
            Kind::Answer | Kind::Push | Kind::Start | Kind::Routine
        )
    }

    pub fn id_prefix(self) -> Prefix {
        match self {
            Kind::Board => Prefix::Board,
            Kind::Notice => Prefix::Notice,
            _ => Prefix::Command,
        }
    }

    pub fn default_ttl(self) -> u64 {
        match self {
            Kind::Board => 86_400,
            Kind::Notice => 86_400,
            Kind::Result => 7 * 86_400,
            Kind::Answer | Kind::Push => 3_600,
            Kind::Start | Kind::Routine => 600,
        }
    }

    pub fn max_ttl(self) -> u64 {
        match self {
            Kind::Board => 86_400,
            Kind::Notice => 86_400,
            Kind::Result => 7 * 86_400,
            Kind::Answer | Kind::Push | Kind::Start | Kind::Routine => 86_400,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Urgency {
    High,
    Normal,
}

impl Urgency {
    fn name(self) -> &'static str {
        match self {
            Urgency::High => "high",
            Urgency::Normal => "normal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub v: u16,
    pub id: Id,
    pub kind: Kind,
    pub from: Id,
    pub machine: Id,
    pub at: u64,
    pub exp: u64,
    pub schema: u16,
    pub kc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapse: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeError {
    UnsupportedVersion,
    UnsupportedSchema,
    Malformed(&'static str),
}

impl Header {
    pub fn new(id: Id, kind: Kind, from: Id, machine: Id, at: u64) -> Header {
        Header {
            v: PROTOCOL_VERSION,
            id,
            kind,
            from,
            machine,
            at,
            exp: at + kind.default_ttl(),
            schema: SCHEMA,
            kc: String::new(),
            urgency: None,
            collapse: None,
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut out = HEADER_DOMAIN.to_vec();
        out.extend(self.v.to_be_bytes());
        for text in [
            self.id.as_str(),
            self.kind.name(),
            self.from.as_str(),
            self.machine.as_str(),
        ] {
            put_text(&mut out, text);
        }
        out.extend(self.at.to_be_bytes());
        out.extend(self.exp.to_be_bytes());
        out.extend(self.schema.to_be_bytes());
        put_text(&mut out, &self.kc);
        for optional in [self.urgency.map(Urgency::name), self.collapse.as_deref()] {
            match optional {
                Some(text) => {
                    out.push(1);
                    put_text(&mut out, text);
                }
                None => out.push(0),
            }
        }
        out
    }

    pub fn check_shape(&self) -> Result<(), ShapeError> {
        if self.v != PROTOCOL_VERSION {
            return Err(ShapeError::UnsupportedVersion);
        }
        if self.schema != SCHEMA {
            return Err(ShapeError::UnsupportedSchema);
        }
        if self.id.prefix() != self.kind.id_prefix() {
            return Err(ShapeError::Malformed("the id does not fit the kind"));
        }
        if self.machine.prefix() != Prefix::Machine {
            return Err(ShapeError::Malformed("machine is not a machine id"));
        }
        let sender_fits = if self.kind.is_command() {
            self.from.prefix() == Prefix::Device
        } else {
            self.from == self.machine
        };
        if !sender_fits {
            return Err(ShapeError::Malformed("the sender cannot send this kind"));
        }
        if self.exp <= self.at {
            return Err(ShapeError::Malformed("exp is not after at"));
        }
        if self.exp - self.at > self.kind.max_ttl() {
            return Err(ShapeError::Malformed(
                "exp is further out than the kind allows",
            ));
        }
        if self.kc.len() != KEY_COMMIT_CHARS || !is_base64url(&self.kc) {
            return Err(ShapeError::Malformed("kc is not a base64url SHA-256"));
        }
        if self.kind != Kind::Notice && (self.urgency.is_some() || self.collapse.is_some()) {
            return Err(ShapeError::Malformed(
                "only a notice carries urgency or collapse",
            ));
        }
        let collapse_fits = self.collapse.as_deref().is_none_or(|collapse| {
            (1..=MAX_COLLAPSE_CHARS).contains(&collapse.len()) && is_base64url(collapse)
        });
        if !collapse_fits {
            return Err(ShapeError::Malformed(
                "collapse is not a short base64url tag",
            ));
        }
        Ok(())
    }
}

fn is_base64url(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    out.extend((text.len() as u16).to_be_bytes());
    out.extend(text.as_bytes());
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(flatten)]
    pub header: Header,
    pub keys: BTreeMap<Id, String>,
    pub ct: String,
    pub sig: String,
}

impl Envelope {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("an envelope always serializes")
    }

    pub fn from_json(text: &str) -> Result<Envelope, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn for_recipient(&self, recipient: &Id) -> Option<Envelope> {
        let row = self.keys.get(recipient)?;
        Some(Envelope {
            keys: BTreeMap::from([(recipient.clone(), row.clone())]),
            ..self.clone()
        })
    }
}
