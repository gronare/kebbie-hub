#[allow(dead_code)]
mod common;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::*;
use kelpie_hub::webpush::{MAX_PUSH_BODY, Vapid, topic};
use kelpie_hub::{Config, Hub, OpenError, RateLimit};
use kelpie_hub_api::{NoticePosted, NoticeUpload, PushKeys, PushSubscription, VapidKey};
use kelpie_hub_fakes::machine::MachineState;
use kelpie_hub_fakes::phone::PhoneState;
use kelpie_hub_fakes::{Error, HubClient, HubError};
use kelpie_hub_protocol::{
    Event, Header, Id, Kind, Notice, Payload, Prefix, PublicKeys, Urgency, seal,
};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const SOON: Duration = Duration::from_secs(5);
const SUBJECT: &str = "mailto:hub-admin@example.com";
const ADDRESS: &str = "refunds@greenhouse";

struct Received {
    path: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

#[derive(Clone)]
struct Script {
    answers: Arc<Mutex<HashMap<String, VecDeque<u16>>>>,
    received: mpsc::UnboundedSender<Received>,
}

struct PushService {
    url: String,
    script: Script,
    received: mpsc::UnboundedReceiver<Received>,
}

async fn receive(
    State(script): State<Script>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let path = uri.path().to_string();
    let status = script
        .answers
        .lock()
        .unwrap()
        .get_mut(&path)
        .and_then(VecDeque::pop_front)
        .unwrap_or(201);
    let _ = script.received.send(Received {
        path,
        headers,
        body: body.to_vec(),
    });
    StatusCode::from_u16(status).unwrap()
}

impl PushService {
    async fn start() -> PushService {
        let (sender, received) = mpsc::unbounded_channel();
        let script = Script {
            answers: Arc::new(Mutex::new(HashMap::new())),
            received: sender,
        };
        let app = Router::new().fallback(receive).with_state(script.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        PushService {
            url,
            script,
            received,
        }
    }

    fn endpoint(&self, name: &str) -> String {
        format!("{}/push/{name}", self.url)
    }

    fn answer(&self, name: &str, statuses: &[u16]) {
        self.script
            .answers
            .lock()
            .unwrap()
            .insert(format!("/push/{name}"), statuses.iter().copied().collect());
    }

    async fn next(&mut self) -> Received {
        tokio::time::timeout(SOON, self.received.recv())
            .await
            .expect("a push arrives")
            .unwrap()
    }

    fn drain(&mut self) -> Vec<Received> {
        let mut all = Vec::new();
        while let Ok(received) = self.received.try_recv() {
            all.push(received);
        }
        all
    }
}

async fn push_hub(configure: impl FnOnce(&mut Config)) -> TestHub {
    start_hub(|config| {
        config.vapid_subject = SUBJECT.to_string();
        config.push_allow_hosts = vec!["127.0.0.1".to_string()];
        config.push_backoff = Duration::from_millis(20);
        configure(config);
    })
    .await
}

async fn settle(test: &TestHub) {
    let deadline = tokio::time::Instant::now() + SOON;
    while test.hub.pushes_in_flight() > 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "pushes still in flight"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn paired_phone(test: &TestHub, machine: &mut MachineState) -> PhoneState {
    let mut phone = phone(test).await;
    pair_both(machine, &mut phone, true).await.0.unwrap();
    phone
}

async fn subscribed_phone(
    test: &TestHub,
    machine: &mut MachineState,
    service: &PushService,
    name: &str,
) -> PhoneState {
    let mut phone = paired_phone(test, machine).await;
    phone.subscribe_push(&service.endpoint(name)).await.unwrap();
    phone
}

fn notice(body: &str) -> Notice {
    Notice {
        event: Event::Question,
        address: Some(ADDRESS.to_string()),
        title: "refunds asks".to_string(),
        body: body.to_string(),
        question: Some("1761069540-4411-0-question".to_string()),
        options: vec!["Keep it".to_string(), "Remove it".to_string()],
        recommend: Some(0),
        board_rev: 1,
        retract: None,
    }
}

fn device(phone: &PhoneState) -> Id {
    phone.device.clone().unwrap()
}

fn header<'a>(received: &'a Received, name: &str) -> Option<&'a str> {
    received
        .headers
        .get(name)
        .map(|value| value.to_str().unwrap())
}

fn hub_refusal<T: std::fmt::Debug>(result: Result<T, Error>) -> (u16, String) {
    match result {
        Err(Error::Hub(HubError::Status { status, code })) => (status, code),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

fn verify_vapid(authorization: &str, published_key: &str) -> Value {
    let rest = authorization
        .strip_prefix("vapid ")
        .expect("a vapid scheme");
    let (token, key) = rest.split_once(", ").unwrap();
    let token = token.strip_prefix("t=").unwrap();
    let key = key.strip_prefix("k=").unwrap();
    assert_eq!(key, published_key);
    let (signing_input, signature) = token.rsplit_once('.').unwrap();
    let (header, claims) = signing_input.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
    assert_eq!(header["alg"], "ES256");
    assert_eq!(header["typ"], "JWT");
    let verifying = VerifyingKey::from_sec1_bytes(&URL_SAFE_NO_PAD.decode(key).unwrap()).unwrap();
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
    verifying
        .verify(signing_input.as_bytes(), &signature)
        .expect("the VAPID token is signed by the published key");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap()
}

#[tokio::test]
async fn a_notice_reaches_each_subscribed_device_as_its_own_cut_of_the_sealed_envelope() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let first = subscribed_phone(&test, &mut machine, &service, "first").await;
    let second = subscribed_phone(&test, &mut machine, &service, "second").await;
    let collapse = machine.collapse_tag(Event::Question, ADDRESS);
    let envelope = machine
        .seal_notice(
            &notice(&format!("{MARKER} body")),
            Urgency::High,
            Some(collapse.clone()),
            kelpie_hub::now(),
        )
        .unwrap();

    let posted = machine.post_notice(envelope.clone()).await.unwrap();
    assert_eq!(posted.notice, envelope.header.id);
    assert_eq!(posted.pushes, 2);

    let published: VapidKey = HubClient::new(&test.url, None)
        .get("/v1/push/vapid", &[])
        .await
        .unwrap();
    let mut pushes: Vec<Received> = vec![service.next().await, service.next().await];
    pushes.sort_by(|a, b| a.path.cmp(&b.path));
    let now = kelpie_hub::now();
    for (push, phone, other) in [(&pushes[0], &first, &second), (&pushes[1], &second, &first)] {
        let (opened, payload) = phone.open_push(&push.body).unwrap();
        assert_eq!(Some(opened.clone()), envelope.for_recipient(&device(phone)));
        assert_eq!(
            opened.keys.keys().cloned().collect::<Vec<_>>(),
            vec![device(phone)]
        );
        assert_eq!(payload.body, format!("{MARKER} body"));
        assert!(other.open_push(&push.body).is_err());
        assert!(push.body.len() <= MAX_PUSH_BODY);
        assert!(!holds(&push.body, MARKER.as_bytes()));

        assert_eq!(header(push, "content-encoding"), Some("aes128gcm"));
        assert_eq!(header(push, "urgency"), Some("high"));
        let ttl: u64 = header(push, "ttl").unwrap().parse().unwrap();
        let left = envelope.header.exp - now;
        assert!(ttl <= left && ttl + 5 >= left, "ttl {ttl}, {left} left");
        let topic_header = header(push, "topic").unwrap();
        assert_eq!(collapse.len(), 43);
        assert_eq!(topic_header, topic(&collapse));
        assert_eq!(topic_header.len(), 32);

        let claims = verify_vapid(header(push, "authorization").unwrap(), &published.key);
        assert_eq!(claims["aud"], service.url.as_str());
        assert_eq!(claims["sub"], SUBJECT);
        let exp = claims["exp"].as_u64().unwrap();
        assert!(exp > now && exp <= now + 86_400, "exp {exp}");
    }
    assert_eq!(pushes[0].path, "/push/first");
    assert_eq!(pushes[1].path, "/push/second");
}

#[tokio::test]
async fn a_normal_notice_without_collapse_has_no_topic() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _phone = subscribed_phone(&test, &mut machine, &service, "one").await;
    let short = "short_tag".to_string();
    machine
        .notice(&notice("plain"), Urgency::Normal, None)
        .await
        .unwrap();
    let push = service.next().await;
    assert_eq!(header(&push, "urgency"), Some("normal"));
    assert_eq!(header(&push, "topic"), None);
    machine
        .notice(&notice("tagged"), Urgency::Normal, Some(short.clone()))
        .await
        .unwrap();
    assert_eq!(header(&service.next().await, "topic"), Some(short.as_str()));
}

#[tokio::test]
async fn notices_skip_unsubscribed_and_revoked_devices() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _subscribed = subscribed_phone(&test, &mut machine, &service, "subscribed").await;
    let _never = paired_phone(&test, &mut machine).await;
    let mut unsubscribed = subscribed_phone(&test, &mut machine, &service, "unsubscribed").await;
    unsubscribed.unsubscribe_push().await.unwrap();
    let revoked = subscribed_phone(&test, &mut machine, &service, "revoked").await;
    let envelope = machine
        .seal_notice(
            &notice("who hears this"),
            Urgency::High,
            None,
            kelpie_hub::now(),
        )
        .unwrap();
    assert_eq!(envelope.keys.len(), 4);
    machine.remove_device(&device(&revoked)).await.unwrap();

