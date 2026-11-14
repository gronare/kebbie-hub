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
use kebbie_hub::webpush::{MAX_PUSH_BODY, topic};
use kebbie_hub::{Config, Hub, OpenError, RateLimit};
use kebbie_hub_api::{HubInfo, NoticePosted, NoticeUpload, NotifiedDevices, PushKeys, PushSubscribe};
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{ApproveFrom, Handling, MachineState};
use kebbie_hub_fakes::push::PushSecret;
use kebbie_hub_fakes::{HubClient, HubError};
use kebbie_hub_protocol::{
    AccountKey, Answer, DeviceCert, DeviceDecision, DeviceKey, DeviceKind, DeviceStanding, Envelope, Header, Id, Kind,
    Notice, NoticeCategory, Outcome, Payload, Prefix, Reason, SecretKeys, Target, Urgency, seal,
};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const SOON: Duration = Duration::from_secs(5);
const SUBJECT: &str = "mailto:hub-admin@example.com";
const ADDRESS: &str = "refunds@shop";

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

async fn receive(State(script): State<Script>, uri: Uri, headers: HeaderMap, body: Bytes) -> StatusCode {
    let path = uri.path().to_string();
    let status = script.answers.lock().unwrap().get_mut(&path).and_then(VecDeque::pop_front).unwrap_or(201);
    let _ = script.received.send(Received { path, headers, body: body.to_vec() });
    StatusCode::from_u16(status).unwrap()
}

