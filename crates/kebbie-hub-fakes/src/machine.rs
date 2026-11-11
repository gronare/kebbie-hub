use std::fmt;
use std::path::Path;
use std::time::Duration;

use kebbie_hub_api::{
    BoardStored, BoardUpload, Bytes32, CONNECT_DENIED, CONNECT_EXPIRED, CONNECT_PENDING,
    CONNECT_SLOW_DOWN, ConnectPoll, ConnectStart, ConnectStarted, Connected, Inbox,
    JoinRequest, Joined, MachineName, PairingFailure,
    PairingRefusal, PairingReply, PairingState, PairingView, ResultStored, ResultUpload,
};
use kebbie_hub_protocol::{
    AccountFingerprints, AccountKey, AppliedCommands, Board, CommandResult, ConnectSecret, Dropped, Envelope, Header, Id,
    Kind, Machine, Outcome, Payload, Prefix, PublicKeys, Reason, Refusal,
    DeviceStanding, SecretKeys, Trust, VerifiedDevice, authorize, device_fingerprint, pin_account, seal,
};
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::{Error, HubClient, HubError, KeyFile, PairError, now, save};

fn every_command() -> Vec<Kind> {
    vec![Kind::Answer, Kind::Push, Kind::Start, Kind::Routine, Kind::PromptAnswer]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handling {
    Applied { did: String, result: CommandResult },
    Refused(CommandResult),
    Replayed(CommandResult),
    Waiting(Id),
    Dropped(Dropped),
}

impl Handling {
    pub fn result(&self) -> Option<&CommandResult> {
        match self {
            Handling::Applied { result, .. }
            | Handling::Refused(result)
            | Handling::Replayed(result) => Some(result),
            Handling::Waiting(_) | Handling::Dropped(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Handled {
    pub seq: u64,
    pub command: Id,
    pub from: Id,
    pub kind: Kind,
    pub handling: Handling,
    pub posted: Option<Posted>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posted {
    Stored(u64),
    AlreadyHeld,
    CommandGone,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MachineState {
    pub hub: String,
    pub machine: Id,
    pub name: String,
    token: String,
    secret: KeyFile,
    pub last_rev: u64,
    pub inbox_seq: u64,
    #[serde(default)]
    pub board: Option<Board>,
    #[serde(default = "every_command")]
    pub accepts: Vec<Kind>,
    #[serde(default)]
    pub applied: AppliedCommands,
    #[serde(default)]
    pub account: Option<AccountKey>,
    #[serde(default)]
    pub trust_devices: bool,
    #[serde(default)]
    pub devices: BTreeMap<Id, KnownDevice>,
    #[serde(default)]
    pub held: Vec<Envelope>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnownDevice {
    pub name: String,
    #[serde(with = "signing_key")]
    pub signing: [u8; 32],
    pub standing: DeviceStanding,
}

mod signing_key {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 32], D::Error> {
        URL_SAFE_NO_PAD
            .decode(String::deserialize(deserializer)?)
            .map_err(D::Error::custom)?
            .try_into()
            .map_err(|_| D::Error::custom("expected 32 bytes"))
    }
}

fn trust_of(devices: &BTreeMap<Id, KnownDevice>, device: &VerifiedDevice) -> Trust {
    match devices.get(device.id()) {
        Some(known) if known.signing == *device.signing() => match known.standing {
            DeviceStanding::Approved { .. } => Trust::Approved,
            DeviceStanding::Denied { .. } => Trust::Denied,
            DeviceStanding::Waiting { .. } => Trust::Waiting,
        },
        _ => Trust::Waiting,
    }
}

pub async fn join(hub: &str, code: &str, name: &str) -> Result<MachineState, Error> {
    let secret = KeyFile::generate();
    let joined: Joined = HubClient::new(hub, None)
        .post(
            "/v1/m/join",
            &JoinRequest {
                code: code.to_string(),
                name: name.to_string(),
                keys: secret.keys().public(),
            },
        )
        .await?;
    Ok(MachineState::joined(
        hub,
        name,
        joined.machine,
        joined.token,
        secret,
    ))
}

pub struct PendingConnect {
    pub hub: String,
    pub name: String,
    secret: KeyFile,
    pub connect: ConnectSecret,
    pub started: ConnectStarted,
    pub interval: u64,
}

#[derive(Debug)]
pub enum ConnectError {
    Hub(HubError),
    Denied,
    Expired,
    TimedOut,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::Hub(error) => error.fmt(formatter),
            ConnectError::Denied => {
                formatter.write_str("the connection was cancelled in the browser")
            }
            ConnectError::Expired => formatter.write_str("the code expired; connect again"),
            ConnectError::TimedOut => formatter.write_str("nobody connected the machine in time"),
        }
    }
}

impl std::error::Error for ConnectError {}

pub enum ConnectPolled {
    Pending,
    SlowDown,
    Connected {
        machine: Box<MachineState>,
        pairing: Option<String>,
    },
}

pub async fn connect(hub: &str, name: &str) -> Result<PendingConnect, Error> {
    let secret = KeyFile::generate();
    let keys = secret.keys().public();
    let started: ConnectStarted = HubClient::new(hub, None)
        .post(
            "/v1/m/connect",
            &ConnectStart {
                name: name.to_string(),
                keys,
            },
        )
        .await?;
    Ok(PendingConnect {
        hub: hub.to_string(),
        name: name.to_string(),
        connect: ConnectSecret::new(&keys),
        interval: started.interval,
        started,
        secret,
    })
}

impl PendingConnect {
    pub fn link(&self) -> String {
        self.connect.link(&self.started.verification_uri_complete)
    }

    pub async fn poll(&mut self) -> Result<ConnectPolled, ConnectError> {
        let answer = HubClient::new(&self.hub, None)
            .post::<_, Connected>(
                "/v1/m/connect/token",
                &ConnectPoll {
                    device_code: self.started.device_code.clone(),
                },
            )
            .await;
        match answer {
            Ok(connected) => Ok(ConnectPolled::Connected {
                machine: Box::new(MachineState::joined(
                    &self.hub,
                    &self.name,
                    connected.machine,
                    connected.token,
                    self.secret.clone(),
                )),
                pairing: connected.pairing,
            }),
            Err(HubError::Status { status: 400, code }) => match code.as_str() {
                CONNECT_PENDING => Ok(ConnectPolled::Pending),
                CONNECT_SLOW_DOWN => {
                    self.interval += 5;
                    Ok(ConnectPolled::SlowDown)
                }
                CONNECT_DENIED => Err(ConnectError::Denied),
                CONNECT_EXPIRED => Err(ConnectError::Expired),
                _ => Err(ConnectError::Hub(HubError::Status { status: 400, code })),
            },
            Err(error) => Err(ConnectError::Hub(error)),
        }
    }

    pub async fn wait(
        mut self,
        timeout: Duration,
    ) -> Result<(MachineState, Option<String>, ConnectSecret), ConnectError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let ConnectPolled::Connected { machine, pairing } = self.poll().await? {
                return Ok((*machine, pairing, self.connect));
            }
            let next = Instant::now() + Duration::from_secs(self.interval);
            if next > deadline {
                return Err(ConnectError::TimedOut);
            }
            tokio::time::sleep_until(next).await;
        }
    }
}

impl MachineState {
    fn joined(hub: &str, name: &str, machine: Id, token: String, secret: KeyFile) -> MachineState {
        MachineState {
            hub: hub.to_string(),
            machine,
            name: name.to_string(),
            token,
            secret,
            last_rev: 0,
            inbox_seq: 0,
            board: None,
            accepts: every_command(),
            applied: AppliedCommands::default(),
            account: None,
            trust_devices: false,
            devices: BTreeMap::new(),
            held: Vec::new(),
        }
    }

    pub fn device_named(&self, name_or_id: &str) -> Option<Id> {
        self.devices
            .iter()
            .find(|(id, known)| id.as_str() == name_or_id || known.name == name_or_id)
            .map(|(id, _)| id.clone())
    }

    pub async fn decide_device(&mut self, device: &Id, approve: bool, state_file: Option<&Path>) -> Result<Vec<Handled>, Error> {
        let at = now();
        let Some(known) = self.devices.get_mut(device) else {
            return Ok(Vec::new());
        };
        let fingerprint = device_fingerprint(&known.signing);
        known.standing = if approve {
            DeviceStanding::Approved { fingerprint, at }
        } else {
            DeviceStanding::Denied { fingerprint, at }
        };
        let held = std::mem::take(&mut self.held);
        let mut handled = Vec::new();
        for envelope in &held {
            handled.push(self.handle(0, envelope, state_file).await?);
        }
        if let Some(path) = state_file {
            save(path, self)?;
        }
        Ok(handled)
    }

    pub fn standings(&self) -> Option<BTreeMap<Id, DeviceStanding>> {
        self.trust_devices.then(|| {
            self.devices
                .iter()
                .map(|(id, known)| (id.clone(), known.standing.clone()))
                .collect()
        })
    }

    pub fn secret(&self) -> SecretKeys {
        self.secret.keys()
    }

    pub fn public(&self) -> PublicKeys {
        self.secret().public()
    }

    pub fn client(&self) -> HubClient {
        HubClient::new(&self.hub, Some(&self.token))
    }

    pub async fn finish_connect(
        &mut self,
        pairing_id: &str,
        connect: &ConnectSecret,
        timeout: Duration,
    ) -> Result<AccountFingerprints, PairError> {
        let client = self.client();
        let path = format!("/v1/m/pairings/{pairing_id}");
        let deadline = Instant::now() + timeout;
        let committed = self
            .next_state(&client, &path, PairingState::Open, deadline)
            .await?;
        let commitment = committed
            .commitment
            .ok_or(PairError::Incomplete("the commitment"))?;
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).expect("the system random source failed");
        let _: PairingView = client
            .post(
                &format!("{path}/reply"),
                &PairingReply {
                    keys: self.public(),
                    nonce: Bytes32(nonce),
                },
            )
            .await?;
        let revealed = self
            .next_state(&client, &path, PairingState::Replied, deadline)
            .await?;
        let key = revealed
            .account_key
            .ok_or(PairError::Incomplete("the account key"))?;
        let account_nonce = revealed
            .account_nonce
            .ok_or(PairError::Incomplete("the account nonce"))?;
        let Ok(pinned) = pin_account(&commitment.0, &key, &account_nonce.0, &connect.pairing_code())
        else {
            refuse(&client, &path, PairingFailure::Mismatch).await?;
            return Err(PairError::Mismatch);
        };
        let admitted: PairingView = client.post(&format!("{path}/admit"), &()).await?;
        if admitted.state != PairingState::Admitted {
            return Err(PairError::Ended {
                state: admitted.state,
                reason: admitted.reason,
            });
        }
        self.account = Some(key);
        Ok(pinned)
    }

    async fn next_state(
        &self,
        client: &HubClient,
        path: &str,
        seen: PairingState,
        deadline: Instant,
    ) -> Result<PairingView, PairError> {
        let Some(view) = client.await_pairing(path, seen, deadline).await? else {
            let _ = refuse(client, path, PairingFailure::Timeout).await;
            return Err(PairError::Timeout);
        };
        let expected = match seen {
            PairingState::Open => PairingState::Committed,
            _ => PairingState::Revealed,
        };
        if view.state != expected {
            return Err(PairError::Ended {
                state: view.state,
                reason: view.reason,
            });
        }
        Ok(view)
    }

    pub async fn rename(&self, name: &str) -> Result<String, Error> {
        let renamed: MachineName = self
            .client()
            .put(
                "/v1/m/name",
                &MachineName {
                    name: name.to_string(),
                },
            )
            .await?;
        Ok(renamed.name)
    }

    pub async fn leave(&self) -> Result<(), Error> {
        self.client().post_empty("/v1/m/leave", &()).await?;
        Ok(())
    }

    pub async fn put_blob(&self, id: &Id, ciphertext: Vec<u8>) -> Result<(), Error> {
        self.client()
            .put_bytes(&format!("/v1/m/blobs/{id}"), ciphertext)
            .await?;
        Ok(())
    }

    pub async fn post_board(&mut self, board: Board) -> Result<u64, Error> {
        let rev = self.last_rev + 1;
        self.post_board_at(board, rev).await
    }

    pub async fn post_board_at(&mut self, mut board: Board, rev: u64) -> Result<u64, Error> {
        board.accepts = Some(self.accepts.clone());
        board.devices = self.standings();
        let envelope = self.seal_board(board.clone(), rev, now())?;
        let stored = self.upload_board(rev, envelope).await?;
        board.rev = stored;
        self.board = Some(board);
        Ok(stored)
    }

    pub async fn apply_once(
        &mut self,
        wait: u64,
        state_file: Option<&Path>,
    ) -> Result<Vec<Handled>, Error> {
        let inbox: Inbox = self
            .client()
            .get(
                "/v1/m/inbox",
                &[
                    ("after", self.inbox_seq.to_string()),
                    ("wait", wait.to_string()),
                ],
            )
            .await?;
        let mut handled = Vec::new();
        for message in &inbox.messages {
            if message.envelope.header.kind.is_command() {
                handled.push(
                    self.handle(message.seq, &message.envelope, state_file)
                        .await?,
                );
            }
        }
        self.inbox_seq = inbox.seq;
        if let Some(path) = state_file {
            save(path, self)?;
        }
        Ok(handled)
    }

    async fn handle(
        &mut self,
        seq: u64,
        envelope: &Envelope,
        state_file: Option<&Path>,
    ) -> Result<Handled, Error> {
        self.handle_at(seq, envelope, state_file, now()).await
    }

    pub async fn expire_held(&mut self, state_file: Option<&Path>) -> Result<Vec<Handled>, Error> {
        let held = std::mem::take(&mut self.held);
        let mut handled = Vec::new();
        for envelope in &held {
            handled.push(self.handle_at(0, envelope, state_file, envelope.header.exp).await?);
        }
        if let Some(path) = state_file {
            save(path, self)?;
        }
        Ok(handled)
    }

    async fn handle_at(
        &mut self,
        seq: u64,
        envelope: &Envelope,
        state_file: Option<&Path>,
        at: u64,
    ) -> Result<Handled, Error> {
        let header = &envelope.header;
        self.applied.prune(at);
        let secret = self.secret();
        let Some(account) = self.account.clone() else {
            return Ok(Handled {
                seq,
                command: header.id.clone(),
                from: header.from.clone(),
                kind: header.kind,
                handling: Handling::Dropped(Dropped::WrongAccount),
                posted: None,
            });
        };
        let machine = Machine {
            id: &self.machine,
            keys: &secret,
            account: &account,
            accepts: &self.accepts,
            trust: &|device: &VerifiedDevice, _: u64| {
                if self.trust_devices { trust_of(&self.devices, device) } else { Trust::Approved }
            },
        };
        let handling = match authorize(&machine, &self.applied, envelope, at) {
            Ok(verified) => {
                let (did, result) = self.execute(&header.id, verified.device.id(), &verified.payload);
                Handling::Applied { did, result }
            }
            Err(Refusal::Answer(reason)) => Handling::Refused(refusal(&header.id, &header.from, reason)),
            Err(Refusal::Replay(result)) => Handling::Replayed(result),
            Err(Refusal::Waiting(waiting)) => {
                let device = waiting.device.id().clone();
                let cert = waiting.device.cert();
                self.devices.entry(device.clone()).or_insert_with(|| KnownDevice {
                    name: cert.name.clone(),
                    signing: cert.signing,
                    standing: DeviceStanding::Waiting { fingerprint: cert.fingerprint(), until: None },
                });
                if !self.held.iter().any(|held| held.header.id == header.id) {
                    self.held.push(envelope.clone());
                }
                if let Some(path) = state_file {
                    save(path, self)?;
                }
                Handling::Waiting(device)
            }
            Err(Refusal::Drop(dropped)) => Handling::Dropped(dropped),
        };
        if let Handling::Applied { result, .. } | Handling::Refused(result) = &handling {
            self.applied.record(at, result.clone());
            if let Some(path) = state_file {
                save(path, self)?;
            }
        }
        let posted = match handling.result() {
            Some(result) => Some(self.deliver_result(&account, result).await?),
            None => None,
        };
        Ok(Handled {
            seq,
            command: header.id.clone(),
            from: header.from.clone(),
            kind: header.kind,
            handling,
            posted,
        })
    }

    fn execute(&self, command: &Id, device: &Id, payload: &Payload) -> (String, CommandResult) {
        let done = |text: String| CommandResult {
            command: command.clone(),
            device: device.clone(),
            outcome: Outcome::Done,
            text,
        };
        match payload {
            Payload::Answer(answer) => {
                let choice = match answer.choice {
                    Some(choice) => format!("choice {choice}"),
                    None => format!("comment {:?}", answer.comment),
                };
                (
                    format!(
                        "answer {} question {} {choice}",
                        answer.address, answer.question
                    ),
                    done(format!("Answered {}'s question.", answer.address)),
                )
            }
            Payload::Push(push) => {
                let head = self
                    .board
                    .as_ref()
                    .and_then(|board| {
                        board
                            .items
                            .iter()
                            .find(|item| item.address == push.address)
                            .and_then(|item| item.head.clone())
                    })
                    .unwrap_or_else(|| push.head.clone());
                (
                    format!("push {} at {head} (seen {})", push.address, push.head),
                    done(format!("Pushed {} at {head}.", push.address)),
                )
            }
            Payload::Start(start) => (
                format!("start {}@{}: {}", start.name, start.repo, start.prompt),
                done(format!("Started {}@{}.", start.name, start.repo)),
            ),
            Payload::Routine(routine) => (
                format!("run routine {}", routine.id),
                done(format!("Started routine {}.", routine.id)),
            ),
            Payload::PromptAnswer(answer) => (
                format!(
                    "choose {} in {}'s prompt {}",
                    answer.key, answer.address, answer.prompt
                ),
                done(format!("Chose {} in {}'s prompt.", answer.key, answer.address)),
            ),
            Payload::Board(_) | Payload::Notice(_) | Payload::Result(_) => (
                "nothing: not a command".to_string(),
                refusal(command, device, Reason::BadRequest),
            ),
        }
    }

    pub fn seal_result(
        &self,
        account: &AccountKey,
        result: &CommandResult,
    ) -> Result<Envelope, Error> {
        let header = Header::new(
            result.command.clone(),
            Kind::Result,
            self.machine.clone(),
            self.machine.clone(),
            now(),
        );
        seal(
            header,
            &Payload::Result(result.clone()),
            &self.secret(),
            &[account.recipient(), (self.machine.clone(), self.public())],
        )
        .map_err(|error| Error::Protocol(error.to_string()))
    }

    async fn deliver_result(
        &self,
        account: &AccountKey,
        result: &CommandResult,
    ) -> Result<Posted, Error> {
        match self.post_result(account, result).await {
            Ok(stored) => Ok(Posted::Stored(stored.seq)),
            Err(Error::Hub(HubError::Status { status: 404, code }))
                if code == "unknown-command" =>
            {
                Ok(Posted::CommandGone)
            }
            Err(Error::Hub(HubError::Status { status: 409, code })) if code == "result-differs" => {
                Ok(Posted::AlreadyHeld)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn post_result(
        &self,
        account: &AccountKey,
        result: &CommandResult,
    ) -> Result<ResultStored, Error> {
        let envelope = self.seal_result(account, result)?;
        Ok(self
            .client()
            .post("/v1/m/results", &ResultUpload { envelope })
            .await?)
    }

    pub fn seal_board(&self, mut board: Board, rev: u64, at: u64) -> Result<Envelope, Error> {
        board.rev = rev;
        let header = Header::new(
            Id::new(Prefix::Board).expect("a fresh board id is valid"),
            Kind::Board,
            self.machine.clone(),
            self.machine.clone(),
            at,
        );
        let mut recipients = vec![(self.machine.clone(), self.public())];
        recipients.extend(self.account.as_ref().map(AccountKey::recipient));
        seal(header, &Payload::Board(board), &self.secret(), &recipients)
            .map_err(|error| Error::Protocol(error.to_string()))
    }

    pub async fn upload_board(&mut self, rev: u64, envelope: Envelope) -> Result<u64, Error> {
        let stored: BoardStored = self
            .client()
            .put("/v1/m/board", &BoardUpload { rev, envelope })
            .await?;
        self.last_rev = self.last_rev.max(stored.rev);
        Ok(stored.rev)
    }

    pub async fn poll_once(&mut self, wait: u64) -> Result<Inbox, Error> {
        let inbox: Inbox = self
            .client()
            .get(
                "/v1/m/inbox",
                &[
                    ("after", self.inbox_seq.to_string()),
                    ("wait", wait.to_string()),
                ],
            )
            .await?;
        self.inbox_seq = inbox.seq;
        Ok(inbox)
    }
}

fn refusal(command: &Id, device: &Id, reason: Reason) -> CommandResult {
    CommandResult {
        command: command.clone(),
        device: device.clone(),
        outcome: Outcome::Refused { reason },
        text: format!("refused: {}", reason.key()),
    }
}

async fn refuse(client: &HubClient, path: &str, reason: PairingFailure) -> Result<(), PairError> {
    let _: PairingView = client
        .post(&format!("{path}/refuse"), &PairingRefusal { reason })
        .await?;
    Ok(())
}