    let posted = machine.post_notice(envelope).await.unwrap();
    assert_eq!(posted.pushes, 1);
    assert_eq!(service.next().await.path, "/push/subscribed");
    settle(&test).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(service.drain().is_empty());
    assert_eq!(rows(&test, "push_subscriptions"), 2);
}

#[tokio::test]
async fn a_push_service_that_answers_gone_prunes_the_subscription() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _gone = subscribed_phone(&test, &mut machine, &service, "gone").await;
    let _missing = subscribed_phone(&test, &mut machine, &service, "missing").await;
    service.answer("gone", &[410]);
    service.answer("missing", &[404]);

    let posted = machine
        .notice(&notice("anyone there"), Urgency::High, None)
        .await
        .unwrap();
    assert_eq!(posted.pushes, 2);
    service.next().await;
    service.next().await;
    settle(&test).await;
    assert_eq!(rows(&test, "push_subscriptions"), 0);
    let again = machine
        .notice(&notice("still there"), Urgency::High, None)
        .await
        .unwrap();
    assert_eq!(again.pushes, 0);
    let kinds: Vec<String> = test
        .hub
        .store()
        .events()
        .unwrap()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    assert_eq!(kinds.iter().filter(|kind| *kind == "push.gone").count(), 2);
}

#[tokio::test]
async fn a_rate_limited_push_is_retried_until_the_service_takes_it() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let phone = subscribed_phone(&test, &mut machine, &service, "busy").await;
    service.answer("busy", &[429, 503, 201]);

    machine
        .notice(&notice(&format!("{MARKER} retried")), Urgency::High, None)
        .await
        .unwrap();
    let attempts = [
        service.next().await,
        service.next().await,
        service.next().await,
    ];
    for attempt in &attempts {
        let (_, payload) = phone.open_push(&attempt.body).unwrap();
        assert_eq!(payload.body, format!("{MARKER} retried"));
    }
    settle(&test).await;
    assert!(service.drain().is_empty());
    assert_eq!(rows(&test, "push_subscriptions"), 1);
    let delivered = test
        .hub
        .store()
        .events()
        .unwrap()
        .into_iter()
        .filter(|(kind, _)| kind == "push.delivered")
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].1.as_deref(), Some("status=201"));
}

