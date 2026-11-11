use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_protocol::{
    AppliedCommands, BlobRef, CommandResult, CommitmentMismatch, DevicePairing, Envelope, Id, Kind, Machine,
    MachinePairing, Payload, PublicKeys, Refusal, SecretKeys, TrustedDevice, authorize, open, open_blob,
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
fn a_connect_link_names_its_machine_and_binds_the_browser_commitment() {
    use kebbie_hub_protocol::{ConnectSecret, machine_fingerprint};
    let parties = parties();
    let vector = &load("pairing.json")["connect"];
    let text = |key: &str| vector[key].as_str().unwrap().to_string();
    let machine = &parties[&text("machine")];
    let device = &parties[&text("device")];
    assert_eq!(
        URL_SAFE_NO_PAD.encode(machine_fingerprint(&machine.public)),
        text("fingerprint")
    );
    let link = text("link");
    let secret = ConnectSecret::parse(link.split_once('#').unwrap().1).unwrap();
    assert!(secret.names(&machine.public));
    assert!(!secret.names(&parties["intruder"].public));
    assert_eq!(secret.pairing_code(), text("code"));
    let browser = DevicePairing::with_nonce(
        device.public,
        bytes32(&vector["device_nonce"]),
        &text("code"),
    );
    assert_eq!(browser.commitment(), bytes32(&vector["commitment"]));
    let answering = MachinePairing::with_nonce(
        machine.public,
        browser.commitment(),
        bytes32(&vector["machine_nonce"]),
        &text("code"),
    );
    assert!(
        answering
            .comparison_code(&device.public, &bytes32(&vector["device_nonce"]))
            .is_ok()
    );
}

fn bytes(value: &Value) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(value.as_str().unwrap()).unwrap()
}