impl PushService {
    async fn start() -> PushService {
        let (sender, received) = mpsc::unbounded_channel();
        let script = Script { answers: Arc::new(Mutex::new(HashMap::new())), received: sender };
        let app = Router::new().fallback(receive).with_state(script.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        PushService { url, script, received }
    }

    fn endpoint(&self, name: &str) -> String {
        format!("{}/push/{name}", self.url)
    }

    fn answer(&self, name: &str, statuses: &[u16]) {
        self.script.answers.lock().unwrap().insert(format!("/push/{name}"), statuses.iter().copied().collect());
    }

    async fn next(&mut self) -> Received {
        tokio::time::timeout(SOON, self.received.recv()).await.expect("a push arrives").unwrap()
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
        assert!(tokio::time::Instant::now() < deadline, "pushes still in flight");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn notifying(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    (machine, person)
}

fn account_of(person: &Browser) -> AccountKey {
    person.account_key().expect("the account is unlocked").clone()
}

fn another_device(person: &Browser, name: &str) -> (DeviceKey, DeviceCert) {
    let key = DeviceKey::generate();
    let (signing, encryption) = person.account_secret().unwrap();
    let cert = DeviceCert::issue(
        &SecretKeys::from_bytes(signing, encryption),
        &account_of(person).account,
        Id::new(Prefix::Device).unwrap(),
        name,
        DeviceKind::Web,
        key.public(),
        kebbie_hub::now(),
    )
    .unwrap();
    (key, cert)
}

async fn subscribed(person: &mut Browser, service: &PushService, name: &str, categories: &[NoticeCategory]) -> PushSecret {
    person.register_device().await.unwrap();
    let secret = PushSecret::generate(&service.endpoint(name));
    person.subscribe_push(&secret.subscription(categories)).await.unwrap();
    secret
}

fn notice(category: NoticeCategory, body: &str) -> Notice {
    Notice {
        category,
        title: "refunds · shop".to_string(),
        body: body.to_string(),
        open: Target::Task { address: ADDRESS.to_string() },
        retract: Vec::new(),
    }
}

fn header<'a>(received: &'a Received, name: &str) -> Option<&'a str> {
    received.headers.get(name).map(|value| value.to_str().unwrap())
}

fn refusal_of<T: std::fmt::Debug>(result: Result<T, HubError>) -> (u16, String) {
    match result {
        Err(error) => hub_code(error),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

fn verify_vapid(authorization: &str, published_key: &str) -> Value {
    let rest = authorization.strip_prefix("vapid ").expect("a vapid scheme");
    let (token, key) = rest.split_once(", ").unwrap();
    let token = token.strip_prefix("t=").unwrap();
    let key = key.strip_prefix("k=").unwrap();
    assert_eq!(key, published_key);
    let (signing_input, signature) = token.rsplit_once('.').unwrap();
    let (header, claims) = signing_input.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
    assert_eq!((header["alg"].as_str(), header["typ"].as_str()), (Some("ES256"), Some("JWT")));
    let verifying = VerifyingKey::from_sec1_bytes(&URL_SAFE_NO_PAD.decode(key).unwrap()).unwrap();
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
    verifying.verify(signing_input.as_bytes(), &signature).expect("the VAPID token is signed by the published key");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap()
}

async fn post(machine: &MachineState, envelope: Envelope) -> Result<NoticePosted, HubError> {
    machine.client().post("/v1/m/notices", &NoticeUpload { envelope }).await
}

fn opened_push(secret: &PushSecret, person: &Browser, push: &Received) -> Result<(Envelope, Notice), String> {
    let plaintext = secret.decrypt(&push.body).map_err(str::to_string)?;
    let envelope = Envelope::from_json(std::str::from_utf8(&plaintext).unwrap()).map_err(|error| error.to_string())?;
    let notice = person.open_notice(&envelope, kebbie_hub::now())?;
    Ok((envelope, notice))
}

#[tokio::test]
async fn a_notice_reaches_each_subscribed_device_as_its_own_cut_sealed_end_to_end() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    let phone = (person.device_key().clone(), person.device_cert().unwrap().clone());
    let phone_secret = subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    let tablet = another_device(&person, "iPad");
    person.use_device(tablet.0.clone(), tablet.1.clone());
    let tablet_secret = subscribed(&mut person, &service, "tablet", &NoticeCategory::defaults()).await;

    let collapse = "q-refunds-1761069540-4411-0-question-to-hash".to_string();
    let sent = machine.notify(notice(NoticeCategory::Questions, &format!("{MARKER} body")), &collapse, Urgency::High).await.unwrap();
    let envelope = sent.envelope.unwrap();
    assert_eq!((sent.recipients.len(), sent.pushes), (2, 2));

    let info: HubInfo = HubClient::new(&test.url, None).get("/v1/hub", &[]).await.unwrap();
    let published = info.vapid.expect("the hub publishes its VAPID key");
    let mut pushes = [service.next().await, service.next().await];
    pushes.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!((pushes[0].path.as_str(), pushes[1].path.as_str()), ("/push/phone", "/push/tablet"));
    let now = kebbie_hub::now();
    for (push, secret, other, (key, cert)) in [(&pushes[0], &phone_secret, &tablet_secret, &phone), (&pushes[1], &tablet_secret, &phone_secret, &tablet)] {
        person.use_device(key.clone(), cert.clone());
        let (opened, payload) = opened_push(secret, &person, push).unwrap();
        assert_eq!(Some(opened.clone()), envelope.for_recipient(&cert.device));
        assert_eq!(opened.keys.keys().cloned().collect::<Vec<_>>(), vec![cert.device.clone()]);
        assert_eq!(payload.body, format!("{MARKER} body"));
        assert!(other.decrypt(&push.body).is_err());
        assert!(push.body.len() <= MAX_PUSH_BODY);
        assert!(!holds(&push.body, MARKER.as_bytes()));
        assert_eq!(header(push, "content-encoding"), Some("aes128gcm"));
        assert_eq!(header(push, "urgency"), Some("high"));
        let ttl: u64 = header(push, "ttl").unwrap().parse().unwrap();
        let left = envelope.header.exp - now;
        assert!(ttl >= left && ttl <= left + 5, "ttl {ttl}, {left} left");
        assert_eq!(header(push, "topic").unwrap(), topic(&collapse));
        assert_eq!(header(push, "topic").unwrap().len(), 32);
        let claims = verify_vapid(header(push, "authorization").unwrap(), &published);
        assert_eq!((claims["aud"].as_str(), claims["sub"].as_str()), (Some(service.url.as_str()), Some(SUBJECT)));
        let exp = claims["exp"].as_u64().unwrap();
        assert!(exp > now && exp <= now + 86_400, "exp {exp}");
    }
    let other_device = another_device(&person, "Laptop");
    person.use_device(other_device.0, other_device.1);
    assert!(opened_push(&phone_secret, &person, &pushes[0]).is_err(), "a device the notice was not sealed to cannot open it");
}

#[tokio::test]
async fn machines_see_the_accounts_subscribed_devices_with_the_categories_each_chose() {
    let test = push_hub(|_| {}).await;
    let service = PushService::start().await;
    let (machine, mut person) = notifying(&test).await;
    let listed = |machine: &MachineState| {
        let machine = machine.clone();
        async move { machine.notified_devices().await.unwrap().devices }
    };
    person.register_device().await.unwrap();
    let unsubscribed = listed(&machine).await;
    assert_eq!(unsubscribed.len(), 1);
    assert_eq!(unsubscribed[0].notifications, None, "a device without a subscription is listed without notifications");
    let json = serde_json::to_value(&unsubscribed[0]).unwrap();
    assert!(json["notifications"].is_null() && json["cert"]["encryption"].is_string(), "{json}");

    let secret = subscribed(&mut person, &service, "phone", &[NoticeCategory::Started, NoticeCategory::Questions, NoticeCategory::Questions]).await;
    let devices = listed(&machine).await;
    assert_eq!(devices.len(), 1);
    assert_eq!(&devices[0].cert, person.device_cert().unwrap());
    assert_eq!(devices[0].notifications.as_deref(), Some([NoticeCategory::Questions, NoticeCategory::Started].as_slice()));
    let view = person.devices().await.unwrap().devices.pop().unwrap();
    assert_eq!(view.push.unwrap().categories, [NoticeCategory::Questions, NoticeCategory::Started]);

    person.set_categories(&[NoticeCategory::Ready]).await.unwrap();
    assert_eq!(listed(&machine).await[0].notifications.as_deref(), Some([NoticeCategory::Ready].as_slice()));

    let mut stranger = member(&test).await;
    let theirs = connected_with_board(&test, &mut stranger).await;
    assert!(listed(&theirs).await.is_empty(), "another account's machine sees none of them");
    let path = format!("/v1/a/devices/{}/push", person.device_id());
    assert_eq!(refusal_of(stranger.put::<_, Option<()>>(&path, &secret.subscription(&[])).await), (404, "unknown-device".to_string()));
    assert_eq!(refusal_of(stranger.delete(&path).await), (404, "unknown-device".to_string()));

    person.unsubscribe_push().await.unwrap();
    assert_eq!(listed(&machine).await[0].notifications, None);
    assert!(person.devices().await.unwrap().devices[0].push.is_none());
    assert_eq!(refusal_of(person.set_categories(&[NoticeCategory::Ready]).await), (409, "not-subscribed".to_string()));
    let unknown_category = serde_json::json!({ "categories": ["summary"] });
    let categories_path = format!("/v1/a/devices/{}/categories", person.device_id());
    assert_eq!(refusal_of(person.put::<_, Option<()>>(&categories_path, &unknown_category).await).0, 422);
}

#[tokio::test]
async fn a_machine_seals_only_to_devices_it_approved_that_want_the_category() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    let phone = (person.device_key().clone(), person.device_cert().unwrap().clone());
    subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    let tablet = another_device(&person, "iPad");
    person.use_device(tablet.0.clone(), tablet.1.clone());
    subscribed(&mut person, &service, "tablet", &[NoticeCategory::Ready]).await;

    let started = machine.notify(notice(NoticeCategory::Started, "Started invoice-rounding@shop."), "started-1", Urgency::Normal).await.unwrap();
    assert!(started.envelope.is_none(), "nobody wants a start going through");
    let ready = machine.notify(notice(NoticeCategory::Ready, "Ready to push."), "ready-1", Urgency::Normal).await.unwrap();
    assert_eq!(ready.recipients, [phone.1.device.clone(), tablet.1.device.clone()].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    settle(&test).await;
    service.drain();

    machine.trust_devices = true;
    let none = machine.notify(notice(NoticeCategory::Ready, "Ready to push."), "ready-2", Urgency::Normal).await.unwrap();
    assert!(none.envelope.is_none(), "with approvals on, a device the machine never approved gets nothing");
    let fingerprint = phone.1.fingerprint();
    machine.devices.insert(phone.1.device.clone(), kebbie_hub_fakes::machine::KnownDevice {
        name: "Test browser".into(),
        signing: phone.1.signing,
        standing: DeviceStanding::Approved { fingerprint, at: 1 },
    });
    let approved = machine.notify(notice(NoticeCategory::Questions, "Keep the old endpoint?"), "question-1", Urgency::High).await.unwrap();
    assert_eq!(approved.recipients, std::slice::from_ref(&phone.1.device));
    let push = service.next().await;
    assert_eq!(push.path, "/push/phone");
}

#[tokio::test]
async fn a_settled_subject_rides_on_the_next_notice_as_a_retract() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    let secret = subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    machine.notify(notice(NoticeCategory::Questions, "First question"), "question-1", Urgency::High).await.unwrap();
    let first = opened_push(&secret, &person, &service.next().await).unwrap();
    assert_eq!((first.0.header.collapse.as_deref(), first.1.retract.len()), (Some("question-1"), 0));

    machine.settle("question-1");
    machine.notify(notice(NoticeCategory::Questions, "Second question"), "question-2", Urgency::High).await.unwrap();
    let second = opened_push(&secret, &person, &service.next().await).unwrap();
    assert_eq!(second.1.retract, ["question-1"]);
    assert!(machine.retracts.is_empty(), "a retract that went out is not sent again");
    for tag in 0..12 {
        machine.settle(&format!("tag-{tag}"));
    }
    assert_eq!(machine.retracts.len(), kebbie_hub_protocol::MAX_RETRACTS);
}

#[tokio::test]
async fn a_push_service_that_answers_gone_drops_the_subscription() {
    for gone in [404, 410] {
        let test = push_hub(|_| {}).await;
        let mut service = PushService::start().await;
        let (mut machine, mut person) = notifying(&test).await;
        subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
        service.answer("phone", &[gone]);
        machine.notify(notice(NoticeCategory::Questions, "x"), "q", Urgency::High).await.unwrap();
        service.next().await;
        settle(&test).await;
        assert_eq!(rows(&test, "push_subscriptions"), 0, "{gone}");
        assert_eq!(machine.notified_devices().await.unwrap().devices[0].notifications, None);
        let events = test.hub.store().events().unwrap();
        assert!(events.iter().any(|(kind, _)| kind == "push.gone"), "{events:?}");
    }
}

#[tokio::test]
async fn a_push_the_service_rate_limits_is_retried_and_one_it_keeps_refusing_is_dropped() {
    let test = push_hub(|config| config.push_retries = 2).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    service.answer("phone", &[429, 503, 201]);
    machine.notify(notice(NoticeCategory::Questions, "x"), "q", Urgency::High).await.unwrap();
    for _ in 0..3 {
        service.next().await;
    }
    settle(&test).await;
    assert!(test.hub.store().events().unwrap().iter().any(|(kind, _)| kind == "push.delivered"));

    service.answer("phone", &[500, 500, 500, 500]);
    machine.notify(notice(NoticeCategory::Questions, "y"), "q", Urgency::High).await.unwrap();
    for _ in 0..3 {
        service.next().await;
    }
    settle(&test).await;
    assert!(service.drain().is_empty(), "no fourth attempt");
    assert!(test.hub.store().events().unwrap().iter().any(|(kind, _)| kind == "push.dropped"));
    assert_eq!(rows(&test, "push_subscriptions"), 1, "a failing service keeps the subscription");
}

#[tokio::test]
async fn notices_are_checked_before_anything_is_pushed() {
    let test = push_hub(|config| config.notice_rate = RateLimit { burst: 3, per_minute: 1 }).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    let cert = person.device_cert().unwrap().clone();
    let at = kebbie_hub::now();
    let sealed = |notice: &Notice, recipients: &[(Id, kebbie_hub_protocol::PublicKeys)]| {
        let mut header = Header::new(Id::new(Prefix::Notice).unwrap(), Kind::Notice, machine.machine.clone(), machine.machine.clone(), at);
        header.collapse = Some("q".into());
        seal(header, &Payload::Notice(notice.clone()), &machine.secret(), recipients).unwrap()
    };
    let fine = sealed(&notice(NoticeCategory::Questions, "x"), &[cert.recipient()]);

    let board = machine.seal_board(board(MARKER), 99, at).unwrap();
    assert_eq!(refusal_of(post(&machine, board).await), (400, "not-a-notice".to_string()));
    let to_the_account = sealed(&notice(NoticeCategory::Questions, "x"), &[cert.recipient(), account_of(&person).recipient()]);
    assert_eq!(refusal_of(post(&machine, to_the_account).await), (400, "bad-recipients".to_string()));
    let mut forged = fine.clone();
    forged.ct = URL_SAFE_NO_PAD.encode(b"another ciphertext entirely");
    assert_eq!(refusal_of(post(&machine, forged).await), (400, "bad-signature".to_string()));
    let stranger = joined_machine(&test).await;
    assert_eq!(refusal_of(post(&stranger, fine.clone()).await), (400, "wrong-machine".to_string()));

    let oversized = sealed(&notice(NoticeCategory::Questions, &"€".repeat(2 * kebbie_hub_protocol::MAX_NOTICE_BODY_CHARS)), &[cert.recipient()]);
    assert_eq!(refusal_of(post(&machine, oversized).await), (413, "too-large".to_string()));

    assert_eq!(post(&machine, fine.clone()).await.unwrap().pushes, 1);
    assert_eq!(refusal_of(post(&machine, fine).await), (409, "replayed-envelope".to_string()));
    service.next().await;
    let mut limited = None;
    for round in 0..5 {
        let result = post(&machine, sealed(&notice(NoticeCategory::Questions, &format!("{round}")), &[cert.recipient()])).await;
        if let Err(error) = result {
            limited = Some(hub_code(error));
            break;
        }
    }
    assert_eq!(limited.map(|(status, _)| status), Some(429));
    let without_token = HubClient::new(&test.url, None);
    let refused: Result<NoticePosted, HubError> = without_token.post("/v1/m/notices", &NoticeUpload { envelope: sealed(&notice(NoticeCategory::Questions, "z"), &[cert.recipient()]) }).await;
    assert_eq!(refusal_of(refused).0, 401);
    let _ = machine.notified_devices().await.unwrap();
    let refused_list: Result<NotifiedDevices, HubError> = without_token.get("/v1/m/devices", &[]).await;
    assert_eq!(refusal_of(refused_list).0, 401);
    let _ = &mut machine;
}

#[tokio::test]
async fn a_subscription_must_point_at_a_known_push_service_and_hold_real_keys() {
    let test = start_hub(|config| config.vapid_subject = SUBJECT.to_string()).await;
    let (_machine, mut person) = notifying(&test).await;
    person.register_device().await.unwrap();
    let good = PushSecret::generate("https://web.push.apple.com/QGuQyavXutnMH");
    person.subscribe_push(&good.subscription(&NoticeCategory::defaults())).await.unwrap();
    for (endpoint, refusal) in [
        ("http://127.0.0.1:9000/push", "endpoint-not-allowed"),
        ("http://fcm.googleapis.com/fcm/send/x", "endpoint-not-https"),
        ("https://fcm.googleapis.com.evil.example/x", "endpoint-not-allowed"),
        ("not a url", "bad-endpoint"),
    ] {
        let subscription = PushSecret::generate(endpoint).subscription(&[]);
        assert_eq!(refusal_of(person.subscribe_push(&subscription).await), (400, refusal.to_string()), "{endpoint}");
    }
    let bad_keys = PushSubscribe {
        endpoint: "https://fcm.googleapis.com/fcm/send/abc".into(),
        keys: PushKeys { p256dh: URL_SAFE_NO_PAD.encode([4u8; 65]), auth: URL_SAFE_NO_PAD.encode([1u8; 16]) },
        categories: Vec::new(),
    };
    assert_eq!(refusal_of(person.subscribe_push(&bad_keys).await), (400, "bad-push-keys".to_string()));
    assert_eq!(rows(&test, "push_subscriptions"), 1);
    let path = format!("/v1/a/devices/{}/push", Id::new(Prefix::Device).unwrap());
    assert_eq!(refusal_of(person.put::<_, Option<()>>(&path, &good.subscription(&[])).await), (404, "unknown-device".to_string()));
}

#[tokio::test]
async fn an_endpoint_moves_between_devices_of_one_account_and_never_to_another() {
    let test = start_hub(|config| config.vapid_subject = SUBJECT.to_string()).await;
    let (_machine, mut person) = notifying(&test).await;
    person.register_device().await.unwrap();
    let shared = PushSecret::generate("https://web.push.apple.com/shared");
    person.subscribe_push(&shared.subscription(&[])).await.unwrap();
    let (key, cert) = another_device(&person, "Same phone, new key");
    person.use_device(key, cert);
    person.register_device().await.unwrap();
    person.subscribe_push(&shared.subscription(&[])).await.unwrap();
    assert_eq!(rows(&test, "push_subscriptions"), 1);
    assert!(test.hub.store().events().unwrap().iter().any(|(kind, _)| kind == "push.moved"));

    let mut stranger = member(&test).await;
    let _theirs = connected_with_board(&test, &mut stranger).await;
    stranger.register_device().await.unwrap();
    assert_eq!(refusal_of(stranger.subscribe_push(&shared.subscription(&[])).await), (409, "endpoint-taken".to_string()));
}

#[tokio::test]
async fn nothing_readable_is_kept_and_a_notice_id_is_swept_once_it_expires() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    machine.notify(notice(NoticeCategory::Questions, &format!("{MARKER} secret body")), "q", Urgency::High).await.unwrap();
    service.next().await;
    settle(&test).await;
    assert_eq!(rows(&test, "notices"), 1);
    assert!(!holds(&database_bytes(&test), MARKER.as_bytes()));
    for (kind, detail) in test.hub.store().events().unwrap() {
        assert!(!detail.unwrap_or_default().contains(MARKER), "{kind}");
    }
    test.hub.store().sweep(kebbie_hub::now() + 2 * 86_400).unwrap();
    assert_eq!(rows(&test, "notices"), 0);
}