#[tokio::test]
async fn a_push_the_service_keeps_refusing_is_dropped_after_its_retries() {
    let test = push_hub(|config| config.push_retries = 2).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _phone = subscribed_phone(&test, &mut machine, &service, "down").await;
    service.answer("down", &[500, 502, 429, 429]);

    machine
        .notice(&notice("never lands"), Urgency::High, None)
        .await
        .unwrap();
    settle(&test).await;
    assert_eq!(service.drain().len(), 3);
    assert_eq!(rows(&test, "push_subscriptions"), 1);
    let events = test.hub.store().events().unwrap();
    assert!(events.iter().any(|(kind, detail)| kind == "push.dropped"
        && detail.as_deref() == Some("status=429")));
}

#[tokio::test]
async fn a_notice_too_large_for_one_push_is_refused_when_posted() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _phone = subscribed_phone(&test, &mut machine, &service, "small").await;

    let (status, code) = hub_refusal(
        machine
            .notice(&notice(&"x".repeat(2_900)), Urgency::High, None)
            .await,
    );
    assert_eq!((status, code.as_str()), (413, "too-large"));
    let fits = machine
        .notice(&notice(&"x".repeat(2_000)), Urgency::High, None)
        .await
        .unwrap();
    assert_eq!(fits.pushes, 1);
    assert!(service.next().await.body.len() <= MAX_PUSH_BODY);
}

