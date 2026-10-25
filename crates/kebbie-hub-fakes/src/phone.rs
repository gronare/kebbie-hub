use std::time::Duration;

use kebbie_hub_api::{
    Bytes32, CommandQueued, CommandUpload, CommitRequest, Committed, DeviceName, PairingState,
    PairingView, ResultEntry, Results, RevealRequest, normalize_code,
};
use kebbie_hub_protocol::{
    CommandResult, DevicePairing, Envelope, Header, Id, Notice, Payload, Prefix, PublicKeys,
    SecretKeys, open, seal,
};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::push::PushSecret;
use crate::{Error, HubClient, KeyFile, PairError, now};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedMachine {
    pub id: Id,
    pub keys: PublicKeys,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub at: u64,
    pub machine: Option<Id>,
    pub outcome: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PhoneState {
    pub hub: String,
    pub device: Option<Id>,
    token: Option<String>,
    secret: KeyFile,
    pub machines: Vec<PairedMachine>,
    pub attempts: Vec<Attempt>,
    #[serde(default)]
    pub push: Option<PushSecret>,
}

#[derive(Debug)]
pub struct ReceivedResult {
    pub seq: u64,
    pub machine: Id,
    pub command: Id,
    pub result: Result<CommandResult, String>,
}

impl PhoneState {
    pub fn new(hub: &str) -> PhoneState {
        PhoneState {
            hub: hub.to_string(),
            device: None,
            token: None,
            secret: KeyFile::generate(),
            machines: Vec::new(),
            attempts: Vec::new(),
            push: None,
        }
    }

    pub fn registered(hub: &str, device: Id, token: &str) -> PhoneState {
        PhoneState {
            device: Some(device),
            token: Some(token.to_string()),
            ..PhoneState::new(hub)
        }
    }

    pub fn secret(&self) -> SecretKeys {
        self.secret.keys()
    }

    pub fn public(&self) -> PublicKeys {
        self.secret().public()
    }

    pub fn client(&self) -> HubClient {
        HubClient::new(&self.hub, self.token.as_deref())
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub async fn pair<C>(
        &mut self,
        code: &str,
        timeout: Duration,
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let outcome = self.try_pair(code, timeout, confirm).await;
        self.attempts.push(Attempt {
            at: now(),
            machine: outcome.as_ref().ok().cloned(),
            outcome: match &outcome {
                Ok(_) => "admitted".to_string(),
                Err(error) => error.to_string(),
            },
        });
        outcome
    }

    async fn try_pair<C>(
        &mut self,
        code: &str,
        timeout: Duration,
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let code = normalize_code(code).ok_or(PairError::BadCode)?;
        let deadline = Instant::now() + timeout;
        let pairing = DevicePairing::start(self.public(), &code);
        let committed: Committed = self
            .client()
            .post(
                "/v1/d/pairings",
                &CommitRequest {
                    code,
                    commitment: Bytes32(pairing.commitment()),
                },
            )
            .await?;
        if let Some(token) = committed.token {
            self.token = Some(token);
            self.device = Some(committed.device);
        }
        self.follow(&committed.pairing, &pairing, deadline, confirm)
            .await
    }

    async fn follow<C>(
        &mut self,
        pairing_id: &str,
        pairing: &DevicePairing,
        deadline: Instant,
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let client = self.client();
        let path = format!("/v1/d/pairings/{pairing_id}");
        let replied = next_state(&client, &path, PairingState::Committed, deadline).await?;
        if replied.state != PairingState::Replied {
            return Err(ended(&replied));
        }
        let machine_keys = replied
            .machine_keys
            .ok_or(PairError::Incomplete("the machine keys"))?;
        let machine_nonce = replied
            .machine_nonce
            .ok_or(PairError::Incomplete("the machine nonce"))?;
        let (keys, nonce) = pairing.reveal();
        let _: PairingView = client
            .post(
                &format!("{path}/reveal"),
                &RevealRequest {
                    keys,
                    nonce: Bytes32(nonce),
                },
            )
            .await?;
        if !confirm(pairing.comparison_code(&machine_keys, &machine_nonce.0)).await {
            return Err(PairError::Declined);
        }
        let settled = next_state(&client, &path, PairingState::Revealed, deadline).await?;
        if settled.state != PairingState::Admitted {
            return Err(ended(&settled));
        }
        self.machines.retain(|known| known.id != settled.machine);
        self.machines.push(PairedMachine {
            id: settled.machine.clone(),
            keys: machine_keys,
        });
        Ok(settled.machine)
    }

    pub fn machine_keys(&self, machine: &Id) -> Result<PublicKeys, Error> {
        self.machines
            .iter()
            .find(|paired| &paired.id == machine)
            .map(|paired| paired.keys)
            .ok_or_else(|| Error::Protocol(format!("{machine} is not a paired machine")))
    }

    pub fn only_machine(&self) -> Result<Id, Error> {
        match self.machines.as_slice() {
            [paired] => Ok(paired.id.clone()),
            [] => Err(Error::Protocol("this phone has no paired machine".into())),
            _ => Err(Error::Protocol(
                "this phone paired with several machines; pass --machine".into(),
            )),
        }
    }

    pub fn seal_command(
        &self,
        machine: &Id,
        payload: &Payload,
        at: u64,
        ttl: Option<u64>,
    ) -> Result<Envelope, Error> {
        let device = self
            .device
            .clone()
            .ok_or_else(|| Error::Protocol("this phone has no device id yet".into()))?;
        let mut header = Header::new(
            Id::new(Prefix::Command).expect("a fresh command id is valid"),
            payload.kind(),
            device.clone(),
            machine.clone(),
            at,
        );
        if let Some(ttl) = ttl {
            header.exp = at + ttl;
        }
        seal(
            header,
            payload,
            &self.secret(),
            &[
                (machine.clone(), self.machine_keys(machine)?),
                (device, self.public()),
            ],
        )
        .map_err(|error| Error::Protocol(error.to_string()))
    }

    pub async fn send_command(&self, envelope: &Envelope) -> Result<CommandQueued, Error> {
        Ok(self
            .client()
            .post(
                "/v1/d/commands",
                &CommandUpload {
                    envelope: envelope.clone(),
                },
            )
            .await?)
    }

    pub async fn command(&self, machine: &Id, payload: &Payload) -> Result<Envelope, Error> {
        let envelope = self.seal_command(machine, payload, now(), None)?;
        self.send_command(&envelope).await?;
        Ok(envelope)
    }

    pub async fn results(
        &self,
        after: u64,
        wait: u64,
    ) -> Result<(u64, Vec<ReceivedResult>), Error> {
        let listing: Results = self
            .client()
            .get(
                "/v1/d/results",
                &[("after", after.to_string()), ("wait", wait.to_string())],
            )
            .await?;
        let received = listing
            .results
            .into_iter()
            .map(|entry| ReceivedResult {
                result: self.open_result(&entry),
                seq: entry.seq,
                machine: entry.machine,
                command: entry.command,
            })
            .collect();
        Ok((listing.cursor, received))
    }

    pub async fn await_result(
        &self,
        command: &Id,
        timeout: Duration,
    ) -> Result<Option<CommandResult>, Error> {
        let deadline = Instant::now() + timeout;
        let mut after = 0;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let (cursor, received) = self
                .results(after, remaining.as_secs().clamp(1, 25))
                .await?;
            after = cursor;
            if let Some(found) = received.into_iter().find(|entry| &entry.command == command) {
                return found.result.map(Some).map_err(Error::Protocol);
            }
        }
    }

    fn open_result(&self, entry: &ResultEntry) -> Result<CommandResult, String> {
        let device = self.device.as_ref().ok_or("this phone has no device id")?;
        let keys = self
            .machine_keys(&entry.machine)
            .map_err(|error| error.to_string())?;
        let envelope = &entry.envelope;
        if envelope.header.machine != entry.machine || envelope.header.id != entry.command {
            return Err("the hub filed this result under another command".into());
        }
        let payload = open(
            envelope,
            device,
            &self.secret(),
            &entry.machine,
            &keys,
            now(),
        )
        .map_err(|error| format!("does not open: {error}"))?;
        match payload {
            Payload::Result(result) if result.command == entry.command => Ok(result),
            Payload::Result(_) => Err("the result names another command".into()),
            _ => Err("is not a result".into()),
        }
    }

    pub async fn rename(&self, name: &str) -> Result<String, Error> {
        let renamed: DeviceName = self
            .client()
            .put(
                "/v1/d/name",
                &DeviceName {
                    name: name.to_string(),
                },
            )
            .await?;
        Ok(renamed.name)
    }

    pub async fn subscribe_push(&mut self, endpoint: &str) -> Result<PushSecret, Error> {
        let secret = PushSecret::generate(endpoint);
        self.client()
            .put_empty("/v1/d/push", &secret.subscription())
            .await?;
        self.push = Some(secret.clone());
        Ok(secret)
    }

    pub async fn unsubscribe_push(&mut self) -> Result<(), Error> {
        self.client().delete("/v1/d/push").await?;
        self.push = None;
        Ok(())
    }

    pub fn open_push(&self, body: &[u8]) -> Result<(Envelope, Notice), String> {
        let device = self.device.as_ref().ok_or("this phone has no device id")?;
        let secret = self
            .push
            .as_ref()
            .ok_or("this phone has no push subscription")?;
        let plaintext = secret.decrypt(body)?;
        let envelope = Envelope::from_json(
            std::str::from_utf8(&plaintext).map_err(|_| "the push is not text")?,
        )
        .map_err(|error| format!("the push is not an envelope: {error}"))?;
        let machine = envelope.header.machine.clone();
        let keys = self
            .machine_keys(&machine)
            .map_err(|error| error.to_string())?;
        let payload = open(&envelope, device, &self.secret(), &machine, &keys, now())
            .map_err(|error| format!("does not open: {error}"))?;
        match payload {
            Payload::Notice(notice) => Ok((envelope, notice)),
            _ => Err("is not a notice".into()),
        }
    }
}

async fn next_state(
    client: &HubClient,
    path: &str,
    seen: PairingState,
    deadline: Instant,
) -> Result<PairingView, PairError> {
    client
        .await_pairing(path, seen, deadline)
        .await?
        .ok_or(PairError::Timeout)
}

fn ended(view: &PairingView) -> PairError {
    PairError::Ended {
        state: view.state,
        reason: view.reason,
    }
}
