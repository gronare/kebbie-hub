use std::time::Duration;

use kelpie_hub_api::{
    AccountView, AddView, Bytes32, ConnectApproval, ConnectApproved, ConnectDenial,
    ConnectRequestView, CreationCeremony, DeviceRegistered, DeviceRequest, EmailSignup, LoginBegin,
    LoginFinish, RegisterBegin, RegisterFinish, RequestCeremony,
};
use kelpie_hub_protocol::{ConnectSecret, DevicePairing, Id};
use reqwest::{Method, RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::prelude::Url;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;

use crate::phone::PhoneState;
use crate::{HubError, PairError};

const SESSION_COOKIE: &str = "__Host-kelpie_session";

pub struct Browser {
    pub hub: String,
    pub origin: String,
    pub handle: String,
    pub cookie: Option<String>,
    pub passkey: WebauthnAuthenticator<SoftPasskey>,
    pub device: Option<PhoneState>,
    http: reqwest::Client,
}

pub struct Approval {
    pub approved: ConnectApproved,
    board: Option<(DevicePairing, ConnectSecret)>,
}

#[derive(Debug)]
pub struct Link {
    pub code: String,
    pub connect: Option<ConnectSecret>,
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

impl Browser {
    pub fn new(hub: &str, origin: &str, handle: &str) -> Browser {
        Browser {
            hub: hub.trim_end_matches('/').to_string(),
            origin: origin.to_string(),
            handle: handle.to_string(),
            cookie: None,
            passkey: WebauthnAuthenticator::new(SoftPasskey::new(true)),
            device: None,
            http: reqwest::Client::new(),
        }
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

    pub async fn delete(&mut self, path: &str) -> Result<(), HubError> {
        let request = self.request(Method::DELETE, path);
        self.send::<Option<()>>(request).await.map(drop)
    }

    fn origin_url(&self) -> Url {
        Url::parse(&self.origin).expect("the browser's origin is a URL")
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
        self.post(
            "/v1/a/register/finish",
            &RegisterFinish {
                ceremony: ceremony.ceremony,
                credential,
                name: Some("soft passkey".to_string()),
            },
        )
        .await
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
        let ceremony: RequestCeremony = self
            .post(
                "/v1/a/login/begin",
                &LoginBegin {
                    handle: self.handle.clone(),
                },
            )
            .await?;
        let credential = self
            .passkey
            .do_authentication(self.origin_url(), ceremony.options)
            .map_err(|error| HubError::Transport(format!("the passkey refused: {error:?}")))?;
        self.post(
            "/v1/a/login/finish",
            &LoginFinish {
                ceremony: ceremony.ceremony,
                credential,
            },
        )
        .await
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
        show_board: bool,
    ) -> Result<Approval, HubError> {
        let link = Link::parse(link).ok_or_else(|| status("not-a-connect-link"))?;
        let board = match (show_board, link.connect) {
            (true, Some(connect)) => {
                let pairing = self.ensure_device().await?.connect_pairing(&connect);
                Some((pairing, connect))
            }
            (true, None) => return Err(status("no-secret-in-the-link")),
            (false, _) => None,
        };
        let approval = ConnectApproval {
            code: link.code,
            device: board
                .as_ref()
                .and_then(|_| self.device.as_ref()?.device.clone()),
            commitment: board
                .as_ref()
                .map(|(pairing, _)| Bytes32(pairing.commitment())),
        };
        let approved = match self.post("/v1/a/connect/approve", &approval).await {
            Err(HubError::Status { status: 401, code }) if code == "reauth-required" => {
                self.log_in().await?;
                self.post("/v1/a/connect/approve", &approval).await?
            }
            other => other?,
        };
        Ok(Approval { approved, board })
    }

    pub async fn show_board(
        &mut self,
        approval: Approval,
        timeout: Duration,
    ) -> Result<Id, PairError> {
        let (Some(pairing_id), Some((pairing, connect))) =
            (approval.approved.pairing, approval.board)
        else {
            return Err(PairError::Incomplete("the pairing"));
        };
        let device = self
            .device
            .as_mut()
            .ok_or(PairError::Incomplete("the device"))?;
        device
            .complete_connect(&pairing_id, &pairing, &connect, timeout)
            .await
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

    pub async fn add_view(&mut self, link: &str) -> Result<AddView, HubError> {
        let link = Link::parse(link).ok_or_else(|| status("not-an-add-link"))?;
        self.get(&format!("/v1/a/add?code={}", link.code)).await
    }

    pub async fn add_this_phone<C>(
        &mut self,
        link: &str,
        timeout: Duration,
        confirm: C,
    ) -> Result<Id, PairError>
    where
        C: AsyncFnOnce(String) -> bool,
    {
        let code = Link::parse(link).ok_or(PairError::BadCode)?.code;
        self.add_view(link).await?;
        let device = self.ensure_device().await?;
        device.pair(&code, timeout, confirm).await
    }
}
