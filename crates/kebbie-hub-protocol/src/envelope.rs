use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::device::DeviceCert;
use crate::ids::{Id, Prefix};
use crate::message::is_collapse;
use crate::{PROTOCOL_VERSION, SCHEMA};

const HEADER_DOMAIN: &[u8] = b"kebbie-hub envelope\0";
pub(crate) const MAX_COLLAPSE_CHARS: usize = 64;
const KEY_COMMIT_CHARS: usize = 43;
/// How long an answer to kebbie's own question, a message to the lead or away mode may wait on the hub for a machine that sleeps.
pub const KEPT_ANSWER_TTL: u64 = 7 * 86_400;

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
    PromptAnswer,
    Idea,
    ItemAnswer,
    StartItem,
    ItemEdit,
    DeviceDecision,
    ReviewOpen,
    ReviewComments,
    ReviewRead,
    AcceptStep,
    AcceptSubtask,
    FoundStart,
    FoundAdd,
    FoundDismiss,
    FoundKeep,
    ReviewSend,
    LeadMessage,
    Away,
}

impl Kind {
    pub const ALL: [Kind; 25] = [
        Kind::Board,
        Kind::Notice,
        Kind::Result,
        Kind::Answer,
        Kind::Push,
        Kind::Start,
        Kind::Routine,
        Kind::PromptAnswer,
        Kind::Idea,
        Kind::ItemAnswer,
        Kind::StartItem,
        Kind::ItemEdit,
        Kind::DeviceDecision,
        Kind::ReviewOpen,
        Kind::ReviewComments,
        Kind::ReviewRead,
        Kind::AcceptStep,
        Kind::AcceptSubtask,
        Kind::FoundStart,
        Kind::FoundAdd,
        Kind::FoundDismiss,
        Kind::FoundKeep,
        Kind::ReviewSend,
        Kind::LeadMessage,
        Kind::Away,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Board => "board",
            Kind::Notice => "notice",
            Kind::Result => "result",
            Kind::Answer => "answer",
            Kind::Push => "push",
            Kind::Start => "start",
            Kind::Routine => "routine",
            Kind::PromptAnswer => "prompt-answer",
            Kind::Idea => "idea",
            Kind::ItemAnswer => "item-answer",
            Kind::StartItem => "start-item",
            Kind::ItemEdit => "item-edit",
            Kind::DeviceDecision => "device-decision",
            Kind::ReviewOpen => "review-open",
            Kind::ReviewComments => "review-comments",
            Kind::ReviewRead => "review-read",
            Kind::AcceptStep => "accept-step",
            Kind::AcceptSubtask => "accept-subtask",
            Kind::FoundStart => "found-start",
            Kind::FoundAdd => "found-add",
            Kind::FoundDismiss => "found-dismiss",
            Kind::FoundKeep => "found-keep",
            Kind::ReviewSend => "review-send",
            Kind::LeadMessage => "lead-message",
            Kind::Away => "away",
        }
    }

    pub fn is_command(self) -> bool {
        matches!(
            self,
            Kind::Answer
                | Kind::Push
                | Kind::Start
                | Kind::Routine
                | Kind::PromptAnswer
                | Kind::Idea
                | Kind::ItemAnswer
                | Kind::StartItem
                | Kind::ItemEdit
                | Kind::DeviceDecision
                | Kind::ReviewOpen
                | Kind::ReviewComments
                | Kind::ReviewRead
                | Kind::AcceptStep
                | Kind::AcceptSubtask
                | Kind::FoundStart
                | Kind::FoundAdd
                | Kind::FoundDismiss
                | Kind::FoundKeep
                | Kind::ReviewSend
                | Kind::LeadMessage
                | Kind::Away
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
            Kind::Answer | Kind::Push | Kind::Idea | Kind::ItemAnswer | Kind::ItemEdit | Kind::DeviceDecision => 3_600,
            Kind::ReviewOpen | Kind::ReviewComments | Kind::ReviewRead | Kind::AcceptStep | Kind::AcceptSubtask => 3_600,
            Kind::FoundAdd | Kind::FoundDismiss | Kind::FoundKeep | Kind::ReviewSend => 3_600,
            Kind::Start | Kind::Routine | Kind::PromptAnswer => 600,
            Kind::StartItem | Kind::FoundStart => 86_400,
            Kind::LeadMessage | Kind::Away => KEPT_ANSWER_TTL,
        }
    }

    pub fn max_ttl(self) -> u64 {
        match self {
            Kind::Board => 86_400,
            Kind::Notice => 86_400,
            Kind::Result => 7 * 86_400,
            Kind::Answer | Kind::LeadMessage | Kind::Away => KEPT_ANSWER_TTL,
            Kind::Push | Kind::Start | Kind::Routine | Kind::StartItem | Kind::FoundStart => 86_400,
            Kind::Idea | Kind::ItemAnswer | Kind::ItemEdit | Kind::DeviceDecision => 3_600,
            Kind::ReviewOpen | Kind::ReviewComments | Kind::ReviewRead | Kind::AcceptStep | Kind::AcceptSubtask => 3_600,
            Kind::FoundAdd | Kind::FoundDismiss | Kind::FoundKeep | Kind::ReviewSend => 3_600,
            Kind::PromptAnswer => 600,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert: Option<DeviceCert>,
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
            cert: None,
        }
    }

    pub fn command(id: Id, kind: Kind, cert: DeviceCert, machine: Id, at: u64) -> Header {
        Header {
            cert: Some(cert.clone()),
            ..Header::new(id, kind, cert.device, machine, at)
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
        match &self.cert {
            Some(cert) => {
                out.push(1);
                cert.put_body(&mut out);
                out.extend(cert.sig);
            }
            None => out.push(0),
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
        match (self.kind.is_command(), &self.cert) {
            (true, Some(cert)) => {
                cert.check()
                    .map_err(|_| ShapeError::Malformed("the device certificate is malformed"))?;
                if self.from != cert.device {
                    return Err(ShapeError::Malformed(
                        "the sender is not the certified device",
                    ));
                }
            }
            (true, None) => {
                return Err(ShapeError::Malformed(
                    "a command carries its device certificate",
                ));
            }
            (false, Some(_)) => {
                return Err(ShapeError::Malformed(
                    "only a command carries a device certificate",
                ));
            }
            (false, None) if self.from != self.machine => {
                return Err(ShapeError::Malformed("the sender cannot send this kind"));
            }
            (false, None) => {}
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
        if self.kind == Kind::Notice && self.collapse.is_none() {
            return Err(ShapeError::Malformed("a notice carries its collapse tag"));
        }
        if !self.collapse.as_deref().is_none_or(is_collapse) {
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