#[tokio::test]
async fn the_vapid_key_is_published_and_kept_across_restarts() {
    let data = tempfile::tempdir().unwrap();
    let open = || {
        let mut config = Config::new(data.path());
        config.vapid_subject = SUBJECT.to_string();
        Hub::open(config).unwrap()
    };
    let first = open().vapid_public_key().to_string();
    assert_eq!(URL_SAFE_NO_PAD.decode(&first).unwrap().len(), 65);
    assert_eq!(open().vapid_public_key(), first);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = data.path().join(kebbie_hub::VAPID_KEY_FILE);
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut config = Config::new(data.path());
        config.vapid_subject = SUBJECT.to_string();
        assert!(matches!(Hub::open(config), Err(OpenError::Push(_))), "a key others can read is refused");
    }
    let mut refused = Config::new(tempfile::tempdir().unwrap().path());
    refused.vapid_subject = "mailto:admin@hub.local".to_string();
    assert!(matches!(Hub::open(refused), Err(OpenError::Push(_))));
}

#[tokio::test]
async fn a_redirect_from_a_push_service_is_not_followed() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    service.answer("phone", &[307]);
    machine.notify(notice(NoticeCategory::Questions, "x"), "q", Urgency::High).await.unwrap();
    service.next().await;
    settle(&test).await;
    assert!(service.drain().is_empty());
    assert!(test.hub.store().events().unwrap().iter().any(|(kind, detail)| kind == "push.refused" && detail.as_deref().is_some_and(|detail| detail.contains("status=307"))));
}

