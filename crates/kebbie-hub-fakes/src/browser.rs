use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use kebbie_hub_api::{
    AccountKeySetup, AccountReveal, AccountView, Boards, Bytes32, Bytes64, ConnectApproval,
    ConnectApproved, ConnectDenial, ConnectRequestView, CreationCeremony, DeviceRegistered,
    DeviceRequest, EmailSignup, LoginBegin, LoginFinish, PairingState, PairingView, PasskeyView,
    RegisterBegin, RegisterFinish, RequestCeremony, SignedIn, SignedMachineList, WrapperUpload,
};
use kebbie_hub_protocol::{
    AccountHandoff, AccountKey, Board, ConnectSecret, Envelope, Id, MachineList, OpenError,
    Payload, PublicKeys, SecretKeys, Unlock, open, sign_wrapper,
};
use reqwest::{Method, RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::Sha256;
use tokio::time::Instant;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::prelude::Url;

use crate::passkey::DiscoverablePasskey;
use crate::phone::PhoneState;
use crate::{HubError, PairError, now};

const SESSION_COOKIE: &str = "__Host-kebbie_session";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    Locked,
    Unlocked,
    NoPrf,
    NotWrapped,
}

pub struct Browser {
    pub hub: String,
    pub origin: String,
    pub handle: String,
    pub cookie: Option<String>,
    pub passkey: WebauthnAuthenticator<DiscoverablePasskey>,
    pub device: Option<PhoneState>,
    pub prf_capable: bool,
    pub lock: Lock,
    pub last_credential: Option<Vec<u8>>,
    pub seen_list: u64,
    prf_secrets: BTreeMap<Vec<u8>, [u8; 32]>,
    unlocked: Option<(AccountKey, SecretKeys)>,
    http: reqwest::Client,
}

pub struct Approval {
    pub approved: ConnectApproved,
    handoff: Option<(AccountHandoff, ConnectSecret)>,
}

#[derive(Debug)]
pub struct Link {
    pub code: String,
    pub connect: Option<ConnectSecret>,
}

#[derive(Debug)]
pub enum BoardProblem {
    UnconfirmedMachine,
    RolledBackList,
    NotSealedToTheAccount,
    Unopenable(OpenError),
    NotABoard,
    RevisionDiffers { inside: u64 },
}

impl fmt::Display for BoardProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardProblem::UnconfirmedMachine => {
                formatter.write_str("from a machine the account never confirmed")
            }
            BoardProblem::RolledBackList => {
                formatter.write_str("the hub served an older machine list than this browser saw")
            }
            BoardProblem::NotSealedToTheAccount => {
                formatter.write_str("not sealed to the account yet")
            }
            BoardProblem::Unopenable(error) => write!(formatter, "does not open: {error}"),
            BoardProblem::NotABoard => formatter.write_str("is not a board"),
            BoardProblem::RevisionDiffers { inside } => write!(
                formatter,
                "the hub's revision differs from the sealed one ({inside})"
            ),
        }
    }
}

#[derive(Debug)]
pub struct MachineBoard {
    pub machine: Id,
    pub name: String,
    pub rev: Option<u64>,
    pub board: Result<Board, BoardProblem>,
}

impl Link {
    pub fn parse(link: &str) -> Option<Link> {
        let (address, fragment) = link.split_once('#').unwrap_or((link, ""));
        let code = address
            .split_once('?')?
            .1
            .split('&')
            .find_map(|pair| pair.strip_prefix("code="))?
            .to_string();
        Some(Link {
            code,
            connect: ConnectSecret::parse(fragment),
        })
    }
}

fn status(code: &str) -> HubError {
    HubError::Status {
        status: 0,
        code: code.to_string(),
    }
}

fn decoded(text: &str) -> Result<Vec<u8>, HubError> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| status("not-base64url"))
}

impl Browser {
    pub fn new(hub: &str, origin: &str, handle: &str) -> Browser {
        Browser {
            hub: hub.trim_end_matches('/').to_string(),
            origin: origin.to_string(),
            handle: handle.to_string(),
            cookie: None,
            passkey: WebauthnAuthenticator::new(DiscoverablePasskey::new(true)),
            device: None,
            prf_capable: true,
            lock: Lock::Locked,
            last_credential: None,
            seen_list: 0,
            prf_secrets: BTreeMap::new(),
            unlocked: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn account_key(&self) -> Option<&AccountKey> {
        self.unlocked.as_ref().map(|(key, _)| key)
    }

    pub fn account_secret(&self) -> Option<([u8; 32], [u8; 32])> {
        self.unlocked.as_ref().map(|(_, secret)| secret.to_bytes())
    }

    pub fn forget_keys(&mut self) {
        self.unlocked = None;
        self.lock = Lock::Locked;
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.hub))
            .header("origin", &self.origin);
        if let Some(cookie) = &self.cookie {
            request = request.header("cookie", cookie);
        }
        request
    }

