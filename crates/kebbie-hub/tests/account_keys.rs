#[allow(dead_code)]
mod common;

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::*;
use kebbie_hub_api::{
    AccountKeySetup, Bytes32, Bytes64, CreationCeremony, LoginBegin, Passkeys, RegisterBegin,
    RequestCeremony, WrapperUpload,
};
use kebbie_hub_fakes::browser::{Browser, Lock};
use kebbie_hub_protocol::{AccountKey, Id, SecretKeys, Unlock, hub_prf_salt, sign_wrapper};

fn wrap_for(
    account: &str,
    credential: &[u8],
    version: u32,
) -> (AccountKey, SecretKeys, Vec<u8>, [u8; 64]) {
    let secret = SecretKeys::generate();
    let key = AccountKey::new(Id::parse(account).unwrap(), version, secret.public()).unwrap();
    let wrapped = Unlock {
        account: &key.account,
        credential,
        version,
        prf_output: &[4; 32],
        salt: &[5; 32],
    }
    .wrap(&secret)
    .unwrap();
    let signature = sign_wrapper(&secret, &key, credential, &wrapped).unwrap();
    (key, secret, wrapped, signature)
}

async fn without_prf(test: &TestHub) -> Browser {
    let mut person = browser(test);
    person.prf_capable = false;
    person
        .sign_up_with_invite(&test.hub.mint_invite().unwrap())
        .await
        .unwrap();
    person
}

fn set_salt(test: &TestHub, credential: &[u8], salt: [u8; 32]) {
    rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .execute(
            "UPDATE passkeys SET prf_salt = ?2 WHERE credential_id = ?1",
            rusqlite::params![credential, salt.as_slice()],
        )
        .unwrap();
}

async fn salt_of(person: &mut Browser, credential: &[u8]) -> [u8; 32] {
    let named = URL_SAFE_NO_PAD.encode(credential);
    let begun: RequestCeremony = person
        .post(
            "/v1/a/login/begin",
            &LoginBegin {
                credential: Some(named.clone()),
            },
        )
        .await
        .unwrap();
    begun.prf_salts[&named].0
}

async fn passkeys(person: &mut Browser) -> Vec<(Vec<u8>, bool)> {
    let listed: Passkeys = person.get("/v1/a/passkeys").await.unwrap();
    listed
        .passkeys
        .into_iter()
        .map(|view| (URL_SAFE_NO_PAD.decode(view.passkey).unwrap(), view.unlocks))
        .collect()
}

