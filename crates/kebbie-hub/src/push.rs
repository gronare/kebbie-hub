use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use kebbie_hub_api::{NoticePosted, NoticeUpload, PushSubscription, VapidKey};
use kebbie_hub_protocol::{Id, Kind, Prefix, Urgency, verify};
use reqwest::header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, RETRY_AFTER};
use tokio::sync::Semaphore;
use webauthn_rs::prelude::Url;

use crate::auth::{Anonymous, DeviceAuth, MachineAuth};
use crate::error::ApiError;
use crate::store::Subscribed;
use crate::webpush::{
    MAX_PUSH_PLAINTEXT, SubscriptionKeys, Vapid, audience, check_endpoint, encrypt, parse_keys,
    topic,
};
use crate::{Hub, now};

const VAPID_TOKEN_SECONDS: u64 = 12 * 3_600;
const LONGEST_BACKOFF: Duration = Duration::from_secs(60);

pub struct Pusher {
    pub vapid: Vapid,
    http: reqwest::Client,
    pending: Arc<Semaphore>,
    capacity: usize,
    sending: Semaphore,
}

impl Pusher {
    pub fn new(
        vapid: Vapid,
        timeout: Duration,
        in_flight: usize,
        pending: usize,
    ) -> Result<Pusher, String> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("the push client does not build: {error}"))?;
        Ok(Pusher {
            vapid,
            http,
            pending: Arc::new(Semaphore::new(pending.max(1))),
            capacity: pending.max(1),
            sending: Semaphore::new(in_flight.max(1)),
        })
    }

    pub fn pending(&self) -> usize {
        self.capacity - self.pending.available_permits()
    }
}

struct PushJob {
    device: Id,
    endpoint: String,
    keys: SubscriptionKeys,
    payload: Vec<u8>,
    expires_at: u64,
    urgency: &'static str,
    topic: Option<String>,
}

pub async fn vapid(
    State(hub): State<Arc<Hub>>,
    client: Anonymous,
) -> Result<Json<VapidKey>, ApiError> {
    hub.limit_anonymous(&client.address)?;
    Ok(Json(VapidKey {
        key: hub.pusher.vapid.public_key().to_string(),
    }))
}