    fn keep_cookie(&mut self, response: &Response) {
        let set = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with(SESSION_COOKIE));
        if let Some(set) = set {
            let pair = set.split(';').next().unwrap_or_default().trim().to_string();
            self.cookie = Some(pair).filter(|pair| !pair.ends_with('='));
        }
    }

    async fn send<T: DeserializeOwned>(&mut self, request: RequestBuilder) -> Result<T, HubError> {
        let response = request
            .send()
            .await
            .map_err(|error| HubError::Transport(error.to_string()))?;
        self.keep_cookie(&response);
        if !response.status().is_success() {
            return Err(HubError::from_response(response).await);
        }
        let text = response
            .text()
            .await
            .map_err(|error| HubError::Transport(error.to_string()))?;
        serde_json::from_str(if text.is_empty() { "null" } else { &text })
            .map_err(|error| HubError::Transport(error.to_string()))
    }

    pub async fn get<T: DeserializeOwned>(&mut self, path: &str) -> Result<T, HubError> {
        let request = self.request(Method::GET, path);
        self.send(request).await
    }

    pub async fn post<B: Serialize, T: DeserializeOwned>(
        &mut self,
        path: &str,
        body: &B,
    ) -> Result<T, HubError> {
        let request = self.request(Method::POST, path).json(body);
        self.send(request).await
    }

    pub async fn put<B: Serialize, T: DeserializeOwned>(
        &mut self,
        path: &str,
        body: &B,
    ) -> Result<T, HubError> {
        let request = self.request(Method::PUT, path).json(body);
        self.send(request).await
    }

    pub async fn delete(&mut self, path: &str) -> Result<(), HubError> {
        let request = self.request(Method::DELETE, path);
        self.send::<Option<()>>(request).await.map(drop)
    }

    fn origin_url(&self) -> Url {
        Url::parse(&self.origin).expect("the browser's origin is a URL")
    }

    fn prf(&self, credential: &[u8], salt: &[u8; 32]) -> Option<[u8; 32]> {
        let secret = self.prf_secrets.get(credential)?;
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("any key length");
        mac.update(salt);
        Some(mac.finalize().into_bytes().into())
    }

    fn remember_credential(&mut self, credential: &[u8]) {
        if self.prf_capable && !self.prf_secrets.contains_key(credential) {
            let mut secret = [0u8; 32];
            getrandom::fill(&mut secret).expect("the system random source failed");
            self.prf_secrets.insert(credential.to_vec(), secret);
        }
    }

    pub async fn sign_up(&mut self, begin: RegisterBegin) -> Result<AccountView, HubError> {
        let begin = RegisterBegin {
            handle: self.handle.clone(),
            ..begin
        };
        let ceremony: CreationCeremony = self.post("/v1/a/register/begin", &begin).await?;
        let credential = self
            .passkey
            .do_registration(self.origin_url(), ceremony.options)
            .map_err(|error| HubError::Transport(format!("the passkey refused: {error:?}")))?;
        self.remember_credential(credential.raw_id.as_ref());
        let signed: SignedIn = self
            .post(
                "/v1/a/register/finish",
                &RegisterFinish {
                    ceremony: ceremony.ceremony,
                    credential,
                    name: Some("soft passkey".to_string()),
                },
            )
            .await?;
        self.unlock(&signed).await?;
        Ok(signed.account)
    }

    pub async fn sign_up_with_invite(&mut self, invite: &str) -> Result<AccountView, HubError> {
        self.sign_up(RegisterBegin {
            invite: invite.to_string(),
            ..RegisterBegin::default()
        })
        .await
    }

    pub async fn sign_up_as_owner(&mut self, setup_url: &str) -> Result<AccountView, HubError> {
        let token = setup_url
            .split_once("token=")
            .map_or(setup_url, |(_, token)| token);
        self.sign_up(RegisterBegin {
            setup: Some(token.to_string()),
            ..RegisterBegin::default()
        })
        .await
    }

    pub async fn sign_up_verified(&mut self, link: &str) -> Result<AccountView, HubError> {
        let token = link.split_once("verify=").map_or(link, |(_, token)| token);
        self.sign_up(RegisterBegin {
            verification: Some(token.trim().to_string()),
            ..RegisterBegin::default()
        })
        .await
    }

    pub async fn request_email(&mut self, email: &str) -> Result<(), HubError> {
        self.post::<_, Option<()>>(
            "/v1/a/signup/email",
            &EmailSignup {
                email: email.to_string(),
            },
        )
        .await
        .map(drop)
    }

    pub async fn log_in(&mut self) -> Result<AccountView, HubError> {
        self.log_in_as(LoginBegin::default()).await
    }

    pub async fn log_in_with_handle(&mut self) -> Result<AccountView, HubError> {
        self.log_in_as(LoginBegin {
            handle: Some(self.handle.clone()),
            credential: None,
        })
        .await
    }

    pub async fn log_in_with_credential(
        &mut self,
        credential: &[u8],
    ) -> Result<AccountView, HubError> {
        self.log_in_as(LoginBegin {
            handle: None,
            credential: Some(URL_SAFE_NO_PAD.encode(credential)),
        })
        .await
    }

    async fn log_in_as(&mut self, begin: LoginBegin) -> Result<AccountView, HubError> {
        let ceremony: RequestCeremony = self.post("/v1/a/login/begin", &begin).await?;
        let credential = self
            .passkey
            .do_authentication(self.origin_url(), ceremony.options)
            .map_err(|error| HubError::Transport(format!("the passkey refused: {error:?}")))?;
        let signed: SignedIn = self
            .post(
                "/v1/a/login/finish",
                &LoginFinish {
                    ceremony: ceremony.ceremony,
                    credential,
                },
            )
            .await?;
        self.unlock(&signed).await?;
        Ok(signed.account)
    }

    async fn unlock(&mut self, signed: &SignedIn) -> Result<(), HubError> {
        let credential = decoded(&signed.unlock.credential)?;
        self.last_credential = Some(credential.clone());
        let salt = signed.unlock.prf_salt.0;
        let Some(prf_output) = self.prf(&credential, &salt) else {
            self.forget_keys();
            self.lock = Lock::NoPrf;
            return Ok(());
        };
        let account = Id::parse(&signed.account.account).map_err(|_| status("bad-account"))?;
        let unlock = |version| Unlock {
            account: &account,
            credential: &credential,
            version,
            prf_output: &prf_output,
            salt: &salt,
        };
        let unlocked = match (&signed.account.key, &signed.unlock.wrapped) {
            (None, _) => {
                let secret = SecretKeys::generate();
                let key = AccountKey::new(account.clone(), 1, secret.public())
                    .ok_or_else(|| status("bad-account"))?;
                let wrapped = unlock(key.version)
                    .wrap(&secret)
                    .map_err(|_| status("credential-too-long"))?;
                let signature = sign_wrapper(&secret, &key, &credential, &wrapped)
                    .map_err(|_| status("credential-too-long"))?;
                self.post::<_, Option<()>>(
                    "/v1/a/key",
                    &AccountKeySetup {
                        key: key.clone(),
                        credential: signed.unlock.credential.clone(),
                        wrapped: URL_SAFE_NO_PAD.encode(&wrapped),
                        signature: Bytes64(signature),
                    },
                )
                .await?;
                (key, secret)
            }
            (Some(key), Some(wrapped)) => {
                let secret = unlock(key.version)
                    .unwrap(&decoded(wrapped)?)
                    .map_err(|_| status("wrong-account-key"))?;
                if secret.public() != key.keys {
                    return Err(status("key-mismatch"));
                }
                (key.clone(), secret)
            }
            (Some(_), None) => {
                self.forget_keys();
                self.lock = Lock::NotWrapped;
                return Ok(());
            }
        };
        self.unlocked = Some(unlocked);
        self.lock = Lock::Unlocked;
        Ok(())
    }

    pub async fn add_passkey(&mut self) -> Result<PasskeyView, HubError> {
        let ceremony: CreationCeremony = self.post("/v1/a/passkeys/begin", &()).await?;
        let credential = self
            .passkey
            .do_registration(self.origin_url(), ceremony.options)
            .map_err(|error| HubError::Transport(format!("the passkey refused: {error:?}")))?;
        let raw = credential.raw_id.as_ref().to_vec();
        self.remember_credential(&raw);
        let added: PasskeyView = self
            .post(
                "/v1/a/passkeys/finish",
                &RegisterFinish {
                    ceremony: ceremony.ceremony,
                    credential,
                    name: Some("second soft passkey".to_string()),
                },
            )
            .await?;
        let Some((key, secret)) = self.unlocked.clone() else {
            return Ok(added);
        };
        let Some(prf_output) = self.prf(&raw, &ceremony.prf_salt.0) else {
            return Ok(added);
        };
        let wrapped = Unlock {
            account: &key.account,
            credential: &raw,
            version: key.version,
            prf_output: &prf_output,
            salt: &ceremony.prf_salt.0,
        }
        .wrap(&secret)
        .map_err(|_| status("credential-too-long"))?;
        let signature = sign_wrapper(&secret, &key, &raw, &wrapped)
            .map_err(|_| status("credential-too-long"))?;
        self.put::<_, Option<()>>(
            &format!("/v1/a/passkeys/{}/wrapper", added.passkey),
            &WrapperUpload {
                key_version: key.version,
                wrapped: URL_SAFE_NO_PAD.encode(&wrapped),
                signature: Bytes64(signature),
            },
        )
        .await?;
        Ok(PasskeyView {
            unlocks: true,
            ..added
        })
    }

    pub async fn account(&mut self) -> Result<AccountView, HubError> {
        self.get("/v1/a/account").await
    }

    pub async fn ensure_device(&mut self) -> Result<&mut PhoneState, HubError> {
        if self.device.is_none() {
            let registered: DeviceRegistered = self
                .post(
                    "/v1/a/devices",
                    &DeviceRequest {
                        name: Some("browser".to_string()),
                    },
                )
                .await?;
            self.device = Some(PhoneState::registered(
                &self.hub,
                registered.device,
                &registered.token,
            ));
        }
        Ok(self.device.as_mut().expect("the device was just set"))
    }

    pub async fn connect_request(&mut self, code: &str) -> Result<ConnectRequestView, HubError> {
        self.get(&format!("/v1/a/connect?code={code}")).await
    }

    pub async fn approve_connect(
        &mut self,
        link: &str,
        hand_off: bool,
    ) -> Result<Approval, HubError> {
        let link = Link::parse(link).ok_or_else(|| status("not-a-connect-link"))?;
        let handoff = match (hand_off, link.connect) {
            (true, Some(connect)) => {
                let key = self
                    .account_key()
                    .cloned()
                    .ok_or_else(|| status("locked"))?;
                Some((AccountHandoff::start(key, &connect.pairing_code()), connect))
            }
            (true, None) => return Err(status("no-secret-in-the-link")),
            (false, _) => None,
        };
        let approval = ConnectApproval {
            code: link.code,
            commitment: handoff
                .as_ref()
                .map(|(handoff, _)| Bytes32(handoff.commitment())),
        };
        let approved = match self.post("/v1/a/connect/approve", &approval).await {
            Err(HubError::Status { status: 401, code }) if code == "reauth-required" => {
                self.log_in().await?;
                self.post("/v1/a/connect/approve", &approval).await?
            }
            other => other?,
        };
        Ok(Approval { approved, handoff })
    }

    async fn await_pairing(
        &mut self,
        pairing: &str,
        seen: PairingState,
        deadline: Instant,
    ) -> Result<PairingView, PairError> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(PairError::Timeout);
            }
            let wait = remaining.as_secs().clamp(1, 25);
            let view: PairingView = self
                .get(&format!(
                    "/v1/a/pairings/{pairing}?seen={}&wait={wait}",
                    seen.name()
                ))
                .await?;
            if view.state != seen {
                return Ok(view);
            }
        }
    }

    pub async fn hand_off(
        &mut self,
        approval: Approval,
        timeout: Duration,
    ) -> Result<Id, PairError> {
        let (Some(pairing), Some((handoff, connect))) =
            (approval.approved.pairing, approval.handoff)
        else {
            return Err(PairError::Incomplete("the pairing"));
        };
        let deadline = Instant::now() + timeout;
        let replied = self
            .await_pairing(&pairing, PairingState::Committed, deadline)
            .await?;
        if replied.state != PairingState::Replied {
            return Err(PairError::Ended {
                state: replied.state,
                reason: replied.reason,
            });
        }
        let machine_keys = replied
            .machine_keys
            .ok_or(PairError::Incomplete("the machine keys"))?;
        if !connect.names(&machine_keys) {
            return Err(PairError::WrongMachine);
        }
        let (key, nonce) = handoff.reveal();
        let _: PairingView = self
            .post(
                &format!("/v1/a/pairings/{pairing}/reveal"),
                &AccountReveal {
                    key,
                    nonce: Bytes32(nonce),
                },
            )
            .await?;
        let settled = self
            .await_pairing(&pairing, PairingState::Revealed, deadline)
            .await?;
        if settled.state != PairingState::Admitted {
            return Err(PairError::Ended {
                state: settled.state,
                reason: settled.reason,
            });
        }
        let listing: Boards = self.get("/v1/a/boards").await?;
        let list = self
            .trusted_list(&listing)
            .map_err(|_| PairError::Incomplete("a machine list this browser trusts"))?;
        self.publish(&list.with(replied.machine.clone(), machine_keys))
            .await?;
        Ok(replied.machine)
    }

    fn trusted_list(&mut self, listing: &Boards) -> Result<MachineList, BoardProblem> {
        let (key, _) = self
            .unlocked
            .as_ref()
            .ok_or(BoardProblem::NotSealedToTheAccount)?;
        let list = match &listing.list {
            Some(signed) if key.verifies_list(&signed.list, &signed.signature.0) => {
                signed.list.clone()
            }
            Some(_) => return Err(BoardProblem::UnconfirmedMachine),
            None => MachineList::default(),
        };
        if list.counter < self.seen_list {
            return Err(BoardProblem::RolledBackList);
        }
        self.seen_list = list.counter;
        Ok(list)
    }

    async fn publish(&mut self, list: &MachineList) -> Result<(), HubError> {
        let (key, secret) = self.unlocked.clone().ok_or_else(|| status("locked"))?;
        let signature = list
            .sign(&secret, &key.account)
            .ok_or_else(|| status("bad-list"))?;
        self.put::<_, Option<()>>(
            "/v1/a/machines",
            &SignedMachineList {
                list: list.clone(),
                signature: Bytes64(signature),
            },
        )
        .await?;
        self.seen_list = list.counter;
        Ok(())
    }

    pub async fn remove_machine(&mut self, machine: &Id) -> Result<(), HubError> {
        let listing: Boards = self.get("/v1/a/boards").await?;
        let list = self
            .trusted_list(&listing)
            .map_err(|problem| status(&problem.to_string()))?;
        self.publish(&list.without(machine)).await?;
        self.delete(&format!("/v1/a/machines/{machine}")).await
    }

    pub async fn boards(&mut self) -> Result<Vec<MachineBoard>, HubError> {
        let listing: Boards = self.get("/v1/a/boards").await?;
        let mut list = self.trusted_list(&listing);
        if let Ok(trusted) = &list {
            let gone: Vec<Id> = trusted
                .machines
                .iter()
                .filter(|listed| {
                    !listing
                        .boards
                        .iter()
                        .any(|entry| entry.machine == listed.machine)
                })
                .map(|listed| listed.machine.clone())
                .collect();
            if !gone.is_empty() {
                let mut pruned = trusted.clone();
                for machine in &gone {
                    pruned = pruned.without(machine);
                }
                self.publish(&pruned).await?;
                list = Ok(pruned);
            }
        }
        Ok(listing
            .boards
            .into_iter()
            .map(|entry| MachineBoard {
                board: match &list {
                    Ok(list) if list.trusts(&entry.machine, &entry.machine_keys) => self
                        .open_board(
                            &entry.machine,
                            &entry.machine_keys,
                            entry.rev,
                            entry.envelope.as_ref(),
                        ),
                    Ok(_) => Err(BoardProblem::UnconfirmedMachine),
                    Err(BoardProblem::RolledBackList) => Err(BoardProblem::RolledBackList),
                    Err(BoardProblem::NotSealedToTheAccount) => {
                        Err(BoardProblem::NotSealedToTheAccount)
                    }
                    Err(_) => Err(BoardProblem::UnconfirmedMachine),
                },
                machine: entry.machine,
                name: entry.name,
                rev: entry.rev,
            })
            .collect())
    }

    fn open_board(
        &self,
        machine: &Id,
        keys: &PublicKeys,
        rev: Option<u64>,
        envelope: Option<&Envelope>,
    ) -> Result<Board, BoardProblem> {
        let (key, secret) = self
            .unlocked
            .as_ref()
            .ok_or(BoardProblem::NotSealedToTheAccount)?;
        let envelope = envelope.ok_or(BoardProblem::NotSealedToTheAccount)?;
        let payload = open(envelope, &key.account, secret, machine, keys, now())
            .map_err(BoardProblem::Unopenable)?;
        let Payload::Board(board) = payload else {
            return Err(BoardProblem::NotABoard);
        };
        if Some(board.rev) != rev {
            return Err(BoardProblem::RevisionDiffers { inside: board.rev });
        }
        Ok(board)
    }

    pub async fn deny_connect(&mut self, code: &str) -> Result<(), HubError> {
        self.post::<_, Option<()>>(
            "/v1/a/connect/deny",
            &ConnectDenial {
                code: code.to_string(),
            },
        )
        .await
        .map(drop)
    }
}
