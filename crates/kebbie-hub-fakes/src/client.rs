use std::fmt;
use std::time::Duration;

use kebbie_hub_api::{ErrorBody, PLAN_INACTIVE, PRO_REQUIRED, PairingState, PairingView};
use reqwest::{Method, RequestBuilder};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::time::Instant;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(35);
const LONGEST_WAIT: u64 = 25;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubError {
    Status { status: u16, code: String },
    CommandsRefused { url: Option<String> },
    ProRequired { url: Option<String> },
    Transport(String),
}

impl HubError {
    pub fn status(&self) -> Option<u16> {
        match self {
            HubError::Status { status, .. } => Some(*status),
            HubError::CommandsRefused { .. } | HubError::ProRequired { .. } => Some(402),
            HubError::Transport(_) => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            HubError::Status { code, .. } => Some(code),
            HubError::CommandsRefused { .. } => Some(PLAN_INACTIVE),
            HubError::ProRequired { .. } => Some(PRO_REQUIRED),
            HubError::Transport(_) => None,
        }
    }

    pub(crate) async fn from_response(response: reqwest::Response) -> HubError {
        let status = response.status();
        let body = response.json::<ErrorBody>().await.ok();
        match body {
            Some(body) if status.as_u16() == 402 && body.error == PLAN_INACTIVE => {
                HubError::CommandsRefused { url: body.url }
            }
            Some(body) if status.as_u16() == 402 && body.error == PRO_REQUIRED => {
                HubError::ProRequired { url: body.url }
            }
            Some(body) => HubError::Status {
                status: status.as_u16(),
                code: body.error,
            },
            None => HubError::Status {
                status: status.as_u16(),
                code: status.canonical_reason().unwrap_or("error").to_string(),
            },
        }
    }
}

impl fmt::Display for HubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HubError::Status { status, code } => {
                write!(formatter, "the hub answered {status} {code}")
            }
            HubError::CommandsRefused { url: Some(url) } => write!(
                formatter,
                "this hub takes no commands for this machine's account; see {url}"
            ),
            HubError::CommandsRefused { url: None } => {
                formatter.write_str("this hub takes no commands for this machine's account")
            }
            HubError::ProRequired { url: Some(url) } => write!(formatter, "that is part of Pro; see {url}"),
            HubError::ProRequired { url: None } => formatter.write_str("that is part of Pro"),
            HubError::Transport(message) => write!(formatter, "could not reach the hub: {message}"),
        }
    }
}

impl std::error::Error for HubError {}

#[derive(Clone)]
pub struct HubClient {
    base: String,
    token: Option<String>,
    headers: Vec<(String, String)>,
    http: reqwest::Client,
}

impl HubClient {
    pub fn new(base: &str, token: Option<&str>) -> HubClient {
        HubClient {
            base: base.trim_end_matches('/').to_string(),
            token: token.map(str::to_string),
            headers: Vec::new(),
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("an HTTP client with a timeout builds"),
        }
    }

    pub fn with_token(&self, token: &str) -> HubClient {
        HubClient {
            token: Some(token.to_string()),
            ..self.clone()
        }
    }

    pub fn with_header(&self, name: &str, value: &str) -> HubClient {
        let mut client = self.clone();
        client.headers.push((name.to_string(), value.to_string()));
        client
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut request = self.http.request(method, format!("{}{path}", self.base));
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn send<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, HubError> {
        let response = request
            .send()
            .await
            .map_err(|error| HubError::Transport(error.to_string()))?;
        if !response.status().is_success() {
            return Err(HubError::from_response(response).await);
        }
        response
            .json()
            .await
            .map_err(|error| HubError::Transport(error.to_string()))
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, HubError> {
        HubClient::send(self.request(Method::GET, path).query(query)).await
    }

    pub async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, HubError> {
        HubClient::send(self.request(Method::POST, path).json(body)).await
    }

    pub async fn put<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, HubError> {
        HubClient::send(self.request(Method::PUT, path).json(body)).await
    }

    pub async fn post_empty<B: Serialize>(&self, path: &str, body: &B) -> Result<(), HubError> {
        HubClient::send_empty(self.request(Method::POST, path).json(body)).await
    }

    pub async fn delete(&self, path: &str) -> Result<(), HubError> {
        HubClient::send_empty(self.request(Method::DELETE, path)).await
    }

    pub async fn put_empty<B: Serialize>(&self, path: &str, body: &B) -> Result<(), HubError> {
        HubClient::send_empty(self.request(Method::PUT, path).json(body)).await
    }

    pub async fn put_bytes(&self, path: &str, body: Vec<u8>) -> Result<(), HubError> {
        HubClient::send_empty(
            self.request(Method::PUT, path)
                .header("content-type", "application/octet-stream")
                .body(body),
        )
        .await
    }

    async fn send_empty(request: RequestBuilder) -> Result<(), HubError> {
        let response = request
            .send()
            .await
            .map_err(|error| HubError::Transport(error.to_string()))?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(HubError::from_response(response).await)
    }

    pub async fn await_pairing(
        &self,
        path: &str,
        seen: PairingState,
        deadline: Instant,
    ) -> Result<Option<PairingView>, HubError> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let wait = remaining.as_secs().clamp(1, LONGEST_WAIT);
            let view: PairingView = self
                .get(
                    path,
                    &[
                        ("seen", seen.name().to_string()),
                        ("wait", wait.to_string()),
                    ],
                )
                .await?;
            if view.state != seen {
                return Ok(Some(view));
            }
        }
    }
}