#[tokio::test]
async fn the_first_sign_in_sets_the_account_key_and_every_later_one_unwraps_it() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    assert_eq!(person.lock, Lock::Unlocked);
    let key = person.account_key().unwrap().clone();
    assert_eq!(key.version, 1);
    assert_eq!(person.account().await.unwrap().key, Some(key.clone()));

    person.forget_keys();
    person.log_in().await.unwrap();
    assert_eq!(
        (person.lock, person.account_key()),
        (Lock::Unlocked, Some(&key))
    );
    let credential = person.last_credential.clone().unwrap();
    person.forget_keys();
    person.log_in_with_credential(&credential).await.unwrap();
    assert_eq!(person.account_key(), Some(&key));

    let named = URL_SAFE_NO_PAD.encode(&credential);
    let by_credential: RequestCeremony = person
        .post(
            "/v1/a/login/begin",
            &LoginBegin {
                credential: Some(named.clone()),
            },
        )
        .await
        .unwrap();
    let again: RequestCeremony = person
        .post(
            "/v1/a/login/begin",
            &LoginBegin {
                credential: Some(named.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(by_credential.prf_salts.keys().collect::<Vec<_>>(), [&named]);
    assert_eq!(
        by_credential.prf_salts[&named].0,
        hub_prf_salt(),
        "a new passkey has the hub's salt"
    );
    assert_eq!(by_credential.prf_salt.0, hub_prf_salt());
    assert_eq!(
        by_credential.prf_salts, again.prf_salts,
        "a passkey keeps its salt"
    );
    assert_eq!(by_credential.options.public_key.allow_credentials.len(), 1);
    let discoverable: RequestCeremony = person
        .post("/v1/a/login/begin", &LoginBegin::default())
        .await
        .unwrap();
    assert!(discoverable.prf_salts.is_empty());
    assert_eq!(
        discoverable.prf_salt.0,
        hub_prf_salt(),
        "a discoverable sign-in can ask for the PRF"
    );
    let unknown = person
        .post::<_, RequestCeremony>(
            "/v1/a/login/begin",
            &LoginBegin {
                credential: Some(URL_SAFE_NO_PAD.encode(b"nobody")),
            },
        )
        .await;
    assert_eq!(status_of(unknown), 401);
}

#[tokio::test]
async fn a_key_is_set_once_from_a_fresh_session_with_a_signed_wrap_of_ones_own_passkey() {
    let test = start_hub(|config| config.fresh_session_ttl = 1).await;
    let mut person = without_prf(&test).await;
    assert_eq!(person.lock, Lock::NoPrf);
    assert_eq!(person.account().await.unwrap().key, None);
    let account = person.account().await.unwrap().account;
    let credential = person.last_credential.clone().unwrap();
    let setup = |key: &AccountKey, credential: &[u8], wrapped: &[u8], signature: [u8; 64]| {
        AccountKeySetup {
            key: key.clone(),
            credential: URL_SAFE_NO_PAD.encode(credential),
            wrapped: URL_SAFE_NO_PAD.encode(wrapped),
            signature: Bytes64(signature),
            prf_salt: None,
        }
    };
    let post = async |person: &mut Browser, body: &AccountKeySetup| {
        person
            .post::<_, Option<()>>("/v1/a/key", body)
            .await
            .map(drop)
            .map_err(hub_code)
    };

    let stranger = member(&test).await;
    let (key, _, wrapped, signature) = wrap_for(
        &stranger.account_key().unwrap().account.to_string(),
        &credential,
        1,
    );
    assert_eq!(
        post(&mut person, &setup(&key, &credential, &wrapped, signature)).await,
        Err((400, "bad-key".to_string()))
    );
    let (key, _, wrapped, signature) = wrap_for(&account, &credential, 2);
    assert_eq!(
        post(&mut person, &setup(&key, &credential, &wrapped, signature)).await,
        Err((400, "bad-key".to_string()))
    );
    let (key, _, wrapped, _) = wrap_for(&account, &credential, 1);
    assert_eq!(
        post(&mut person, &setup(&key, &credential, &wrapped, [0; 64])).await,
        Err((400, "bad-signature".to_string()))
    );
    assert_eq!(
        post(
            &mut person,
            &setup(&key, &credential, &wrapped[1..], [0; 64])
        )
        .await,
        Err((400, "bad-wrapped".to_string()))
    );
    let (key, _, wrapped, signature) = wrap_for(&account, b"not-mine", 1);
    assert_eq!(
        post(&mut person, &setup(&key, b"not-mine", &wrapped, signature)).await,
        Err((404, "unknown-passkey".to_string()))
    );

    tokio::time::sleep(Duration::from_millis(2100)).await;
    let (key, _, wrapped, signature) = wrap_for(&account, &credential, 1);
    let body = setup(&key, &credential, &wrapped, signature);
    assert_eq!(
        post(&mut person, &body).await,
        Err((401, "reauth-required".to_string()))
    );
    person.log_in().await.unwrap();
    let elsewhere = AccountKeySetup {
        prf_salt: Some(Bytes32([9; 32])),
        ..body.clone()
    };
    assert_eq!(
        post(&mut person, &elsewhere).await,
        Err((400, "bad-salt".to_string()))
    );
    set_salt(&test, &credential, [7; 32]);
    let body = AccountKeySetup {
        prf_salt: Some(Bytes32(hub_prf_salt())),
        ..body
    };
    assert_eq!(post(&mut person, &body).await, Ok(()));
    assert_eq!(salt_of(&mut person, &credential).await, hub_prf_salt());
    assert_eq!(person.account().await.unwrap().key, Some(key));
    let (other, _, wrapped, signature) = wrap_for(&account, &credential, 1);
    assert_eq!(
        post(
            &mut person,
            &setup(&other, &credential, &wrapped, signature)
        )
        .await,
        Err((409, "key-exists".to_string()))
    );
    assert_eq!(passkeys(&mut person).await, [(credential, true)]);
}

#[tokio::test]
async fn a_passkey_added_while_unlocked_unlocks_and_one_added_while_locked_does_not() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let key = person.account_key().unwrap().clone();
    let added = person.add_passkey().await.unwrap();
    assert!(added.unlocks);
    let second = URL_SAFE_NO_PAD.decode(&added.passkey).unwrap();
    person.forget_keys();
    person.log_in_with_credential(&second).await.unwrap();
    assert_eq!(
        (person.lock, person.account_key()),
        (Lock::Unlocked, Some(&key))
    );

    person.forget_keys();
    let third = person.add_passkey().await.unwrap();
    assert!(!third.unlocks);
    let third = URL_SAFE_NO_PAD.decode(&third.passkey).unwrap();
    person.log_in_with_credential(&third).await.unwrap();
    assert_eq!(person.lock, Lock::NotWrapped);
    let listed = passkeys(&mut person).await;
    assert_eq!(listed.iter().filter(|(_, unlocks)| *unlocks).count(), 2);
    assert!(listed.contains(&(third, false)));
}

#[tokio::test]
async fn a_wrapper_needs_the_account_signature_its_key_version_and_a_passkey_of_the_account() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let key = person.account_key().unwrap().clone();
    let credential = person.last_credential.clone().unwrap();
    let put = async |person: &mut Browser, credential: &[u8], body: &WrapperUpload| {
        person
            .put::<_, Option<()>>(
                &format!(
                    "/v1/a/passkeys/{}/wrapper",
                    URL_SAFE_NO_PAD.encode(credential)
                ),
                body,
            )
            .await
            .map(drop)
            .map_err(hub_code)
    };
    let (_, _, wrapped, forged) = wrap_for(key.account.as_str(), &credential, 1);
    let body = |version, signature| WrapperUpload {
        key_version: version,
        wrapped: URL_SAFE_NO_PAD.encode(&wrapped),
        signature: Bytes64(signature),
        prf_salt: None,
    };
    assert_eq!(
        put(&mut person, &credential, &body(1, forged)).await,
        Err((400, "bad-signature".to_string()))
    );
    assert_eq!(
        put(&mut person, &credential, &body(2, forged)).await,
        Err((409, "key-version".to_string()))
    );
    let stranger = member(&test).await;
    let theirs = stranger.last_credential.clone().unwrap();
    assert_eq!(
        put(&mut person, &theirs, &body(1, forged))
            .await
            .unwrap_err()
            .0,
        400
    );
    let mut locked = without_prf(&test).await;
    let own = locked.last_credential.clone().unwrap();
    assert_eq!(
        put(&mut locked, &own, &body(1, forged)).await,
        Err((409, "no-key".to_string()))
    );
}