async fn approving(test: &TestHub) -> (MachineState, Browser, (DeviceKey, DeviceCert), (DeviceKey, DeviceCert)) {
    let (mut machine, mut person) = notifying(test).await;
    machine.trust_devices = true;
    machine.post_board(board(MARKER)).await.unwrap();
    let phone = (person.device_key().clone(), person.device_cert().unwrap().clone());
    person.register_device().await.unwrap();
    person.command(&machine.machine, &answer()).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    machine.decide_device(&phone.1.device, true, None).await.unwrap();
    let tablet = another_device(&person, "iPad");
    person.use_device(tablet.0.clone(), tablet.1.clone());
    person.command(&machine.machine, &answer()).await.unwrap();
    let held = machine.apply_once(0, None).await.unwrap();
    assert!(matches!(held[0].handling, Handling::Waiting(_)), "{held:?}");
    person.use_device(phone.0.clone(), phone.1.clone());
    (machine, person, phone, tablet)
}

fn answer() -> Payload {
    Payload::Answer(Answer { address: ADDRESS.into(), question: "q1".into(), choice: Some(0), comment: String::new() })
}

fn decision(cert: &DeviceCert, approve: bool) -> Payload {
    Payload::DeviceDecision(DeviceDecision { device: cert.device.clone(), fingerprint: cert.fingerprint(), approve })
}

