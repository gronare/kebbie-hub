use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::account::AccountKey;
use crate::crypto::{self, OpenError, SecretKeys};
use crate::device::{DeviceTrust, Trust, VerifiedDevice};
use crate::envelope::{Envelope, Kind, ShapeError};
use crate::ids::Id;
use crate::message::{CommandResult, Payload, Reason};

pub struct Machine<'a> {
    pub id: &'a Id,
    pub keys: &'a SecretKeys,
    pub account: &'a AccountKey,
    pub accepts: &'a [Kind],
    pub trust: &'a dyn DeviceTrust,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    UnsupportedVersion,
    Malformed,
    NotACommand,
    WrongMachine,
    WrongAccount,
    BadCertificate,
    BadSignature,
}

impl Dropped {
    pub fn key(self) -> &'static str {
        match self {
            Dropped::UnsupportedVersion => "unsupported-version",
            Dropped::Malformed => "malformed",
            Dropped::NotACommand => "not-a-command",
            Dropped::WrongMachine => "wrong-machine",
            Dropped::WrongAccount => "wrong-account",
            Dropped::BadCertificate => "bad-certificate",
            Dropped::BadSignature => "bad-signature",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Drop(Dropped),
    Answer(Reason),
    Replay(Box<CommandResult>),
    Waiting(Box<Verified>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub device: VerifiedDevice,
    pub payload: Payload,
}

pub fn authorize(
    machine: &Machine<'_>,
    applied: &AppliedCommands,
    envelope: &Envelope,
    now: u64,
) -> Result<Verified, Refusal> {
    let header = &envelope.header;
    match header.check_shape() {
        Err(ShapeError::UnsupportedVersion | ShapeError::UnsupportedSchema) => {
            return Err(Refusal::Drop(Dropped::UnsupportedVersion));
        }
        Err(ShapeError::Malformed(_)) => return Err(Refusal::Drop(Dropped::Malformed)),
        Ok(()) => {}
    }
    if !header.kind.is_command() {
        return Err(Refusal::Drop(Dropped::NotACommand));
    }
    if &header.machine != machine.id {
        return Err(Refusal::Drop(Dropped::WrongMachine));
    }
    let device = crypto::verify_device(envelope, machine.account).map_err(|error| {
        Refusal::Drop(match error {
            OpenError::WrongAccount => Dropped::WrongAccount,
            OpenError::BadCertificate => Dropped::BadCertificate,
            OpenError::BadSignature => Dropped::BadSignature,
            _ => Dropped::Malformed,
        })
    })?;
    if let Some(result) = applied.result(device.id(), &header.id) {
        return Err(Refusal::Replay(Box::new(result.clone())));
    }
    let trust = machine.trust.trust(&device, now);
    if trust == Trust::Denied {
        return Err(Refusal::Answer(Reason::Denied));
    }
    crypto::check_time(header, now).map_err(|_| {
        Refusal::Answer(match trust {
            Trust::Waiting => Reason::NotApproved,
            _ => Reason::Expired,
        })
    })?;
    if !machine.accepts.contains(&header.kind) {
        return Err(Refusal::Answer(Reason::NotAllowed));
    }
    let payload = crypto::decrypt(envelope, machine.id, machine.keys)
        .map_err(|error| Refusal::Answer(undecrypted(error)))?;
    payload
        .validate()
        .map_err(|_| Refusal::Answer(Reason::BadRequest))?;
    if let Payload::DeviceDecision(decision) = &payload {
        if &decision.device == device.id() {
            return Err(Refusal::Answer(Reason::BadRequest));
        }
        if trust == Trust::Waiting {
            return Err(Refusal::Answer(Reason::NotApproved));
        }
    }
    match trust {
        Trust::Waiting => Err(Refusal::Waiting(Box::new(Verified { device, payload }))),
        _ => Ok(Verified { device, payload }),
    }
}

fn undecrypted(error: OpenError) -> Reason {
    match error {
        OpenError::Malformed | OpenError::KindMismatch => Reason::BadRequest,
        _ => Reason::Undecryptable,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedCommands {
    applied: BTreeMap<Id, BTreeMap<Id, Applied>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub at: u64,
    pub result: CommandResult,
}

impl AppliedCommands {
    pub const KEEP_SECONDS: u64 = 7 * 86_400;

    pub fn result(&self, device: &Id, command: &Id) -> Option<&CommandResult> {
        self.applied
            .get(device)?
            .get(command)
            .map(|applied| &applied.result)
    }

    pub fn record(&mut self, at: u64, result: CommandResult) {
        self.applied
            .entry(result.device.clone())
            .or_default()
            .insert(result.command.clone(), Applied { at, result });
    }

    pub fn prune(&mut self, now: u64) {
        for commands in self.applied.values_mut() {
            commands.retain(|_, applied| applied.at + Self::KEEP_SECONDS > now);
        }
        self.applied.retain(|_, commands| !commands.is_empty());
    }

    pub fn len(&self) -> usize {
        self.applied.values().map(BTreeMap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.applied.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Prefix;
    use crate::message::Outcome;

    #[test]
    fn no_command_outlives_the_week_its_result_is_kept_to_refuse_a_replay() {
        assert!(crate::Kind::ALL.iter().filter(|kind| kind.is_command()).all(|kind| kind.max_ttl() <= AppliedCommands::KEEP_SECONDS));
        assert_eq!((crate::Kind::Answer.default_ttl(), crate::Kind::Answer.max_ttl()), (3_600, crate::KEPT_ANSWER_TTL), "an answer to kebbie's own question waits a week for a machine that sleeps");
        assert_eq!(crate::KEPT_ANSWER_TTL, AppliedCommands::KEEP_SECONDS);
    }

    #[test]
    fn applied_commands_replay_per_device_and_forget_after_a_week() {
        let mut applied = AppliedCommands::default();
        let phone = Id::ulid(Prefix::Device, 1, [1; 10]).unwrap();
        let other = Id::ulid(Prefix::Device, 1, [2; 10]).unwrap();
        let command = Id::command(1, [7; 10]);
        applied.record(
            1_000,
            CommandResult {
                command: command.clone(),
                device: phone.clone(),
                outcome: Outcome::Done,
                text: "answered".into(),
                blob: None,
            },
        );
        assert_eq!(
            applied
                .result(&phone, &command)
                .map(|result| result.outcome),
            Some(Outcome::Done)
        );
        assert_eq!(applied.result(&other, &command), None);
        let stored: AppliedCommands =
            serde_json::from_str(&serde_json::to_string(&applied).unwrap()).unwrap();
        assert_eq!(stored, applied);
        applied.prune(1_000 + AppliedCommands::KEEP_SECONDS - 1);
        assert_eq!(applied.len(), 1);
        applied.prune(1_000 + AppliedCommands::KEEP_SECONDS);
        assert!(applied.is_empty());
    }
}