#[tokio::test]
async fn the_last_passkey_that_unlocks_the_account_cannot_be_removed() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let first = person.last_credential.clone().unwrap();
    person.forget_keys();
    let spare = person.add_passkey().await.unwrap();
    assert!(!spare.unlocks);
    let removed = person
        .delete(&format!(
            "/v1/a/passkeys/{}",
            URL_SAFE_NO_PAD.encode(&first)
        ))
        .await;
    assert_eq!(
        hub_code(removed.unwrap_err()),
        (409, "last-unlocking-passkey".to_string())
    );
    person
        .delete(&format!("/v1/a/passkeys/{}", spare.passkey))
        .await
        .unwrap();
    assert_eq!(passkeys(&mut person).await, [(first, true)]);
}

#[tokio::test]
async fn a_device_without_prf_signs_in_but_holds_no_key() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = without_prf(&test).await;
    assert_eq!(person.lock, Lock::NoPrf);
    person.log_in().await.unwrap();
    assert_eq!((person.lock, person.account_key()), (Lock::NoPrf, None));
    let pending = kebbie_hub_fakes::machine::connect(&test.url, "mac")
        .await
        .unwrap();
    let refused = person.approve_connect(&pending.link(), true).await;
    assert_eq!(hub_code(refused.err().unwrap()).1, "locked");
    person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    pending.wait(WAIT).await.unwrap();
    let boards = person.boards().await.unwrap();
    assert!(matches!(
        boards[0].board,
        Err(kebbie_hub_fakes::browser::BoardProblem::NotSealedToTheAccount)
    ));
}

