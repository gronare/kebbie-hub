use std::time::Duration;

use kelpie_hub_api::{
    BoardStored, BoardUpload, Bytes32, Inbox, JoinRequest, Joined, PairingFailure, PairingOpened,
    PairingRefusal, PairingReply, PairingState, PairingView,
};
use kelpie_hub_protocol::{
    Board, Envelope, Header, Id, Kind, MachinePairing, Payload, Prefix, PublicKeys, SecretKeys,
    seal,
};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::{Error, HubClient, KeyFile, PairError, now};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmittedDevice {
    pub id: Id,
    pub keys: PublicKeys,
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
    Ok(MachineState {
        hub: hub.to_string(),
        machine: joined.machine,
        name: name.to_string(),
        token: joined.token,
        secret,
        devices: Vec::new(),
        last_rev: 0,
        inbox_seq: 0,
    })
}

impl MachineState {
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
        let client = self.client();
        let opened: PairingOpened = client.post("/v1/m/pairings", &()).await?;
        show_code(&opened.code);
        let path = format!("/v1/m/pairings/{}", opened.pairing);
        let deadline = Instant::now() + timeout;
        let committed = self
            .next_state(&client, &path, PairingState::Open, deadline)
            .await?;
        let commitment = committed
            .commitment
            .ok_or(PairError::Incomplete("the commitment"))?;
        let pairing = MachinePairing::answer(self.public(), commitment.0, &opened.code);
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
        });
        Ok(device)
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

    pub async fn post_board(&mut self, board: Board) -> Result<u64, Error> {
        let rev = self.last_rev + 1;
        self.post_board_at(board, rev).await
    }

    pub async fn post_board_at(&mut self, board: Board, rev: u64) -> Result<u64, Error> {
        let envelope = self.seal_board(board, rev, now())?;
        self.upload_board(rev, envelope).await
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

async fn refuse(client: &HubClient, path: &str, reason: PairingFailure) -> Result<(), PairError> {
    let _: PairingView = client
        .post(&format!("{path}/refuse"), &PairingRefusal { reason })
        .await?;
    Ok(())
}