#[test]
fn account_keys_wrap_sign_and_pin_as_the_vectors_say() {
    use kebbie_hub_protocol::{
        AccountFingerprints, AccountHandoff, AccountKey, DEVICE_LIST_DOMAIN, DeviceList, HUB_PRF_SALT_DOMAIN,
        MachineList, Unlock,
        hub_prf_salt, pin_account, sign_wrapper, wrap_associated_data, wrap_key, wrapper_statement,
    };
    let parties = parties();
    let vectors = load("accounts.json");
    let account = &parties[vectors["account"].as_str().unwrap()];
    let key: AccountKey = serde_json::from_value(vectors["key"].clone()).unwrap();
    assert_eq!((&key.account, key.keys), (&account.id, account.public));
    let wrap = &vectors["wrap"];
    let credential = bytes(&wrap["credential"]);
    let (salt, prf_output) = (bytes32(&wrap["salt"]), bytes32(&wrap["prf_output"]));
    assert_eq!(wrap_key(&prf_output, &salt), bytes32(&wrap["wrap_key"]));
    assert_eq!(
        wrap_associated_data(&key.account, &credential, key.version).unwrap(),
        bytes(&wrap["associated_data"])
    );
    let unlock = Unlock {
        account: &key.account,
        credential: &credential,
        version: key.version,
        prf_output: &prf_output,
        salt: &salt,
    };
    let wrapped = bytes(&wrap["wrapped"]);
    let nonce: [u8; 12] = bytes(&wrap["nonce"]).try_into().unwrap();
    assert_eq!(unlock.wrap_with_nonce(&account.secret, nonce).unwrap(), wrapped);
    assert_eq!(unlock.unwrap(&wrapped).unwrap().public(), account.public);
    for refusal in wrap["refusals"].as_array().unwrap() {
        let changed_account = Id::parse(refusal["account"].as_str().unwrap()).unwrap();
        let changed_credential = bytes(&refusal["credential"]);
        let changed = Unlock {
            account: &changed_account,
            credential: &changed_credential,
            version: refusal["key_version"].as_u64().unwrap() as u32,
            prf_output: &bytes32(&refusal["prf_output"]),
            salt: &bytes32(&refusal["salt"]),
        };
        assert!(changed.unwrap(&wrapped).is_err(), "{}", refusal["name"]);
    }
    let hub_salt = &vectors["hub_prf_salt"];
    assert_eq!(
        hub_salt["domain"].as_str().unwrap().as_bytes(),
        HUB_PRF_SALT_DOMAIN
    );
    assert_eq!(hub_prf_salt(), bytes32(&hub_salt["salt"]));
    assert_eq!(
        wrap_key(&bytes32(&hub_salt["prf_output"]), &hub_prf_salt()),
        bytes32(&hub_salt["wrap_key"])
    );
    let fingerprints: AccountFingerprints =
        serde_json::from_value(vectors["fingerprints"].clone()).unwrap();
    assert_eq!(key.fingerprints(), fingerprints);
    let wrapper = &vectors["wrapper"];
    assert_eq!(
        wrapper_statement(&key, &credential, &wrapped).unwrap(),
        bytes(&wrapper["statement"])
    );
    let signature: [u8; 64] = bytes(&wrapper["signature"]).try_into().unwrap();
    assert_eq!(sign_wrapper(&account.secret, &key, &credential, &wrapped).unwrap(), signature);
    assert!(key.verifies_wrapper(&credential, &wrapped, &signature));
    let lists = &vectors["machines"];
    let list: MachineList = serde_json::from_value(lists["list"].clone()).unwrap();
    assert_eq!(list.statement(&key.account).unwrap(), bytes(&lists["statement"]));
    let signature: [u8; 64] = bytes(&lists["signature"]).try_into().unwrap();
    assert_eq!(list.sign(&account.secret, &key.account).unwrap(), signature);
    assert!(key.verifies_list(&list, &signature));
    let removed: MachineList = serde_json::from_value(lists["removed"].clone()).unwrap();
    let removed_signature: [u8; 64] = bytes(&lists["removed_signature"]).try_into().unwrap();
    assert!(key.verifies_list(&removed, &removed_signature));
    assert!(!key.verifies_list(&removed, &signature));
    assert!(removed.counter > list.counter);
    let trusted = &parties[lists["trusted"].as_str().unwrap()];
    let gone = &parties[lists["removed_machine"].as_str().unwrap()];
    assert!(list.trusts(&gone.id, &gone.public) && !removed.trusts(&gone.id, &gone.public));
    assert!(removed.trusts(&trusted.id, &trusted.public));
    let devices = &vectors["devices"];
    assert_eq!(devices["domain"].as_str().unwrap().as_bytes(), DEVICE_LIST_DOMAIN);
    let list: DeviceList = serde_json::from_value(devices["list"].clone()).unwrap();
    assert_eq!(list.statement(&key.account).unwrap(), bytes(&devices["statement"]));
    let signature: [u8; 64] = bytes(&devices["signature"]).try_into().unwrap();
    assert_eq!(list.sign(&account.secret, &key.account).unwrap(), signature);
    assert!(key.verifies_device_list(&list, &signature));
    let removed: DeviceList = serde_json::from_value(devices["removed"].clone()).unwrap();
    let removed_signature: [u8; 64] = bytes(&devices["removed_signature"]).try_into().unwrap();
    assert!(key.verifies_device_list(&removed, &removed_signature));
    assert!(!key.verifies_device_list(&removed, &signature));
    assert!(removed.counter > list.counter);
    let kept = &parties[devices["trusted"].as_str().unwrap()];
    let gone = &parties[devices["removed_device"].as_str().unwrap()];
    assert!(list.listed(&gone.id).is_some() && removed.listed(&gone.id).is_none());
    assert_eq!(removed.listed(&kept.id).map(|listed| listed.keys), Some(kept.public));
    let handoff = &vectors["handoff"];
    let code = handoff["code"].as_str().unwrap();
    let nonce = bytes32(&handoff["nonce"]);
    let commitment = AccountHandoff::with_nonce(key.clone(), nonce, code).commitment();
    assert_eq!(commitment, bytes32(&handoff["commitment"]));
    for reveal in handoff["reveals"].as_array().unwrap() {
        let revealed: AccountKey = serde_json::from_value(reveal["revealed"].clone()).unwrap();
        let pinned = pin_account(
            &commitment,
            &revealed,
            &nonce,
            reveal["machine_code"].as_str().unwrap(),
        );
        match reveal["expect"].get("pinned") {
            Some(expected) => assert_eq!(
                pinned.unwrap(),
                serde_json::from_value::<AccountFingerprints>(expected.clone()).unwrap()
            ),
            None => assert!(pinned.is_err(), "{}", reveal["name"]),
        }
    }
}

#[test]
fn blobs_open_or_fail_as_the_vectors_say() {
    let vectors = load("blobs.json");
    let plaintext = vectors["plaintext"].as_str().unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let blob: BlobRef = serde_json::from_value(case["blob"].clone()).unwrap();
        let ciphertext = URL_SAFE_NO_PAD.decode(case["ciphertext"].as_str().unwrap()).unwrap();
        let opened = open_blob(&blob, &ciphertext);
        match case["error"].as_str() {
            None => assert_eq!(opened.unwrap(), plaintext.as_bytes(), "{name}"),
            Some(error) => assert_eq!(opened.unwrap_err().key(), error, "{name}"),
        }
    }
}