#[tokio::test]
async fn notices_are_checked_and_limited_before_anything_is_pushed() {
    let test = push_hub(|config| {
        config.notice_rate = RateLimit {
            burst: 4,
            per_minute: 1,
        };
        config.device_push_rate = RateLimit {
            burst: 1,
            per_minute: 1,
        };
    })
    .await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _phone = subscribed_phone(&test, &mut machine, &service, "capped").await;

    let board = machine
        .seal_board(board(MARKER), 1, kelpie_hub::now())
        .unwrap();
    let (status, code) = hub_refusal(machine.post_notice(board).await);
    assert_eq!((status, code.as_str()), (400, "not-a-notice"));

    let envelope = machine
        .seal_notice(&notice("once"), Urgency::High, None, kelpie_hub::now())
        .unwrap();
    let mut forged = envelope.clone();
    forged.header.urgency = Some(Urgency::Normal);
    let (status, code) = hub_refusal(machine.post_notice(forged).await);
    assert_eq!((status, code.as_str()), (400, "bad-signature"));

    assert_eq!(
        machine.post_notice(envelope.clone()).await.unwrap().pushes,
        1
    );
    let (status, code) = hub_refusal(machine.post_notice(envelope).await);
    assert_eq!((status, code.as_str()), (409, "replayed-envelope"));
    service.next().await;

    let capped = machine
        .notice(&notice("over the device cap"), Urgency::High, None)
        .await
        .unwrap();
    assert_eq!(capped.pushes, 0);
    machine
        .notice(&notice("third"), Urgency::High, None)
        .await
        .unwrap();
    let (status, code) = hub_refusal(
        machine
            .notice(&notice("over the machine rate"), Urgency::High, None)
            .await,
    );
    assert_eq!((status, code.as_str()), (429, "rate-limited"));
    settle(&test).await;
    assert!(service.drain().is_empty());
    let events = test.hub.store().events().unwrap();
    assert!(events.iter().any(|(kind, _)| kind == "push.capped"));
}