#[tokio::test]
async fn an_approved_device_approves_a_waiting_one_and_its_held_command_goes_through() {
    let test = push_hub(|_| {}).await;
    let (mut machine, mut person, _phone, tablet) = approving(&test).await;
    assert!(machine.board.as_ref().unwrap().accepts.as_ref().unwrap().contains(&Kind::DeviceDecision));
    let sent = person.command(&machine.machine, &decision(&tablet.1, true)).await.unwrap();
    let handled = machine.apply_once(0, None).await.unwrap();
    let applied = handled.iter().find(|entry| entry.command == sent.header.id).unwrap();
    assert!(matches!(&applied.handling, Handling::Applied { result, .. } if result.outcome == Outcome::Done && result.text == "Approved iPad."), "{applied:?}");
    let released = handled.iter().find(|entry| entry.from == tablet.1.device).unwrap();
    assert!(matches!(released.handling, Handling::Applied { .. }), "the tablet's held answer applies once it is approved");
    let standing = machine.board.as_ref().unwrap().devices.as_ref().unwrap()[&tablet.1.device].clone();
    assert!(matches!(standing, DeviceStanding::Approved { .. }));
    let result = person.await_result(&sent.header.id, SOON).await.unwrap().unwrap();
    assert_eq!(result.outcome, Outcome::Done);

    let again = person.command(&machine.machine, &decision(&tablet.1, false)).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    let settled = person.await_result(&again.header.id, SOON).await.unwrap().unwrap();
    assert_eq!(settled.outcome, Outcome::Refused { reason: Reason::AlreadySettled });
}

