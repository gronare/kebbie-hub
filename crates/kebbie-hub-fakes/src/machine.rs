use std::fmt;
use std::path::Path;
use std::time::Duration;

use kebbie_hub_api::{
    BoardStored, BoardUpload, Bytes32, CONNECT_DENIED, CONNECT_EXPIRED, CONNECT_PENDING,
    CONNECT_SLOW_DOWN, ConnectPoll, ConnectStart, ConnectStarted, Connected, Inbox,
    JoinRequest, Joined, MachineName, PairingFailure,
    PairingRefusal, PairingReply, PairingState, PairingView, ResultStored, ResultUpload,
    NoticePosted, NoticeUpload, NotifiedDevices,
};
use kebbie_hub_protocol::{
    AccountFingerprints, AccountKey, AppliedCommands, BACKLOG_BODY_MEDIA_TYPE, BacklogBody, BacklogRow, BacklogState, Board, CommandResult, Group, Home, HomeKind, Item, ItemKey, Priority, ConnectSecret, Dropped, Envelope, Header, Id,
    Idea, ItemAnswer, ItemEdit, Kind, Machine, Outcome, Payload, Prefix, StartItem, PublicKeys, Reason, Refusal,
    DeviceStanding, SecretKeys, Trust, VerifiedDevice, authorize, device_fingerprint, pin_account, seal, seal_blob, text_hash,
    DeviceCert, DeviceDecision, MAX_RETRACTS, Notice, Urgency, AcceptStep, AcceptSubtask, BlobRef, REVIEW_MEDIA_TYPE, Review, ReviewComments,
    FoundAdd, FoundDismiss, FoundStart, PushApproval, Pushable, ReviewFile, ReviewOffer, ReviewOpen, ReviewRead, Side, SubtaskState,
};
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::{Error, HubClient, HubError, KeyFile, PairError, now, save};