#[tokio::test]
async fn push_subscriptions_must_point_at_a_known_push_service() {
    let test = start_hub(|config| config.vapid_subject = SUBJECT.to_string()).await;
    let mut machine = joined_machine(&test).await;
    let phone = paired_phone(&test, &mut machine).await;
    let secret = kelpie_hub_fakes::push::PushSecret::generate("https://fcm.googleapis.com/fcm/x");
    let good = secret.subscription();
    let with_endpoint = |endpoint: &str| PushSubscription {
        endpoint: endpoint.to_string(),
        keys: good.keys.clone(),
    };
    let client = phone.client();
    for (subscription, expected) in [
        (
            with_endpoint("https://push.attacker.example.com/x"),
            "endpoint-not-allowed",
        ),
        (
            with_endpoint("http://fcm.googleapis.com/fcm/send/x"),
            "endpoint-not-https",
        ),
        (
            with_endpoint("http://127.0.0.1:9/push"),
            "endpoint-not-allowed",
        ),
        (
            PushSubscription {
                endpoint: good.endpoint.clone(),
                keys: PushKeys {
                    p256dh: URL_SAFE_NO_PAD.encode([4u8; 65]),
                    auth: good.keys.auth.clone(),
                },
            },
            "bad-push-keys",
        ),
    ] {
        let refused = client.put_empty("/v1/d/push", &subscription).await;
        assert_eq!(
            refused,
            Err(HubError::Status {
                status: 400,
                code: expected.to_string()
            }),
            "{}",
            subscription.endpoint
        );
    }
    assert_eq!(rows(&test, "push_subscriptions"), 0);
    client.put_empty("/v1/d/push", &good).await.unwrap();
    for endpoint in [
        "https://web.push.apple.com/QGuQyavXutnMH",
        "https://wns2-par02p.notify.windows.com/w/?token=AwYAAA",
    ] {
        client
            .put_empty("/v1/d/push", &with_endpoint(endpoint))
            .await
            .unwrap();
    }
    assert_eq!(rows(&test, "push_subscriptions"), 1);
    let anonymous = HubClient::new(&test.url, None)
        .put_empty("/v1/d/push", &good)
        .await;
    assert_eq!(anonymous.unwrap_err().status(), Some(401));
}

#[tokio::test]
async fn nothing_decryptable_is_stored_and_the_log_carries_no_content() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let phone = subscribed_phone(&test, &mut machine, &service, "private").await;
    let collapse = machine.collapse_tag(Event::Question, ADDRESS);
    let envelope = machine
        .seal_notice(
            &notice(&format!("{MARKER} secret")),
            Urgency::High,
            Some(collapse.clone()),
            kelpie_hub::now(),
        )
        .unwrap();
    machine.post_notice(envelope.clone()).await.unwrap();
    let push = service.next().await;
    phone.open_push(&push.body).unwrap();
    settle(&test).await;

    let stored = database_bytes(&test);
    assert!(!holds(&stored, MARKER.as_bytes()));
    assert!(!holds(&stored, envelope.ct.as_bytes()));
    assert!(!holds(&stored, &envelope.ct.as_bytes()[..32]));
    assert!(!holds(&stored, &push.body[86..118]));
    assert!(!holds(&stored, collapse.as_bytes()));
    assert!(!holds(&stored, topic(&collapse).as_bytes()));
    for (kind, detail) in test.hub.store().events().unwrap() {
        let detail = detail.unwrap_or_default();
        assert!(!detail.contains(MARKER), "{kind}: {detail}");
        assert!(!detail.contains(&collapse), "{kind}: {detail}");
        assert!(!detail.contains("/push/"), "{kind}: {detail}");
    }
}

