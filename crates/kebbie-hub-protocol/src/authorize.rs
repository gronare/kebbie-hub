use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crypto::{self, OpenError, PublicKeys, SecretKeys};
use crate::envelope::{Envelope, Kind, ShapeError};
use crate::ids::Id;
use crate::message::{CommandResult, Payload, Reason};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Read,
    Control,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: Id,
    pub name: String,
    pub keys: PublicKeys,
    pub role: Role,
}

pub struct Machine<'a> {
    pub id: &'a Id,
    pub keys: &'a SecretKeys,
    pub devices: &'a [TrustedDevice],
    pub accepts: &'a [Kind],
}

#[derive(Debug)]
pub struct Accepted<'a> {
    pub device: &'a TrustedDevice,
    pub payload: Payload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    UnsupportedVersion,
    Malformed,
    NotACommand,
    WrongMachine,
    UnknownDevice,
    BadSignature,
}

impl Dropped {
    pub fn key(self) -> &'static str {
        match self {
            Dropped::UnsupportedVersion => "unsupported-version",
            Dropped::Malformed => "malformed",
            Dropped::NotACommand => "not-a-command",
            Dropped::WrongMachine => "wrong-machine",
            Dropped::UnknownDevice => "unknown-device",
            Dropped::BadSignature => "bad-signature",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Drop(Dropped),
    Answer(Reason),
    Replay(CommandResult),
}

pub fn authorize<'a>(
    machine: &Machine<'a>,
    applied: &AppliedCommands,
    envelope: &Envelope,
    now: u64,
) -> Result<Accepted<'a>, Refusal> {
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
    let device = machine
        .devices
        .iter()
        .find(|device| device.id == header.from)
        .ok_or(Refusal::Drop(Dropped::UnknownDevice))?;
    crypto::verify_signature(envelope, &device.id, &device.keys)
        .map_err(|_| Refusal::Drop(Dropped::BadSignature))?;
    if let Some(result) = applied.result(&device.id, &header.id) {
        return Err(Refusal::Replay(result.clone()));
    }
    crypto::check_time(header, now).map_err(|_| Refusal::Answer(Reason::Expired))?;
    if device.role != Role::Control || !machine.accepts.contains(&header.kind) {
        return Err(Refusal::Answer(Reason::NotAllowed));
    }
    let payload = crypto::decrypt(envelope, machine.id, machine.keys)
        .map_err(|error| Refusal::Answer(undecrypted(error)))?;
    payload
        .validate()
        .map_err(|_| Refusal::Answer(Reason::BadRequest))?;
    Ok(Accepted { device, payload })
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

    pub fn record(&mut self, device: &Id, at: u64, result: CommandResult) {
        self.applied
            .entry(device.clone())
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
    fn applied_commands_replay_per_device_and_forget_after_a_week() {
        let mut applied = AppliedCommands::default();
        let phone = Id::ulid(Prefix::Device, 1, [1; 10]).unwrap();
        let tablet = Id::ulid(Prefix::Device, 1, [2; 10]).unwrap();
        let command = Id::command(1, [7; 10]);
        applied.record(
            &phone,
            1_000,
            CommandResult {
                command: command.clone(),
                outcome: Outcome::Done,
                text: "answered".into(),
            },
        );
        assert_eq!(
            applied
                .result(&phone, &command)
                .map(|result| result.outcome),
            Some(Outcome::Done)
        );
        assert_eq!(applied.result(&tablet, &command), None);
        let stored: AppliedCommands =
            serde_json::from_str(&serde_json::to_string(&applied).unwrap()).unwrap();
        assert_eq!(stored, applied);
        applied.prune(1_000 + AppliedCommands::KEEP_SECONDS - 1);
        assert_eq!(applied.len(), 1);
        applied.prune(1_000 + AppliedCommands::KEEP_SECONDS);
        assert!(applied.is_empty());
    }
}