pub fn every_command() -> Vec<Kind> {
    vec![
        Kind::Answer,
        Kind::Push,
        Kind::Start,
        Kind::Routine,
        Kind::PromptAnswer,
        Kind::Idea,
        Kind::ItemAnswer,
        Kind::StartItem,
        Kind::ItemEdit,
        Kind::ReviewOpen,
        Kind::ReviewComments,
        Kind::ReviewRead,
        Kind::AcceptStep,
        Kind::AcceptSubtask,
        Kind::FoundStart,
        Kind::FoundAdd,
        Kind::FoundDismiss,
    ]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApproveFrom {
    #[default]
    Devices,
    Machine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeSent {
    pub envelope: Option<Envelope>,
    pub recipients: Vec<Id>,
    pub pushes: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Backlog {
    pub rows: Vec<BacklogRow>,
    #[serde(default)]
    pub bodies: BTreeMap<ItemKey, BacklogBody>,
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
    #[serde(default)]
    pub backlog: Backlog,
    #[serde(default)]
    pub started: BTreeMap<ItemKey, String>,
    #[serde(default)]
    pub approve_from: ApproveFrom,
    #[serde(default)]
    pub retracts: Vec<String>,
    #[serde(default)]
    pub reviews: BTreeMap<String, Review>,
    #[serde(skip)]
    review_blobs: Vec<(Id, Vec<u8>)>,
    #[serde(skip)]
    sealed_reviews: BTreeMap<String, (Vec<u8>, BlobRef, Vec<u8>)>,
    #[serde(skip)]
    superseded_blobs: Vec<Id>,
    #[serde(skip)]
    board_changed: bool,
    #[serde(skip)]
    backlog_changed: bool,
    #[serde(skip)]
    decided: bool,
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
            backlog: Backlog::default(),
            started: BTreeMap::new(),
            approve_from: ApproveFrom::Devices,
            retracts: Vec::new(),
            reviews: BTreeMap::new(),
            review_blobs: Vec::new(),
            sealed_reviews: BTreeMap::new(),
            superseded_blobs: Vec::new(),
            board_changed: false,
            backlog_changed: false,
            decided: false,
        }
    }

    pub fn takes(&self) -> Vec<Kind> {
        let mut kinds: Vec<Kind> = self.accepts.iter().copied().filter(|kind| *kind != Kind::DeviceDecision).collect();
        if self.trust_devices && self.approve_from == ApproveFrom::Devices {
            kinds.push(Kind::DeviceDecision);
        }
        kinds
    }

    fn decide(&mut self, decision: &DeviceDecision) -> Result<String, Reason> {
        let known = self.devices.get_mut(&decision.device).ok_or(Reason::UnknownDevice)?;
        let fingerprint = device_fingerprint(&known.signing);
        if fingerprint != decision.fingerprint {
            return Err(Reason::UnknownDevice);
        }
        if !matches!(known.standing, DeviceStanding::Waiting { .. }) {
            return Err(Reason::AlreadySettled);
        }
        let at = now();
        known.standing = if decision.approve {
            DeviceStanding::Approved { fingerprint, at }
        } else {
            DeviceStanding::Denied { fingerprint, at }
        };
        self.decided = true;
        Ok(format!("{} {}.", if decision.approve { "Approved" } else { "Denied" }, known.name))
    }

    pub fn settle(&mut self, collapse: &str) {
        self.retracts.retain(|tag| tag != collapse);
        self.retracts.push(collapse.to_string());
        let over = self.retracts.len().saturating_sub(MAX_RETRACTS);
        self.retracts.drain(..over);
    }

    pub async fn notified_devices(&self) -> Result<NotifiedDevices, Error> {
        Ok(self.client().get("/v1/m/devices", &[] as &[(&str, String)]).await?)
    }

    pub fn notice_recipients(&self, listed: &NotifiedDevices, notice: &Notice) -> Vec<DeviceCert> {
        let Some(account) = &self.account else {
            return Vec::new();
        };
        listed
            .devices
            .iter()
            .filter(|device| account.verifies_device(&device.cert) && device.notifications.as_ref().is_some_and(|wanted| wanted.contains(&notice.category)))
            .filter(|device| {
                !self.trust_devices
                    || self.devices.get(&device.cert.device).is_some_and(|known| {
                        known.signing == device.cert.signing && matches!(known.standing, DeviceStanding::Approved { .. })
                    })
            })
            .map(|device| device.cert.clone())
            .collect()
    }

    pub fn seal_notice(&self, notice: &Notice, collapse: &str, urgency: Urgency, recipients: &[DeviceCert], at: u64) -> Result<Envelope, Error> {
        if !notice.fits_collapse(collapse) {
            return Err(Error::Protocol(format!("the collapse tag {collapse} names another category than {}", notice.category.name())));
        }
        let mut header = Header::new(Id::new(Prefix::Notice).map_err(|error| Error::Protocol(error.to_string()))?, Kind::Notice, self.machine.clone(), self.machine.clone(), at);
        header.urgency = Some(urgency);
        header.collapse = Some(collapse.to_string());
        let recipients: Vec<_> = recipients.iter().map(DeviceCert::recipient).collect();
        seal(header, &Payload::Notice(notice.clone()), &self.secret(), &recipients).map_err(|error| Error::Protocol(error.to_string()))
    }

    pub async fn notify(&mut self, mut notice: Notice, collapse: &str, urgency: Urgency) -> Result<NoticeSent, Error> {
        notice.retract = self.retracts.iter().filter(|tag| *tag != collapse).cloned().collect();
        Payload::Notice(notice.clone()).validate().map_err(|problem| Error::Protocol(problem.to_string()))?;
        let listed = self.notified_devices().await?;
        let recipients = self.notice_recipients(&listed, &notice);
        if recipients.is_empty() {
            return Ok(NoticeSent { envelope: None, recipients: Vec::new(), pushes: 0 });
        }
        let envelope = self.seal_notice(&notice, collapse, urgency, &recipients, now())?;
        let posted: NoticePosted = self.client().post("/v1/m/notices", &NoticeUpload { envelope: envelope.clone() }).await?;
        self.retracts.retain(|tag| !notice.retract.contains(tag));
        Ok(NoticeSent {
            envelope: Some(envelope),
            recipients: recipients.into_iter().map(|cert| cert.device).collect(),
            pushes: posted.pushes,
        })
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

    pub async fn delete_blob(&self, id: &Id) -> Result<(), Error> {
        self.client().delete(&format!("/v1/m/blobs/{id}")).await?;
        Ok(())
    }

    pub async fn post_board(&mut self, board: Board) -> Result<u64, Error> {
        let rev = self.last_rev + 1;
        self.post_board_at(board, rev).await
    }

    pub async fn post_board_at(&mut self, mut board: Board, rev: u64) -> Result<u64, Error> {
        board.accepts = Some(self.takes());
        board.devices = self.standings();
        if board.backlog.is_none() && !self.backlog.rows.is_empty() {
            board.backlog = Some(self.backlog.rows.clone());
        }
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
        if std::mem::take(&mut self.decided) {
            let held = std::mem::take(&mut self.held);
            for envelope in &held {
                handled.push(self.handle(0, envelope, state_file).await?);
            }
            if let Some(board) = self.board.clone() {
                self.post_board(board).await?;
            }
        }
        if self.backlog_changed {
            self.publish_backlog().await?;
        } else if std::mem::take(&mut self.board_changed)
            && let Some(board) = self.board.clone()
        {
            self.post_board(board).await?;
        }
        if let Some(path) = state_file {
            save(path, self)?;
        }
        Ok(handled)
    }

    pub async fn publish_backlog(&mut self) -> Result<u64, Error> {
        let mut rows = std::mem::take(&mut self.backlog.rows);
        for row in &mut rows {
            row.body = None;
            if let Some(body) = self.backlog.bodies.get(&row.item) {
                let plaintext = serde_json::to_vec(body).map_err(|error| Error::Protocol(error.to_string()))?;
                let (blob, ciphertext) = seal_blob(&plaintext, BACKLOG_BODY_MEDIA_TYPE);
                self.client().put_bytes(&format!("/v1/m/blobs/{}", blob.id), ciphertext).await?;
                row.body = Some(blob);
            }
            row.questions = self.backlog.bodies.get(&row.item).map_or(0, |body| body.questions.len() as u32);
            row.blocking = self.backlog.bodies.get(&row.item).map_or(0, waiting_on_you);
        }
        self.backlog.rows = rows;
        let mut board = self.board.clone().unwrap_or_else(empty_board);
        board.backlog = Some(self.backlog.rows.clone());
        self.backlog_changed = false;
        self.post_board(board).await
    }

    pub async fn post_review(&mut self, mut item: Item, review: Review) -> Result<u64, Error> {
        review.check().map_err(|problem| Error::Protocol(problem.to_string()))?;
        item.address = review.address.clone();
        self.reviews.insert(review_key(&review.address, review.subtask.as_ref().map(|subtask| subtask.id.as_str())), review);
        item.review = self.offer(&item);
        let mut board = self.board.clone().unwrap_or_else(empty_board);
        board.items.retain(|held| held.address != item.address);
        board.items.insert(0, item);
        self.post_board(board).await
    }

    fn offer(&self, item: &Item) -> Option<ReviewOffer> {
        let subtask = self.first_waiting(item);
        let review = self.reviews.get(&review_key(&item.address, subtask.as_deref()))?;
        Some(ReviewOffer { subtask, files: review.subtask.as_ref().map_or(&review.files, |subtask| &subtask.files).len() as u32 })
    }

    fn first_waiting(&self, item: &Item) -> Option<String> {
        item.subtasks.iter().flatten().filter(|subtask| subtask.state == SubtaskState::Ready).map(|subtask| subtask.id.clone()).find(|id| self.reviews.contains_key(&review_key(&item.address, Some(id))))
    }

    fn opened_subtask(&self, open: &ReviewOpen) -> Option<String> {
        open.subtask.clone().or_else(|| self.board.as_ref()?.items.iter().find(|item| item.address == open.address).and_then(|item| self.first_waiting(item)))
    }

    fn seal_review(&mut self, open: &ReviewOpen) -> Result<(BlobRef, Option<String>), Reason> {
        let subtask = self.opened_subtask(open);
        let key = review_key(&open.address, subtask.as_deref());
        let review = self.reviews.get(&key).ok_or(Reason::AlreadySettled)?;
        let plaintext = serde_json::to_vec(review).expect("a review always serializes");
        if let Some((held, blob, ciphertext)) = self.sealed_reviews.get(&key)
            && *held == plaintext
        {
            self.review_blobs.push((blob.id.clone(), ciphertext.clone()));
            return Ok((blob.clone(), subtask));
        }
        let (blob, ciphertext) = seal_blob(&plaintext, REVIEW_MEDIA_TYPE);
        self.review_blobs.push((blob.id.clone(), ciphertext.clone()));
        if let Some((_, old, _)) = self.sealed_reviews.insert(key, (plaintext, blob.clone(), ciphertext)) {
            self.superseded_blobs.push(old.id);
        }
        Ok((blob, subtask))
    }

    fn mark_read(&mut self, read: &ReviewRead) -> Result<String, Reason> {
        let prefix = review_key(&read.address, None);
        let held = |key: &String| *key == prefix || key.starts_with(&format!("{prefix}#"));
        if !self.reviews.keys().any(held) {
            return Err(Reason::AlreadySettled);
        }
        let names = |file: &&mut ReviewFile| file.path == read.path && file.content == read.content;
        let (mut in_commits, mut whole) = (Vec::new(), Vec::new());
        for review in self.reviews.iter_mut().filter(|(key, _)| held(key)).map(|(_, review)| review) {
            match review.subtask.as_mut() {
                Some(step) => {
                    in_commits.extend(step.commits.iter_mut().filter(|commit| read.sha.as_ref().is_none_or(|sha| *sha == commit.sha)).flat_map(|commit| commit.files.iter_mut()).filter(names));
                    whole.extend(step.files.iter_mut().filter(names));
                }
                None => whole.extend(review.files.iter_mut().filter(names)),
            }
        }
        if in_commits.is_empty() && (read.sha.is_some() || whole.is_empty()) {
            return Err(Reason::HeadMoved);
        }
        for file in in_commits.into_iter().chain(whole) {
            file.read = read.read;
        }
        Ok(format!("Marked {} {}.", read.path, if read.read { "read" } else { "unread" }))
    }

    fn accept_step(&mut self, accept: &AcceptStep) -> Result<String, Reason> {
        self.accept_subtask(&AcceptSubtask { address: accept.address.clone(), subtask: accept.step.to_string(), head: accept.head.clone() })
    }

    fn accept_subtask(&mut self, accept: &AcceptSubtask) -> Result<String, Reason> {
        let item = self.board.as_mut().and_then(|board| board.items.iter_mut().find(|item| item.address == accept.address)).ok_or(Reason::UnknownRepo)?;
        let subtask = item.subtasks.iter_mut().flatten().find(|subtask| subtask.id == accept.subtask).ok_or(Reason::HeadMoved)?;
        if subtask.head() != Some(accept.head.as_str()) {
            return Err(Reason::HeadMoved);
        }
        if subtask.state == SubtaskState::Accepted {
            return Err(Reason::AlreadySettled);
        }
        subtask.state = SubtaskState::Accepted;
        let title = subtask.title.clone();
        item.pushable = pushable(item);
        self.board_changed = true;
        Ok(format!("Accepted {title} of {}.", accept.address))
    }

    fn push_text(&self, push: &PushApproval) -> (String, String) {
        let item = self.board.as_ref().and_then(|board| board.items.iter().find(|item| item.address == push.address));
        if let Some(run) = item.and_then(|item| item.pushable.as_ref()).filter(|run| run.through == push.head && item.and_then(|item| item.head.as_deref()) != Some(push.head.as_str())) {
            return (format!("push {} through {} at {}", push.address, plural(run.count as usize, "commit"), push.head), format!("Pushed {} of {} at {}.", plural(run.count as usize, "commit"), push.address, push.head));
        }
        let head = item.and_then(|item| item.head.clone()).unwrap_or_else(|| push.head.clone());
        (format!("push {} at {head} (seen {})", push.address, push.head), format!("Pushed {} at {head}.", push.address))
    }

    fn found_row(&mut self, item: &ItemKey) -> Result<BacklogRow, Reason> {
        let index = self.backlog.rows.iter().position(|row| &row.item == item && row.found.is_some()).ok_or(Reason::UnknownItem)?;
        self.backlog_changed = true;
        Ok(self.backlog.rows.remove(index))
    }

    fn start_found(&mut self, start: &FoundStart) -> Result<String, Reason> {
        let row = self.found_row(&start.item)?;
        let address = format!("{}@{}", start.name.as_deref().unwrap_or("found-task"), row.repo.as_deref().unwrap_or("repo"));
        let board = self.board.get_or_insert_with(empty_board);
        board.items.retain(|item| item.address != address);
        board.items.insert(0, started_task(&address, &row.title));
        Ok(format!("Started {address} from what tasks found."))
    }

    fn add_found(&mut self, add: &FoundAdd) -> Result<String, Reason> {
        if !self.board.as_ref().is_some_and(|board| board.items.iter().any(|item| item.address == add.task)) {
            return Err(Reason::UnknownRepo);
        }
        let row = self.found_row(&add.item)?;
        Ok(format!("Sent {:?} to {} as a subtask to add.", row.title, add.task))
    }

    fn dismiss_found(&mut self, dismiss: &FoundDismiss) -> Result<String, Reason> {
        let row = self.found_row(&dismiss.item)?;
        Ok(format!("Dismissed {:?}.", row.title))
    }

    fn file_idea(&mut self, idea: &Idea) -> String {
        let number = self.backlog.rows.len() + self.started.len() + 1;
        let title: String = idea.text.trim().chars().take(60).collect();
        self.backlog.rows.insert(0, BacklogRow {
            item: ItemKey::parse(&format!("kept:idea-{number}")).expect("a numbered kept id is a valid key"),
            title,
            priority: Priority::None,
            repo: idea.repo.clone(),
            state: BacklogState::ToWrite,
            filed: now(),
            home: Home { kind: HomeKind::Kept, label: "kept by kebbie".to_string() },
            questions: 0,
            blocking: 0,
            goal: None,
            body: None,
            found: None,
        });
        self.backlog_changed = true;
        "Filed the idea; the lead writes it out.".to_string()
    }

    fn answer_item(&mut self, answer: &ItemAnswer) -> Result<String, Reason> {
        let body = self.backlog.bodies.get_mut(&answer.item).ok_or(Reason::UnknownItem)?;
        let question = body.questions.get_mut(answer.index as usize).filter(|question| question.text == answer.question).ok_or(Reason::QuestionChanged)?;
        if question.pending_answer.is_some() {
            return Err(Reason::AlreadySettled);
        }
        question.pending_answer = Some(answer.answer.clone());
        self.backlog_changed = true;
        Ok("The lead has your answer.".to_string())
    }

    fn edit_item(&mut self, edit: &ItemEdit) -> Result<String, Reason> {
        let row = self.backlog.rows.iter_mut().find(|row| row.item == edit.item).ok_or(Reason::UnknownItem)?;
        let Some(key) = &edit.section else {
            if text_hash(&row.title) != edit.base {
                return Err(Reason::SectionChanged);
            }
            row.title = edit.text.clone();
            self.backlog_changed = true;
            return Ok("Saved the title.".to_string());
        };
        let body = self.backlog.bodies.get_mut(&edit.item).ok_or(Reason::UnknownItem)?;
        let section = body.sections.iter_mut().find(|section| &section.key == key).ok_or(Reason::SectionChanged)?;
        if text_hash(&section.text) != edit.base {
            return Err(Reason::SectionChanged);
        }
        if section.required && edit.text.trim().is_empty() {
            return Err(Reason::BadRequest);
        }
        section.text = edit.text.clone();
        let heading = section.heading.clone();
        self.backlog_changed = true;
        Ok(format!("Saved {heading}."))
    }

    fn start_item(&mut self, start: &StartItem) -> Result<String, Reason> {
        if self.started.contains_key(&start.item) {
            return Err(Reason::AlreadyStarted);
        }
        let index = self.backlog.rows.iter().position(|row| row.item == start.item).ok_or(Reason::UnknownItem)?;
        let blocking = self.backlog.bodies.get(&start.item).map_or(0, waiting_on_you);
        if blocking > 0 && !start.anyway {
            return Err(Reason::QuestionsOpen { blocking });
        }
        let row = self.backlog.rows.remove(index);
        self.backlog.bodies.remove(&row.item);
        let task: String = row.title.to_lowercase().split(|char: char| !char.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).take(3).collect::<Vec<_>>().join("-");
        let address = format!("{}@{}", if task.is_empty() { "task" } else { &task }, row.repo.as_deref().unwrap_or("repo"));
        self.started.insert(row.item.clone(), address.clone());
        let board = self.board.get_or_insert_with(empty_board);
        board.items.retain(|item| item.address != address);
        board.items.insert(0, started_task(&address, &row.title));
        self.backlog_changed = true;
        Ok(format!("Started {address}: the lead chose the recipe small-fix and queued it."))
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
            accepts: &self.takes(),
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
            Err(Refusal::Replay(result)) => Handling::Replayed(*result),
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
        for (id, ciphertext) in std::mem::take(&mut self.review_blobs) {
            self.put_blob(&id, ciphertext).await?;
        }
        for id in std::mem::take(&mut self.superseded_blobs) {
            self.delete_blob(&id).await?;
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

    fn execute(&mut self, command: &Id, device: &Id, payload: &Payload) -> (String, CommandResult) {
        let done = |text: String| CommandResult {
            command: command.clone(),
            device: device.clone(),
            outcome: Outcome::Done,
            text,
            blob: None,
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
                let (did, text) = self.push_text(push);
                (did, done(text))
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
            Payload::Idea(idea) => (
                format!("file idea in {}: {}", idea.repo.as_deref().unwrap_or("-"), idea.text),
                done(self.file_idea(idea)),
            ),
            Payload::ItemAnswer(answer) => (
                format!("answer {} question {}: {}", answer.item, answer.index, answer.answer),
                self.answer_item(answer).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::StartItem(start) => (
                format!("start {}{}", start.item, if start.anyway { " anyway" } else { "" }),
                self.start_item(start).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::ItemEdit(edit) => (
                format!("edit {} {}:\n{}", edit.item, edit.section.as_deref().unwrap_or("title"), edit.text),
                self.edit_item(edit).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::DeviceDecision(decision) => (
                format!("{} {}", if decision.approve { "approve" } else { "deny" }, decision.device),
                self.decide(decision).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::ReviewOpen(open) => (
                format!("open the review of {}{}", open.address, open.subtask.as_deref().map_or(String::new(), |subtask| format!(" subtask {subtask}"))),
                match self.seal_review(open) {
                    Ok((blob, subtask)) => CommandResult { blob: Some(blob), ..done(format!("The review of {}{} is ready.", subtask.map_or(String::new(), |subtask| format!("subtask {subtask} of ")), open.address)) },
                    Err(reason) => refusal(command, device, reason),
                },
            ),
            Payload::ReviewComments(comments) => (
                format!("type into {}'s Claude:\n{}", comments.address, comment_lines(comments)),
                done(format!("Sent {} to {}'s Claude.", plural(comments.comments.len(), "comment"), comments.address)),
            ),
            Payload::ReviewRead(read) => (
                format!("mark {} {} on {}", read.path, if read.read { "read" } else { "unread" }, read.address),
                self.mark_read(read).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::AcceptStep(accept) => (
                format!("accept step {} at {} on {}", accept.step, accept.head, accept.address),
                self.accept_step(accept).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::AcceptSubtask(accept) => (
                format!("accept subtask {} at {} on {}", accept.subtask, accept.head, accept.address),
                self.accept_subtask(accept).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::FoundStart(start) => (
                format!("start a task from {}{}", start.item, start.name.as_deref().map_or(String::new(), |name| format!(" as {name}"))),
                self.start_found(start).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::FoundAdd(add) => (
                format!("add {} to {}", add.item, add.task),
                self.add_found(add).map_or_else(|reason| refusal(command, device, reason), done),
            ),
            Payload::FoundDismiss(dismiss) => (
                format!("dismiss {}", dismiss.item),
                self.dismiss_found(dismiss).map_or_else(|reason| refusal(command, device, reason), done),
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
    let text = match reason {
        Reason::QuestionsOpen { blocking: 1 } => "Not started: 1 question waits on you.".to_string(),
        Reason::QuestionsOpen { blocking } => format!("Not started: {blocking} questions wait on you."),
        Reason::AlreadyStarted => "Not started: it was started already.".to_string(),
        Reason::UnknownItem => "It is no longer in the backlog.".to_string(),
        Reason::QuestionChanged => "Not answered: the question changed since you saw it.".to_string(),
        Reason::SectionChanged => "Not saved: it changed on the machine since you opened it.".to_string(),
        Reason::UnknownDevice => "No such device waits for approval here.".to_string(),
        Reason::AlreadySettled => "Already settled.".to_string(),
        Reason::HeadMoved => "It changed since you saw it.".to_string(),
        _ => format!("refused: {}", reason.key()),
    };
    CommandResult {
        command: command.clone(),
        device: device.clone(),
        outcome: Outcome::Refused { reason },
        text,
        blob: None,
    }
}

fn plural(count: usize, one: &str) -> String {
    if count == 1 { format!("1 {one}") } else { format!("{count} {one}s") }
}

fn review_key(address: &str, subtask: Option<&str>) -> String {
    subtask.map_or_else(|| address.to_string(), |subtask| format!("{address}#{subtask}"))
}

fn pushable(item: &Item) -> Option<Pushable> {
    let order = item.order.as_deref()?;
    let accepted = |id: &str| item.subtask(id).is_some_and(|subtask| subtask.state == SubtaskState::Accepted);
    let count = order.iter().take_while(|row| accepted(&row.subtask)).count();
    let through = order[..count].last()?.sha.clone();
    let mut subtasks: Vec<String> = Vec::new();
    for row in &order[..count] {
        if !subtasks.contains(&row.subtask) {
            subtasks.push(row.subtask.clone());
        }
    }
    Some(Pushable { count: count as u32, through, subtasks })
}

fn comment_lines(comments: &ReviewComments) -> String {
    let line = |comment: &kebbie_hub_protocol::ReviewComment| {
        let before = if comment.side == Side::Old { " (before)" } else { "" };
        let commit = comment.sha.as_deref().map(|sha| format!(" in {}", &sha[..7])).unwrap_or_default();
        format!("{}:{}{before}{commit} — {}", comment.path, comment.line, comment.text)
    };
    comments.comments.iter().map(line).collect::<Vec<_>>().join("\n")
}

fn waiting_on_you(body: &BacklogBody) -> u32 {
    body.questions.iter().filter(|question| question.blocks && question.pending_answer.is_none()).count() as u32
}

fn empty_board() -> Board {
    Board {
        rev: 0,
        kebbie: "fake".to_string(),
        away_since: None,
        checkouts: Vec::new(),
        routines: Vec::new(),
        items: Vec::new(),
        sessions_outside_tasks: 0,
        next: None,
        counts: None,
        later: None,
        accepts: None,
        devices: None,
        backlog: None,
    }
}

fn started_task(address: &str, title: &str) -> Item {
    Item {
        address: address.to_string(),
        group: Group::Working,
        urgency: 3,
        summary: "queued by the lead".to_string(),
        next_for_you: None,
        agent: None,
        report: None,
        question: None,
        prompt: None,
        git: None,
        head: None,
        verdict: None,
        push_permission: None,
        push_waits: None,
        follows_main: false,
        ci: None,
        recover: None,
        routine: None,
        card: None,
        subtasks: None,
        order: None,
        pushable: None,
        require_accept: None,
        obstacle: None,
        title: Some(title.to_string()),
        profile: None,
        recipe: Some("small-fix".to_string()),
        assessment: None,
        second_opinion: None,
        review: None,
    }
}

async fn refuse(client: &HubClient, path: &str, reason: PairingFailure) -> Result<(), PairError> {
    let _: PairingView = client
        .post(&format!("{path}/refuse"), &PairingRefusal { reason })
        .await?;
    Ok(())
}