#[tokio::test]
async fn a_machine_that_takes_approvals_only_at_the_machine_refuses_a_device_decision() {
    let test = push_hub(|_| {}).await;
    let (mut machine, mut person, _phone, tablet) = approving(&test).await;
    machine.approve_from = ApproveFrom::Machine;
    machine.post_board(board(MARKER)).await.unwrap();
    assert!(!machine.board.as_ref().unwrap().accepts.as_ref().unwrap().contains(&Kind::DeviceDecision));
    let sent = person.command(&machine.machine, &decision(&tablet.1, true)).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    let result = person.await_result(&sent.header.id, SOON).await.unwrap().unwrap();
    assert_eq!(result.outcome, Outcome::Refused { reason: Reason::NotAllowed });
    assert!(matches!(machine.devices[&tablet.1.device].standing, DeviceStanding::Waiting { .. }));

    machine.approve_from = ApproveFrom::Devices;
    let misprinted = Payload::DeviceDecision(DeviceDecision { device: tablet.1.device.clone(), fingerprint: "0000-0000".into(), approve: true });
    let sent = person.command(&machine.machine, &misprinted).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    let result = person.await_result(&sent.header.id, SOON).await.unwrap().unwrap();
    assert_eq!(result.outcome, Outcome::Refused { reason: Reason::UnknownDevice });
}