#[tokio::test]
async fn the_vapid_key_is_published_and_kept_across_restarts() {
    let test = push_hub(|_| {}).await;
    let published: VapidKey = HubClient::new(&test.url, None)
        .get("/v1/push/vapid", &[])
        .await
        .unwrap();
    let raw = URL_SAFE_NO_PAD.decode(&published.key).unwrap();
    assert_eq!((raw.len(), raw[0]), (65, 4));
    assert_eq!(published.key, test.hub.vapid_public_key());
    let path = &test.hub.config().vapid_key;
    let reloaded = Vapid::load_or_create(path).unwrap();
    assert_eq!(reloaded.public_key(), published.key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let secret = std::fs::read_to_string(path).unwrap();
    let database = std::fs::read(test.hub.config().database()).unwrap();
    assert!(!holds(&database, secret.trim().as_bytes()));
}

#[tokio::test]
async fn a_hub_refuses_a_vapid_subject_push_services_reject() {
    let data = tempfile::tempdir().unwrap();
    for subject in ["mailto:admin@herdr.local", "mailto:hub@localhost", "kelpie"] {
        let mut config = Config::new(data.path());
        config.vapid_subject = subject.to_string();
        assert!(
            matches!(Hub::open(config), Err(OpenError::Push(_))),
            "{subject}"
        );
    }
}

#[tokio::test]
async fn posting_a_notice_needs_the_machine_token() {
    let test = push_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let _phone = paired_phone(&test, &mut machine).await;
    let envelope = machine
        .seal_notice(&notice("x"), Urgency::High, None, kelpie_hub::now())
        .unwrap();
    let anonymous: Result<NoticePosted, HubError> = HubClient::new(&test.url, None)
        .post("/v1/m/notices", &NoticeUpload { envelope })
        .await;
    assert_eq!(anonymous.unwrap_err().status(), Some(401));
}

#[tokio::test]
async fn a_failing_push_service_does_not_starve_deliveries_to_another() {
    let test = push_hub(|config| {
        config.push_in_flight = 1;
        config.push_backoff = Duration::from_millis(400);
    })
    .await;
    let mut down = PushService::start().await;
    let mut up = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _failing = subscribed_phone(&test, &mut machine, &down, "failing").await;
    let _healthy = subscribed_phone(&test, &mut machine, &up, "healthy").await;
    down.answer("failing", &[503; 8]);

    let posted = machine
        .notice(&notice("both"), Urgency::High, None)
        .await
        .unwrap();
    assert_eq!(posted.pushes, 2);
    let healthy = tokio::time::timeout(Duration::from_millis(300), up.received.recv())
        .await
        .expect("the healthy service is not kept waiting behind retries")
        .unwrap();
    assert_eq!(healthy.path, "/push/healthy");
    assert_eq!(down.next().await.path, "/push/failing");
}

#[tokio::test]
async fn an_endpoint_moves_only_between_devices_of_one_account() {
    let test = push_hub(|_| {}).await;
    let service = PushService::start().await;
    let person = signed_up(&test).await;
    let first = person.register_device().await;
    let second = person.register_device().await;
    let stranger = signed_up(&test).await.register_device().await;
    let subscription =
        kelpie_hub_fakes::push::PushSecret::generate(&service.endpoint("shared")).subscription();

    device_client(&test, &first)
        .put_empty("/v1/d/push", &subscription)
        .await
        .unwrap();
    device_client(&test, &second)
        .put_empty("/v1/d/push", &subscription)
        .await
        .unwrap();
    assert_eq!(rows(&test, "push_subscriptions"), 1);
    let held_by = |device: &Id| test.hub.store().push_subscription(device).unwrap();
    assert!(held_by(&first.device).is_none());
    assert!(held_by(&second.device).is_some());

    let refused = device_client(&test, &stranger)
        .put_empty("/v1/d/push", &subscription)
        .await;
    assert_eq!(
        refused,
        Err(HubError::Status {
            status: 409,
            code: "endpoint-taken".to_string()
        })
    );
    assert!(held_by(&second.device).is_some());
    assert!(held_by(&stranger.device).is_none());
    let events = test.hub.store().events().unwrap();
    assert!(events.iter().any(|(kind, _)| kind == "push.moved"));
    assert!(events.iter().any(|(kind, _)| kind == "push.endpoint-taken"));
}

#[tokio::test]
async fn a_stored_endpoint_off_the_allowlist_is_refused_at_delivery() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let phone = subscribed_phone(&test, &mut machine, &service, "moved-off").await;
    rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .execute(
            "UPDATE push_subscriptions SET endpoint = ?1 WHERE device_id = ?2",
            [
                "https://push.attacker.example.com/x",
                device(&phone).as_str(),
            ],
        )
        .unwrap();

    machine
        .notice(&notice("not there"), Urgency::High, None)
        .await
        .unwrap();
    settle(&test).await;
    assert!(service.drain().is_empty());
    let events = test.hub.store().events().unwrap();
    assert!(events.iter().any(|(kind, detail)| kind == "push.refused"
        && detail.as_deref() == Some("reason=endpoint-not-allowed")));
}