pub async fn subscribe(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
    Json(subscription): Json<PushSubscription>,
) -> Result<StatusCode, ApiError> {
    check_endpoint(&subscription.endpoint, &hub.config().push_allow_hosts)
        .map_err(ApiError::bad_request)?;
    let keys = parse_keys(&subscription.keys.p256dh, &subscription.keys.auth)
        .ok_or_else(|| ApiError::bad_request("bad-push-keys"))?;
    let host = Url::parse(&subscription.endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default();
    let at = now();
    let store = hub.store();
    let outcome = store.subscribe_push(
        &device,
        &subscription.endpoint,
        &keys.p256dh,
        &keys.auth,
        at,
    )?;
    let detail = format!("host={host}");
    match outcome {
        Subscribed::Taken => {
            store.log(
                at,
                "push.endpoint-taken",
                None,
                Some(&device),
                Some(&detail),
            )?;
            return Err(ApiError::conflict("endpoint-taken"));
        }
        Subscribed::Moved(from) => {
            store.log(
                at,
                "push.moved",
                None,
                Some(&device),
                Some(&format!("{detail} from={from}")),
            )?;
        }
        Subscribed::New => {}
    }
    store.log(at, "push.subscribed", None, Some(&device), Some(&detail))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unsubscribe(
    State(hub): State<Arc<Hub>>,
    DeviceAuth(device): DeviceAuth,
) -> Result<StatusCode, ApiError> {
    let store = hub.store();
    if store.unsubscribe_push(&device)? {
        store.log(now(), "push.unsubscribed", None, Some(&device), None)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn urgency_header(urgency: Option<Urgency>) -> &'static str {
    match urgency {
        Some(Urgency::High) => "high",
        Some(Urgency::Normal) | None => "normal",
    }
}

pub async fn post_notice(
    State(hub): State<Arc<Hub>>,
    MachineAuth(machine): MachineAuth,
    Json(upload): Json<NoticeUpload>,
) -> Result<(StatusCode, Json<NoticePosted>), ApiError> {
    let envelope = &upload.envelope;
    let header = &envelope.header;
    if header.kind != Kind::Notice {
        return Err(ApiError::bad_request("not-a-notice"));
    }
    if header.machine != machine {
        return Err(ApiError::bad_request("wrong-machine"));
    }
    let at = now();
    let keys = hub
        .store()
        .machine_keys(&machine)?
        .ok_or_else(ApiError::unauthorized)?;
    verify(envelope, &machine, &keys, at).map_err(|error| ApiError::bad_request(error.key()))?;
    let devices: Vec<&Id> = envelope
        .keys
        .keys()
        .filter(|recipient| recipient.prefix() == Prefix::Device)
        .collect();
    let stray = envelope
        .keys
        .keys()
        .any(|recipient| *recipient != machine && recipient.prefix() != Prefix::Device);
    if devices.is_empty() || stray {
        return Err(ApiError::bad_request("bad-recipients"));
    }
    let payloads = devices
        .into_iter()
        .map(|device| {
            let cut = envelope
                .for_recipient(device)
                .expect("the device holds a key row");
            (device.clone(), cut.to_json().into_bytes())
        })
        .collect::<Vec<_>>();
    if payloads
        .iter()
        .any(|(_, payload)| payload.len() > MAX_PUSH_PLAINTEXT)
    {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too-large"));
    }
    if !hub.notice_limiter.allow(machine.as_str()) {
        return Err(ApiError::rate_limited());
    }
    let recipients = payloads.len();
    let mut jobs = Vec::new();
    {
        let store = hub.store();
        if !store.record_notice(&machine, &header.id, at, header.exp)? {
            return Err(ApiError::conflict("replayed-envelope"));
        }
        for (device, payload) in payloads {
            if !store.is_admitted(&machine, &device)? {
                continue;
            }
            let Some(subscription) = store.push_subscription(&device)? else {
                continue;
            };
            if !hub.device_push_limiter.allow(device.as_str()) {
                store.log(at, "push.capped", Some(&machine), Some(&device), None)?;
                continue;
            }
            jobs.push(PushJob {
                device,
                endpoint: subscription.endpoint,
                keys: SubscriptionKeys {
                    p256dh: subscription.p256dh,
                    auth: subscription.auth,
                },
                payload,
                expires_at: header.exp,
                urgency: urgency_header(header.urgency),
                topic: header.collapse.as_deref().map(topic),
            });
        }
        store.log(
            at,
            "notice.posted",
            Some(&machine),
            None,
            Some(&format!(
                "bytes={} recipients={recipients} pushes={}",
                envelope.to_json().len(),
                jobs.len()
            )),
        )?;
    }
    let pushes = jobs
        .into_iter()
        .map(|job| dispatch(&hub, job))
        .filter(|queued| *queued)
        .count();
    Ok((
        StatusCode::ACCEPTED,
        Json(NoticePosted {
            notice: header.id.clone(),
            pushes,
        }),
    ))
}

fn log(hub: &Hub, kind: &str, device: &Id, detail: Option<String>) {
    if let Err(error) = hub
        .store()
        .log(now(), kind, None, Some(device), detail.as_deref())
    {
        eprintln!("kebbie-hub: could not log {kind}: {error}");
    }
}

fn dispatch(hub: &Arc<Hub>, job: PushJob) -> bool {
    let Ok(slot) = Arc::clone(&hub.pusher.pending).try_acquire_owned() else {
        log(
            hub,
            "push.dropped",
            &job.device,
            Some("reason=queue-full".to_string()),
        );
        return false;
    };
    let hub = Arc::clone(hub);
    tokio::spawn(async move {
        deliver(&hub, job).await;
        drop(slot);
    });
    true
}

fn backoff(hub: &Hub, attempt: u32, retry_after: Option<Duration>) -> Duration {
    let doubled = hub
        .config()
        .push_backoff
        .saturating_mul(2u32.saturating_pow(attempt));
    doubled
        .max(retry_after.unwrap_or_default())
        .min(LONGEST_BACKOFF)
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
        .map(Duration::from_secs)
}

async fn deliver(hub: &Hub, job: PushJob) {
    if let Err(reason) = check_endpoint(&job.endpoint, &hub.config().push_allow_hosts) {
        log(
            hub,
            "push.refused",
            &job.device,
            Some(format!("reason={reason}")),
        );
        return;
    }
    let (Some(body), Some(audience)) = (encrypt(&job.keys, &job.payload), audience(&job.endpoint))
    else {
        log(hub, "push.failed", &job.device, None);
        return;
    };
    let retries = hub.config().push_retries;
    let mut attempt = 0;
    loop {
        let at = now();
        let authorization = hub.pusher.vapid.authorization(
            &audience,
            &hub.config().vapid_subject,
            at + VAPID_TOKEN_SECONDS,
        );
        let mut request = hub
            .pusher
            .http
            .post(&job.endpoint)
            .header("ttl", job.expires_at.saturating_sub(at).to_string())
            .header("urgency", job.urgency)
            .header(CONTENT_ENCODING, "aes128gcm")
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(AUTHORIZATION, authorization)
            .body(body.clone());
        if let Some(topic) = &job.topic {
            request = request.header("topic", topic);
        }
        let answer = {
            let _sending = hub
                .pusher
                .sending
                .acquire()
                .await
                .expect("the sending semaphore is never closed");
            match request.send().await {
                Ok(response) => Some((response.status().as_u16(), retry_after(&response))),
                Err(_) => None,
            }
        };
        let detail = |status: Option<u16>| {
            Some(status.map_or("status=unreachable".to_string(), |status| {
                format!("status={status}")
            }))
        };
        match answer {
            Some((200..=299, _)) => {
                log(
                    hub,
                    "push.delivered",
                    &job.device,
                    detail(answer.map(|a| a.0)),
                );
                return;
            }
            Some((404 | 410, _)) => {
                if let Err(error) = hub.store().drop_push_endpoint(&job.device, &job.endpoint) {
                    eprintln!("kebbie-hub: could not drop a gone subscription: {error}");
                }
                log(hub, "push.gone", &job.device, detail(answer.map(|a| a.0)));
                return;
            }
            Some((413, _)) => {
                log(hub, "push.too-large", &job.device, detail(Some(413)));
                return;
            }
            Some((429 | 500..=599, _)) | None if attempt < retries => {
                let wait = backoff(hub, attempt, answer.and_then(|a| a.1));
                attempt += 1;
                tokio::time::sleep(wait).await;
            }
            Some((429 | 500..=599, _)) | None => {
                log(
                    hub,
                    "push.dropped",
                    &job.device,
                    detail(answer.map(|a| a.0)),
                );
                return;
            }
            Some((status, _)) => {
                if matches!(status, 401 | 403) {
                    eprintln!(
                        "kebbie-hub: a push service refused a push with {status}; check --vapid-subject"
                    );
                }
                log(hub, "push.refused", &job.device, detail(Some(status)));
                return;
            }
        }
    }
}