#[tokio::test]
async fn a_device_removed_under_account_keeps_what_it_sent_listed_but_sends_and_hears_nothing_more() {
    let test = push_hub(|_| {}).await;
    let service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    let secret = subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    person.command(&machine.machine, &answer()).await.unwrap();
    let device = person.device_id().clone();
    let path = format!("/v1/a/devices/{device}");
    assert_eq!(rows(&test, "queue"), 1);

    let mut stranger = member(&test).await;
    assert_eq!(refusal_of(stranger.delete(&path).await), (404, "unknown-device".to_string()));
    person.delete(&path).await.unwrap();
    person.delete(&path).await.unwrap();

    let view = person.devices().await.unwrap().devices.pop().unwrap();
    assert!(view.removed_at.is_some() && view.push.is_none(), "{view:?}");
    assert_eq!(rows(&test, "push_subscriptions"), 0);
    assert_eq!(rows(&test, "queue"), 0, "what it queued for a sleeping machine is never delivered");
    assert_eq!(person.device_commands(&device, None).await.unwrap().commands.len(), 1, "what it sent stays listed");
    assert!(machine.notified_devices().await.unwrap().devices.is_empty(), "machines no longer seal notices to it");
    assert_eq!(machine.notify(notice(NoticeCategory::Questions, "x"), "q", Urgency::High).await.unwrap().pushes, 0);

    assert_eq!(refusal_of(person.command(&machine.machine, &answer()).await), (403, "device-removed".to_string()));
    assert_eq!(refusal_of(person.register_device().await), (410, "device-removed".to_string()));
    assert_eq!(refusal_of(person.subscribe_push(&secret.subscription(&[])).await), (410, "device-removed".to_string()));
}

#[tokio::test]
async fn an_account_holds_at_most_its_device_cap_and_removing_one_frees_a_place() {
    let test = push_hub(|config| config.max_devices_per_account = 2).await;
    let (_machine, mut person) = notifying(&test).await;
    let first = person.register_device().await.unwrap().device;
    let tablet = another_device(&person, "iPad");
    person.use_device(tablet.0, tablet.1);
    person.register_device().await.unwrap();
    let laptop = another_device(&person, "Laptop");
    person.use_device(laptop.0.clone(), laptop.1.clone());
    assert_eq!(refusal_of(person.register_device().await), (409, "too-many-devices".to_string()));
    person.delete(&format!("/v1/a/devices/{first}")).await.unwrap();
    person.use_device(laptop.0, laptop.1);
    person.register_device().await.unwrap();
    person.rename_device("Laptop at home").await.unwrap();
    assert_eq!(person.devices().await.unwrap().devices.iter().filter(|device| device.removed_at.is_none()).count(), 2);
}