#[cfg(unix)]
#[tokio::test]
async fn a_hub_refuses_a_vapid_key_others_can_read() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let mut config = Config::new(data.path());
    config.vapid_subject = SUBJECT.to_string();
    Vapid::load_or_create(&config.vapid_key).unwrap();
    std::fs::set_permissions(&config.vapid_key, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Hub::open(config.clone()),
        Err(OpenError::Push(message)) if message.contains("group or others")
    ));
    std::fs::set_permissions(&config.vapid_key, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Hub::open(config).is_ok());
}

#[tokio::test]
async fn a_redirect_from_a_push_service_is_not_followed() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let mut machine = joined_machine(&test).await;
    let _phone = subscribed_phone(&test, &mut machine, &service, "redirects").await;
    service.answer("redirects", &[302]);

    machine
        .notice(&notice("stay put"), Urgency::High, None)
        .await
        .unwrap();
    assert_eq!(service.next().await.path, "/push/redirects");
    settle(&test).await;
    assert!(service.drain().is_empty());
    let events = test.hub.store().events().unwrap();
    assert!(events.iter().any(
        |(kind, detail)| kind == "push.refused" && detail.as_deref() == Some("status=302")
    ));
}

#[tokio::test]
async fn notices_to_the_wrong_recipients_or_from_another_machine_are_refused() {
    let test = push_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let phone = paired_phone(&test, &mut machine).await;
    let other = joined_machine(&test).await;
    let sealed = |recipients: &[(Id, PublicKeys)]| {
        let mut header = Header::new(
            Id::new(Prefix::Notice).unwrap(),
            Kind::Notice,
            machine.machine.clone(),
            machine.machine.clone(),
            kelpie_hub::now(),
        );
        header.urgency = Some(Urgency::High);
        seal(
            header,
            &Payload::Notice(notice("misaddressed")),
            &machine.secret(),
            recipients,
        )
        .unwrap()
    };
    let to_self = sealed(&[(machine.machine.clone(), machine.public())]);
    let to_another_machine = sealed(&[
        (device(&phone), phone.public()),
        (other.machine.clone(), other.public()),
    ]);
    for envelope in [to_self, to_another_machine] {
        let (status, code) = hub_refusal(machine.post_notice(envelope).await);
        assert_eq!((status, code.as_str()), (400, "bad-recipients"));
    }
    let good = machine
        .seal_notice(&notice("mine"), Urgency::High, None, kelpie_hub::now())
        .unwrap();
    let (status, code) = hub_refusal(other.post_notice(good).await);
    assert_eq!((status, code.as_str()), (400, "wrong-machine"));
    assert_eq!(rows(&test, "notices"), 0);
}

#[tokio::test]
async fn posted_notice_ids_are_swept_once_they_expire() {
    let test = push_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let _phone = paired_phone(&test, &mut machine).await;
    let posted = machine
        .notice(&notice("kept a day"), Urgency::Normal, None)
        .await
        .unwrap();
    assert_eq!(posted.pushes, 0);
    assert_eq!(rows(&test, "notices"), 1);
    test.hub.store().sweep(kelpie_hub::now() + 3_600).unwrap();
    assert_eq!(rows(&test, "notices"), 1);
    test.hub.store().sweep(kelpie_hub::now() + 86_401).unwrap();
    assert_eq!(rows(&test, "notices"), 0);
}