#[tokio::test]
async fn new_passkeys_get_the_hub_salt_and_a_wrapper_moves_an_old_salt_onto_it() {
    let test = start_hub(|_| {}).await;
    let fresh: CreationCeremony = browser(&test)
        .post(
            "/v1/a/register/begin",
            &RegisterBegin {
                invite: test.hub.mint_invite().unwrap(),
                ..RegisterBegin::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(fresh.prf_salt.0, hub_prf_salt());
    let mut person = member(&test).await;
    let added: CreationCeremony = person.post("/v1/a/passkeys/begin", &()).await.unwrap();
    assert_eq!(added.prf_salt.0, hub_prf_salt());

    let key = person.account_key().unwrap().clone();
    let (signing, encryption) = person.account_secret().unwrap();
    let secret = SecretKeys::from_bytes(signing, encryption);
    let credential = person.last_credential.clone().unwrap();
    let mut stranger = member(&test).await;
    let theirs = stranger.last_credential.clone().unwrap();
    let old = [7; 32];
    set_salt(&test, &credential, old);
    set_salt(&test, &theirs, old);
    assert_eq!(salt_of(&mut person, &credential).await, old);
    let rewrap = |credential: &[u8], prf_salt: Option<[u8; 32]>| {
        let wrapped = Unlock {
            account: &key.account,
            credential,
            version: key.version,
            prf_output: &[4; 32],
            salt: &hub_prf_salt(),
        }
        .wrap(&secret)
        .unwrap();
        WrapperUpload {
            key_version: key.version,
            wrapped: URL_SAFE_NO_PAD.encode(&wrapped),
            signature: Bytes64(sign_wrapper(&secret, &key, credential, &wrapped).unwrap()),
            prf_salt: prf_salt.map(Bytes32),
        }
    };
    let put = async |person: &mut Browser, credential: &[u8], body: &WrapperUpload| {
        person
            .put::<_, Option<()>>(
                &format!(
                    "/v1/a/passkeys/{}/wrapper",
                    URL_SAFE_NO_PAD.encode(credential)
                ),
                body,
            )
            .await
            .map(drop)
            .map_err(hub_code)
    };

    assert_eq!(
        put(
            &mut person,
            &credential,
            &rewrap(&credential, Some([9; 32]))
        )
        .await,
        Err((400, "bad-salt".to_string()))
    );
    assert_eq!(
        put(&mut person, &theirs, &rewrap(&theirs, Some(hub_prf_salt()))).await,
        Err((404, "unknown-passkey".to_string()))
    );
    assert_eq!(
        salt_of(&mut stranger, &theirs).await,
        old,
        "another account's salt stays"
    );
    assert_eq!(
        put(&mut person, &credential, &rewrap(&credential, None)).await,
        Ok(())
    );
    assert_eq!(
        salt_of(&mut person, &credential).await,
        old,
        "no salt named, none changed"
    );
    assert_eq!(
        put(
            &mut person,
            &credential,
            &rewrap(&credential, Some(hub_prf_salt()))
        )
        .await,
        Ok(())
    );
    assert_eq!(salt_of(&mut person, &credential).await, hub_prf_salt());
}