#[tokio::test]
async fn devices_and_notifications_close_with_the_command_switch_and_the_accounts_gate() {
    let off = push_hub(|config| config.commands = false).await;
    let mut person = member(&off).await;
    connected_with_board(&off, &mut person).await;
    assert_eq!(refusal_of(person.register_device().await), (404, "commands-off".to_string()));
    let secret = PushSecret::generate("http://127.0.0.1:9/push/phone");
    assert_eq!(refusal_of(person.subscribe_push(&secret.subscription(&[])).await), (404, "commands-off".to_string()));

    let open = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let gate: Box<dyn kebbie_hub::Plans> = {
        let open = Arc::clone(&open);
        Box::new(move |_: &kebbie_hub::store::Store, _: &str, _: kebbie_hub::Gated| if open.load(std::sync::atomic::Ordering::SeqCst) { Ok(()) } else { Err(kebbie_hub::Refusal { url: Some("https://example.com/plan".into()), pro: false }) })
    };
    let test = start_hub_with(
        |config| {
            config.vapid_subject = SUBJECT.to_string();
            config.push_allow_hosts = vec!["127.0.0.1".to_string()];
        },
        Some(gate),
        Router::new(),
    )
    .await;
    let service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    let secret = subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    let envelope = machine.notify(notice(NoticeCategory::Questions, "x"), "q", Urgency::High).await.unwrap().envelope.unwrap();
    open.store(false, std::sync::atomic::Ordering::SeqCst);
    let inactive = (402, kebbie_hub_api::PLAN_INACTIVE.to_string());
    assert_eq!(refusal_of(person.subscribe_push(&secret.subscription(&[])).await), inactive);
    assert_eq!(refusal_of(person.set_categories(&[NoticeCategory::Ready]).await), inactive);
    assert_eq!(refusal_of(post(&machine, envelope).await), inactive);
    assert!(machine.notified_devices().await.unwrap().devices.is_empty());
    person.unsubscribe_push().await.unwrap();
    assert_eq!(rows(&test, "push_subscriptions"), 0, "a closed gate still lets a device stop its notifications");
    let laptop = another_device(&person, "Laptop");
    person.use_device(laptop.0, laptop.1);
    assert_eq!(refusal_of(person.register_device().await), inactive);
}

#[tokio::test]
async fn the_leads_answer_is_a_notice_on_by_default_even_for_a_subscription_older_than_it() {
    let test = push_hub(|_| {}).await;
    let mut service = PushService::start().await;
    let (mut machine, mut person) = notifying(&test).await;
    let secret = subscribed(&mut person, &service, "phone", &NoticeCategory::defaults()).await;
    let wanted = |machine: &MachineState| {
        let machine = machine.clone();
        async move { machine.notified_devices().await.unwrap().devices[0].notifications.clone().unwrap() }
    };
    assert!(wanted(&machine).await.contains(&NoticeCategory::LeadAnswered));

    let connection = rusqlite::Connection::open(test.hub.config().database()).unwrap();
    connection
        .execute(
            r#"UPDATE push_subscriptions SET categories = '["questions","new-device","idea-questions","ready","ci-failed"]', offered = '["questions","new-device","idea-questions","ready","ci-failed","started"]'"#,
            [],
        )
        .unwrap();
    assert!(wanted(&machine).await.contains(&NoticeCategory::LeadAnswered), "a category the subscription was never offered starts at its default");
    assert!(!wanted(&machine).await.contains(&NoticeCategory::Started));

    machine.lead_replies(&format!("{MARKER} Done. email-column is at step 3 of 5."), false);
    machine.publish_lead().await.unwrap();
    let sent = machine.notify_lead_answered().await.unwrap();
    assert_eq!((sent.recipients.len(), sent.pushes), (1, 1));
    let (envelope, notice) = opened_push(&secret, &person, &service.next().await).unwrap();
    assert_eq!((notice.category, notice.open), (NoticeCategory::LeadAnswered, Target::Lead {}));
    assert_eq!(envelope.header.collapse.as_deref(), Some("l-lead"));
    assert!(notice.body.starts_with(MARKER));

    person.set_categories(&[NoticeCategory::Questions]).await.unwrap();
    assert_eq!(wanted(&machine).await, [NoticeCategory::Questions], "once chosen among every category, off stays off");
    assert_eq!(machine.notify_lead_answered().await.unwrap().recipients.len(), 0);
}
