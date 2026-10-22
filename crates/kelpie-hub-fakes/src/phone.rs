use std::fmt;
use std::time::Duration;

use kelpie_hub_api::{
    Boards, Bytes32, CommitRequest, Committed, PairingState, PairingView, RevealRequest,
    normalize_code,
};
use kelpie_hub_protocol::{
    Board, DevicePairing, Id, OpenError, Payload, PublicKeys, SecretKeys, open,
};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

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
}

#[derive(Debug)]
pub enum BoardProblem {
    UnknownMachine,
    NotSealedToThisDevice,
    Unopenable(OpenError),
    NotABoard,
    RevisionDiffers { inside: u64 },
}

impl fmt::Display for BoardProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardProblem::UnknownMachine => {
                formatter.write_str("from a machine this phone never paired with")
            }
            BoardProblem::NotSealedToThisDevice => {
                formatter.write_str("not sealed to this phone yet")
            }
            BoardProblem::Unopenable(error) => write!(formatter, "does not open: {error}"),
            BoardProblem::NotABoard => formatter.write_str("is not a board"),
            BoardProblem::RevisionDiffers { inside } => {
                write!(
                    formatter,
                    "the hub's revision differs from the sealed one ({inside})"
                )
            }
        }
    }
}

#[derive(Debug)]
pub struct MachineBoard {
    pub machine: Id,
    pub rev: u64,
    pub board: Result<Board, BoardProblem>,
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

    pub async fn pair(
        &mut self,
        code: &str,
        timeout: Duration,
        show_code: impl FnOnce(&str),
    ) -> Result<Id, PairError> {
        let outcome = self.try_pair(code, timeout, show_code).await;
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

    async fn try_pair(
        &mut self,
        code: &str,
        timeout: Duration,
        show_code: impl FnOnce(&str),
    ) -> Result<Id, PairError> {
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
        let client = self.client();
        let path = format!("/v1/d/pairings/{}", committed.pairing);
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
        show_code(&pairing.comparison_code(&machine_keys, &machine_nonce.0));
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

    pub async fn boards(&self) -> Result<Vec<MachineBoard>, Error> {
        let Some(device) = &self.device else {
            return Ok(Vec::new());
        };
        let listing: Boards = self.client().get("/v1/d/boards", &[]).await?;
        Ok(listing
            .boards
            .into_iter()
            .map(|entry| MachineBoard {
                board: self.open_board(device, &entry.machine, entry.rev, entry.envelope.as_ref()),
                machine: entry.machine,
                rev: entry.rev,
            })
            .collect())
    }

    fn open_board(
        &self,
        device: &Id,
        machine: &Id,
        rev: u64,
        envelope: Option<&kelpie_hub_protocol::Envelope>,
    ) -> Result<Board, BoardProblem> {
        let paired = self
            .machines
            .iter()
            .find(|paired| &paired.id == machine)
            .ok_or(BoardProblem::UnknownMachine)?;
        let envelope = envelope.ok_or(BoardProblem::NotSealedToThisDevice)?;
        if &envelope.header.machine != machine {
            return Err(BoardProblem::Unopenable(OpenError::WrongSender));
        }
        let payload = open(
            envelope,
            device,
            &self.secret(),
            machine,
            &paired.keys,
            now(),
        )
        .map_err(BoardProblem::Unopenable)?;
        let Payload::Board(board) = payload else {
            return Err(BoardProblem::NotABoard);
        };
        if board.rev != rev {
            return Err(BoardProblem::RevisionDiffers { inside: board.rev });
        }
        Ok(board)
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
