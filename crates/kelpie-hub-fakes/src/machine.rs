use std::fmt;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kelpie_hub_api::{
    AdmittedDevices, BoardStored, BoardUpload, Bytes32, CONNECT_DENIED, CONNECT_EXPIRED, CONNECT_PENDING,
    CONNECT_SLOW_DOWN, ConnectPoll, ConnectStart, ConnectStarted, Connected, DeviceAdd, Inbox,
    JoinRequest, Joined, MachineName, NoticePosted, NoticeUpload, PairingFailure, PairingOpened,
    PairingRefusal, PairingReply, PairingState, PairingView, ResultStored, ResultUpload,
};
use kelpie_hub_protocol::{
    AppliedCommands, Board, CommandResult, ConnectSecret, Dropped, Envelope, Event, Header, Id,
    Kind, Machine, MachinePairing, Notice, Outcome, Payload, Prefix, PublicKeys, Reason, Refusal,
    Role, SecretKeys, TrustedDevice, Urgency, authorize, seal,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::{Error, HubClient, HubError, KeyFile, PairError, now, save};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmittedDevice {
    pub id: Id,
    pub keys: PublicKeys,
    #[serde(default = "control")]
    pub role: Role,
}

fn control() -> Role {
    Role::Control
}

fn every_command() -> Vec<Kind> {
    vec![Kind::Answer, Kind::Push, Kind::Start, Kind::Routine]
}

impl AdmittedDevice {
    fn trusted(&self) -> TrustedDevice {
        TrustedDevice {
            id: self.id.clone(),
            name: self.id.to_string(),
            keys: self.keys,
            role: self.role,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handling {
    Applied { did: String, result: CommandResult },
    Refused(CommandResult),
    Replayed(CommandResult),
    Dropped(Dropped),
}

impl Handling {
    pub fn result(&self) -> Option<&CommandResult> {
        match self {
            Handling::Applied { result, .. }
            | Handling::Refused(result)
            | Handling::Replayed(result) => Some(result),
            Handling::Dropped(_) => None,
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
    pub devices: Vec<AdmittedDevice>,
    pub last_rev: u64,
    pub inbox_seq: u64,
    #[serde(default)]
    pub board: Option<Board>,
    #[serde(default = "every_command")]
    pub accepts: Vec<Kind>,
    #[serde(default)]
    pub applied: AppliedCommands,
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
            devices: Vec::new(),
            last_rev: 0,
            inbox_seq: 0,
            board: None,
            accepts: every_command(),
            applied: AppliedCommands::default(),
        }
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

    pub async fn pair<C>(
        &mut self,
        timeout: Duration,
        show_code: impl FnOnce(&str),
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let opened: PairingOpened = self.client().post("/v1/m/pairings", &()).await?;
        show_code(&opened.code);
        self.run_pairing(&opened.pairing, &opened.code, timeout, confirm)
            .await
    }

    pub async fn add_device<C>(
        &mut self,
        timeout: Duration,
        show_link: impl FnOnce(&DeviceAdd),
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let added: DeviceAdd = self.client().post("/v1/m/devices/add", &()).await?;
        show_link(&added);
        self.run_pairing(&added.pairing, &added.code, timeout, confirm)
            .await
    }

    pub async fn finish_connect(
        &mut self,
        pairing: &str,
        connect: &ConnectSecret,
        timeout: Duration,
    ) -> Result<Id, PairError> {
        self.run_pairing(
            pairing,
            &connect.pairing_code(),
            timeout,
            async |_: String| true,
        )
        .await
    }

    async fn run_pairing<C>(
        &mut self,
        pairing_id: &str,
        code: &str,
        timeout: Duration,
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let client = self.client();
        let path = format!("/v1/m/pairings/{pairing_id}");
        let deadline = Instant::now() + timeout;
        let committed = self
            .next_state(&client, &path, PairingState::Open, deadline)
            .await?;
        let commitment = committed
            .commitment
            .ok_or(PairError::Incomplete("the commitment"))?;
        let pairing = MachinePairing::answer(self.public(), commitment.0, code);
        let (keys, nonce) = pairing.reply();
        let _: PairingView = client
            .post(
                &format!("{path}/reply"),
                &PairingReply {
                    keys,
                    nonce: Bytes32(nonce),
                },
            )
            .await?;
        let revealed = self
            .next_state(&client, &path, PairingState::Replied, deadline)
            .await?;
        let device = revealed
            .device
            .ok_or(PairError::Incomplete("the device id"))?;
        let device_keys = revealed
            .device_keys
            .ok_or(PairError::Incomplete("the device keys"))?;
        let device_nonce = revealed
            .device_nonce
            .ok_or(PairError::Incomplete("the device nonce"))?;
        let Ok(code) = pairing.comparison_code(&device_keys, &device_nonce.0) else {
            refuse(&client, &path, PairingFailure::Mismatch).await?;
            return Err(PairError::Mismatch);
        };
        if !confirm(code).await {
            refuse(&client, &path, PairingFailure::Declined).await?;
            return Err(PairError::Declined);
        }
        let admitted: PairingView = client.post(&format!("{path}/admit"), &()).await?;
        if admitted.state != PairingState::Admitted {
            return Err(PairError::Ended {
                state: admitted.state,
                reason: admitted.reason,
            });
        }
        self.devices.retain(|known| known.id != device);
        self.devices.push(AdmittedDevice {
            id: device.clone(),
            keys: device_keys,
            role: Role::Control,
        });
        Ok(device)
    }

    pub async fn listed_devices(&self) -> Result<Vec<kelpie_hub_api::AdmittedDevice>, Error> {
        let listed: AdmittedDevices = self.client().get("/v1/m/devices", &[]).await?;
        Ok(listed.devices)
    }

    pub async fn remove_device(&mut self, device: &Id) -> Result<(), Error> {
        self.client()
            .delete(&format!("/v1/m/devices/{device}"))
            .await?;
        self.devices.retain(|known| &known.id != device);
        Ok(())
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

    pub async fn post_board(&mut self, board: Board) -> Result<u64, Error> {
        let rev = self.last_rev + 1;
        self.post_board_at(board, rev).await
    }

    pub async fn post_board_at(&mut self, mut board: Board, rev: u64) -> Result<u64, Error> {
        let envelope = self.seal_board(board.clone(), rev, now())?;
        let stored = self.upload_board(rev, envelope).await?;
        board.rev = stored;
        self.board = Some(board);
        Ok(stored)
    }

    pub fn set_role(&mut self, device: &Id, role: Role) -> bool {
        let known = self.devices.iter_mut().find(|known| &known.id == device);
        known.map(|known| known.role = role).is_some()
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
        let header = &envelope.header;
        let at = now();
        self.applied.prune(at);
        let secret = self.secret();
        let trusted: Vec<TrustedDevice> =
            self.devices.iter().map(AdmittedDevice::trusted).collect();
        let machine = Machine {
            id: &self.machine,
            keys: &secret,
            devices: &trusted,
            accepts: &self.accepts,
        };
        let handling = match authorize(&machine, &self.applied, envelope, at) {
            Ok(accepted) => {
                let (did, result) = self.execute(&header.id, &accepted.payload);
                Handling::Applied { did, result }
            }
            Err(Refusal::Answer(reason)) => Handling::Refused(refusal(&header.id, reason)),
            Err(Refusal::Replay(result)) => Handling::Replayed(result),
            Err(Refusal::Drop(dropped)) => Handling::Dropped(dropped),
        };
        if let Handling::Applied { result, .. } | Handling::Refused(result) = &handling {
            self.applied.record(&header.from, at, result.clone());
            if let Some(path) = state_file {
                save(path, self)?;
            }
        }
        let posted = match handling.result() {
            Some(result) => {
                let device = trusted
                    .iter()
                    .find(|device| device.id == header.from)
                    .ok_or_else(|| Error::Protocol("the device left the trusted list".into()))?;
                Some(self.deliver_result(device, result).await?)
            }
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

    fn execute(&self, command: &Id, payload: &Payload) -> (String, CommandResult) {
        let done = |text: String| CommandResult {
            command: command.clone(),
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
                let head = self.board.as_ref().and_then(|board| {
                    board
                        .items
                        .iter()
                        .find(|item| item.address == push.address)
                        .and_then(|item| item.head.clone())
                });
                if head.as_deref() == Some(push.head.as_str()) {
                    (
                        format!("push {} through {}", push.address, push.head),
                        done(format!("Pushed {} through {}.", push.address, push.head)),
                    )
                } else {
                    (
                        format!("push {} refused: head is not {}", push.address, push.head),
                        CommandResult {
                            command: command.clone(),
                            outcome: Outcome::Refused {
                                reason: Reason::HeadMoved,
                            },
                            text: format!("{} moved past {}.", push.address, push.head),
                        },
                    )
                }
            }
            Payload::Start(start) => (
                format!("start {}@{}: {}", start.name, start.repo, start.prompt),
                done(format!("Started {}@{}.", start.name, start.repo)),
            ),
            Payload::Routine(routine) => (
                format!("run routine {}", routine.id),
                done(format!("Started routine {}.", routine.id)),
            ),
            Payload::Board(_) | Payload::Notice(_) | Payload::Result(_) => (
                "nothing: not a command".to_string(),
                refusal(command, Reason::BadRequest),
            ),
        }
    }

    pub fn seal_result(
        &self,
        device: &TrustedDevice,
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
            &[
                (device.id.clone(), device.keys),
                (self.machine.clone(), self.public()),
            ],
        )
        .map_err(|error| Error::Protocol(error.to_string()))
    }

    async fn deliver_result(
        &self,
        device: &TrustedDevice,
        result: &CommandResult,
    ) -> Result<Posted, Error> {
        match self.post_result(device, result).await {
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
        device: &TrustedDevice,
        result: &CommandResult,
    ) -> Result<ResultStored, Error> {
        let envelope = self.seal_result(device, result)?;
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
        recipients.extend(
            self.devices
                .iter()
                .map(|device| (device.id.clone(), device.keys)),
        );
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

    pub fn collapse_tag(&self, event: Event, address: &str) -> String {
        let event = serde_json::to_value(event).expect("an event serializes");
        let (_, encryption) = self.secret().to_bytes();
        let digest = Sha256::new()
            .chain_update(b"kelpie-hub fake collapse\0")
            .chain_update(encryption)
            .chain_update(format!("{}:{address}", event.as_str().unwrap_or_default()))
            .finalize();
        URL_SAFE_NO_PAD.encode(digest)
    }

    pub fn seal_notice(
        &self,
        notice: &Notice,
        urgency: Urgency,
        collapse: Option<String>,
        at: u64,
    ) -> Result<Envelope, Error> {
        let mut header = Header::new(
            Id::new(Prefix::Notice).expect("a fresh notice id is valid"),
            Kind::Notice,
            self.machine.clone(),
            self.machine.clone(),
            at,
        );
        header.urgency = Some(urgency);
        header.collapse = collapse;
        let recipients: Vec<(Id, PublicKeys)> = self
            .devices
            .iter()
            .map(|device| (device.id.clone(), device.keys))
            .collect();
        seal(
            header,
            &Payload::Notice(notice.clone()),
            &self.secret(),
            &recipients,
        )
        .map_err(|error| Error::Protocol(error.to_string()))
    }

    pub async fn post_notice(&self, envelope: Envelope) -> Result<NoticePosted, Error> {
        Ok(self
            .client()
            .post("/v1/m/notices", &NoticeUpload { envelope })
            .await?)
    }

    pub async fn notice(
        &self,
        notice: &Notice,
        urgency: Urgency,
        collapse: Option<String>,
    ) -> Result<NoticePosted, Error> {
        let envelope = self.seal_notice(notice, urgency, collapse, now())?;
        self.post_notice(envelope).await
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

fn refusal(command: &Id, reason: Reason) -> CommandResult {
    CommandResult {
        command: command.clone(),
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
