use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kelpie_hub_protocol::{
    AppliedCommands, CommandResult, CommitmentMismatch, DevicePairing, Envelope, Id, Kind, Machine,
    MachinePairing, Payload, PublicKeys, Refusal, SecretKeys, TrustedDevice, authorize, open,
};
use serde_json::Value;

struct Party {
    id: Id,
    secret: SecretKeys,
    public: PublicKeys,
}

fn load(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vectors")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn bytes32(value: &Value) -> [u8; 32] {
    URL_SAFE_NO_PAD
        .decode(value.as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap()
}

fn parties() -> BTreeMap<String, Party> {
    let keys = load("keys.json");
    keys["parties"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, party)| {
            let secret = SecretKeys::from_bytes(
                bytes32(&party["signing_secret"]),
                bytes32(&party["encryption_secret"]),
            );
            let public: PublicKeys = serde_json::from_value(party["public"].clone()).unwrap();
            assert_eq!(secret.public(), public, "{name}");
            (
                name.clone(),
                Party {
                    id: Id::parse(party["id"].as_str().unwrap()).unwrap(),
                    secret,
                    public,
                },
            )
        })
        .collect()
}

#[test]
fn envelopes_open_as_the_vectors_say() {
    let parties = parties();
    let vectors = load("envelopes.json");
    let now = vectors["now"].as_u64().unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let envelope = Envelope::from_json(&case["envelope"].to_string()).unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(envelope.header.bytes()),
            case["header_bytes"].as_str().unwrap(),
            "{name}"
        );
        let recipient = &parties[case["recipient"].as_str().unwrap()];
        let sender = &parties[case["sender"].as_str().unwrap()];
        let opened = open(
            &envelope,
            &recipient.id,
            &recipient.secret,
            &sender.id,
            &sender.public,
            now,
        );
        match case["expect"].get("payload") {
            Some(payload) => assert_eq!(
                opened.unwrap(),
                serde_json::from_value::<Payload>(payload.clone()).unwrap(),
                "{name}"
            ),
            None => assert_eq!(
                opened.unwrap_err().key(),
                case["expect"]["error"].as_str().unwrap(),
                "{name}"
            ),
        }
    }
}

#[test]
fn commands_are_authorised_as_the_vectors_say() {
    let parties = parties();
    let vectors = load("authorize.json");
    let machine = &parties[vectors["machine"].as_str().unwrap()];
    let trusted: Vec<TrustedDevice> = serde_json::from_value(vectors["trusted"].clone()).unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let accepts: Vec<Kind> = serde_json::from_value(case["accepts"].clone()).unwrap();
        let applied: AppliedCommands = serde_json::from_value(case["applied"].clone()).unwrap();
        let envelope = Envelope::from_json(&case["envelope"].to_string()).unwrap();
        let gate = Machine {
            id: &machine.id,
            keys: &machine.secret,
            devices: &trusted,
            accepts: &accepts,
        };
        let decided = authorize(&gate, &applied, &envelope, vectors["now"].as_u64().unwrap());
        let expect = &case["expect"];
        match decided {
            Ok(accepted) => {
                assert_eq!(
                    accepted.device.id.as_str(),
                    expect["accepted"].as_str().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    accepted.payload,
                    serde_json::from_value::<Payload>(expect["payload"].clone()).unwrap(),
                    "{name}"
                );
            }
            Err(Refusal::Answer(reason)) => {
                assert_eq!(Some(reason.key()), expect["answer"].as_str(), "{name}")
            }
            Err(Refusal::Drop(dropped)) => {
                assert_eq!(Some(dropped.key()), expect["drop"].as_str(), "{name}")
            }
            Err(Refusal::Replay(result)) => assert_eq!(
                result,
                serde_json::from_value::<CommandResult>(expect["replay"].clone()).unwrap(),
                "{name}"
            ),
        }
    }
}

fn bytes32_of(case: &Value, key: &str) -> [u8; 32] {
    bytes32(&case[key])
}

#[test]
fn pairing_flows_end_as_the_vectors_say() {
    let parties = parties();
    for case in load("pairing.json")["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let text = |key: &str| case[key].as_str().unwrap().to_string();
        let machine = &parties[&text("machine")];
        let device = &parties[&text("device")];
        let revealed = &parties[case["revealed"]["device"].as_str().unwrap()];
        let device_side = DevicePairing::with_nonce(
            device.public,
            bytes32_of(case, "device_nonce"),
            &text("code"),
        );
        assert_eq!(
            device_side.commitment(),
            bytes32_of(case, "commitment"),
            "{name}"
        );
        let machine_side = MachinePairing::with_nonce(
            machine.public,
            device_side.commitment(),
            bytes32_of(case, "machine_nonce"),
            &text("machine_code"),
        );
        let decided =
            machine_side.comparison_code(&revealed.public, &bytes32(&case["revealed"]["nonce"]));
        match case["expect"].get("sas") {
            Some(sas) => {
                assert_eq!(decided.as_deref(), Ok(sas.as_str().unwrap()), "{name}");
                assert_eq!(
                    device_side.comparison_code(&machine.public, &machine_side.reply().1),
                    sas.as_str().unwrap(),
                    "{name}"
                );
            }
            None => assert_eq!(decided, Err(CommitmentMismatch), "{name}"),
        }
    }
}

#[test]
fn reports_sign_and_solve_as_the_vectors_say() {
    use kelpie_hub_protocol::{
        BugReport, InstallKey, leading_zero_bits, meets, pow_hash, solve, verify_report,
    };
    let vectors = load("reports.json");
    let key = InstallKey::from_bytes(bytes32(&vectors["install_secret"]));
    assert_eq!(key.public(), bytes32(&vectors["install_public"]));
    let report: BugReport = serde_json::from_value(vectors["report"].clone()).unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.encode(report.bytes()),
        vectors["report_bytes"].as_str().unwrap()
    );
    let digest = report.digest();
    assert_eq!(digest, bytes32(&vectors["digest"]));
    let nonce = bytes32(&vectors["nonce"]);
    let difficulty = vectors["difficulty"].as_u64().unwrap() as u8;
    let counter = vectors["counter"].as_u64().unwrap();
    assert_eq!(
        solve(&nonce, &digest, difficulty),
        counter,
        "the first counter that meets the difficulty"
    );
    let hash = pow_hash(&nonce, &digest, counter);
    assert_eq!(hash, bytes32(&vectors["hash"]));
    assert_eq!(
        u64::from(leading_zero_bits(&hash)),
        vectors["hash_zero_bits"].as_u64().unwrap()
    );
    let short = vectors["short_counter"].as_u64().unwrap();
    assert!(!meets(&nonce, &digest, short, difficulty));
    assert_eq!(
        u64::from(leading_zero_bits(&pow_hash(&nonce, &digest, short))),
        vectors["short_zero_bits"].as_u64().unwrap()
    );
    let signature: [u8; 64] = URL_SAFE_NO_PAD
        .decode(vectors["signature"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(
        key.sign(&nonce, &report),
        signature,
        "Ed25519 signatures are deterministic"
    );
    assert!(verify_report(&key.public(), &nonce, &report, &signature));
    assert!(!verify_report(&key.public(), &digest, &report, &signature));
}
