#[allow(dead_code)]
mod common;

use common::*;
use kelpie_hub::mail::{MailSettings, SmtpSecurity, SmtpSettings};
use kelpie_hub::{Config, Hub};
use kelpie_hub_api::{HubInfo, InviteLink, RegisterBegin, SignupMode, VerifiedEmail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[tokio::test]
async fn a_fresh_hub_is_claimed_only_with_its_setup_link() {
    let test = start_hub(|_| {}).await;
    let setup = test.hub.setup_url().unwrap();
    assert!(
        setup.starts_with("http://localhost:8470/setup?token=kho_"),
        "{setup}"
    );
    let mut anonymous = browser(&test);
    let info: HubInfo = anonymous.get("/v1/hub").await.unwrap();
    assert_eq!(
        info,
        HubInfo {
            signup: SignupMode::Invite,
            plans: false,
            needs_setup: true
        }
    );
    let token = setup.split_once("token=").unwrap().1.to_string();
    let mut guesser = browser(&test);
    for wrong in [format!("{token}x"), "kho_guess".to_string(), String::new()] {
        let refused = guesser
            .sign_up(RegisterBegin {
                setup: Some(wrong),
                ..RegisterBegin::default()
            })
            .await;
        assert_eq!(hub_code(refused.unwrap_err()).0, 401);
    }
    let without = guesser.sign_up(RegisterBegin::default()).await;
    assert_eq!(hub_code(without.unwrap_err()).0, 401);
    assert!(!holds(&database_bytes(&test), token.as_bytes()));

    let mut first = browser(&test);
    let created = first.sign_up_as_owner(&setup).await.unwrap();
    assert!(created.owner);
    assert_eq!(test.hub.setup_url(), None);
    let mut second = browser(&test);
    assert_eq!(
        hub_code(second.sign_up_as_owner(&setup).await.unwrap_err()).0,
        401
    );
    let info: HubInfo = anonymous.get("/v1/hub").await.unwrap();
    assert!(!info.needs_setup);
}

#[tokio::test]
async fn a_hub_mints_a_new_setup_link_at_each_start_until_it_has_an_account() {
    let data = tempfile::tempdir().unwrap();
    let first = Hub::open(Config::new(data.path()))
        .unwrap()
        .setup_url()
        .unwrap();
    let second = Hub::open(Config::new(data.path()))
        .unwrap()
        .setup_url()
        .unwrap();
    assert_ne!(first, second);
    let connection = rusqlite::Connection::open(kelpie_hub::database_path(data.path())).unwrap();
    let held: u64 = connection
        .query_row(
            "SELECT count(*) FROM invites WHERE kind = 'owner'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(held, 1, "the earlier link stopped working");
    let test = start_hub(|_| {}).await;
    member(&test).await;
    let reopened = Hub::open(Config::new(test.data.path())).unwrap();
    assert_eq!(reopened.setup_url(), None);
}

#[tokio::test]
async fn the_owner_hands_out_invite_links() {
    let test = start_hub(|_| {}).await;
    let mut owner = owner(&test).await;
    let link: InviteLink = owner.post("/v1/a/admin/invites", &()).await.unwrap();
    assert_eq!(
        link.url,
        format!("http://localhost:8470/signup?invite={}", link.code)
    );
    let mut invited = browser(&test);
    let created = invited.sign_up_with_invite(&link.code).await.unwrap();
    assert!(!created.owner);
    let mut again = browser(&test);
    assert_eq!(
        hub_code(again.sign_up_with_invite(&link.code).await.unwrap_err()).0,
        401
    );
    let mut member = invited;
    let refused = member
        .post::<_, InviteLink>("/v1/a/admin/invites", &())
        .await;
    assert_eq!(
        hub_code(refused.unwrap_err()),
        (403, "not-owner".to_string())
    );
}

#[tokio::test]
async fn open_sign_up_verifies_the_email_by_a_link_first() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let mut person = browser(&test);
    person.request_email(" Carl@Example.com ").await.unwrap();
    let sent = test.hub.mail_log();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "carl@example.com");
    let link = sent[0]
        .text
        .split_whitespace()
        .find(|word| word.starts_with("http://localhost:8470/signup?verify=khe_"))
        .unwrap()
        .to_string();
    let token = link.split_once("verify=").unwrap().1;
    let shown: VerifiedEmail = person
        .get(&format!("/v1/a/signup/verify?token={token}"))
        .await
        .unwrap();
    assert_eq!(shown.email, "carl@example.com");
    assert!(!holds(&database_bytes(&test), token.as_bytes()));

    let created = person.sign_up_verified(&link).await.unwrap();
    assert_eq!(created.email.as_deref(), Some("carl@example.com"));
    let verified: Option<u64> = rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .query_row("SELECT email_verified_at FROM accounts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(verified.is_some());
    let mut reuse = browser(&test);
    assert_eq!(
        hub_code(reuse.sign_up_verified(&link).await.unwrap_err()).0,
        401
    );

    let mut again = browser(&test);
    again.request_email("carl@example.com").await.unwrap();
    assert_eq!(
        test.hub.mail_log().len(),
        1,
        "a held address gets no second mail"
    );
    let refused = again.request_email("not an address").await;
    assert_eq!(
        hub_code(refused.unwrap_err()),
        (400, "bad-email".to_string())
    );
}

#[tokio::test]
async fn an_invite_hub_takes_no_email_sign_ups() {
    let test = start_hub(|_| {}).await;
    let mut person = browser(&test);
    let refused = person.request_email("carl@example.com").await;
    assert_eq!(
        hub_code(refused.unwrap_err()),
        (403, "signup-closed".to_string())
    );
    assert!(test.hub.mail_log().is_empty());
}

#[tokio::test]
async fn sign_up_mail_is_rate_limited_per_address() {
    let test = start_hub(|config| config.signup = SignupMode::Open).await;
    let mut person = browser(&test);
    for count in 0..3 {
        person
            .request_email(&format!("person{count}@example.com"))
            .await
            .unwrap();
    }
    let refused = person.request_email("person9@example.com").await;
    assert_eq!(hub_code(refused.unwrap_err()).0, 429);
}

async fn fake_smtp() -> (u16, oneshot::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done, transcript) = oneshot::channel();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        let mut seen = Vec::new();
        write.write_all(b"220 fake ready\r\n").await.unwrap();
        let mut in_data = false;
        while let Some(line) = lines.next_line().await.unwrap() {
            seen.push(line.clone());
            let reply: &[u8] = if in_data {
                if line != "." {
                    continue;
                }
                in_data = false;
                b"250 queued\r\n"
            } else if line.starts_with("EHLO") {
                b"250-fake\r\n250 AUTH PLAIN\r\n"
            } else if line.starts_with("AUTH PLAIN") {
                b"235 ok\r\n"
            } else if line == "DATA" {
                in_data = true;
                b"354 go on\r\n"
            } else if line == "QUIT" {
                write.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 ok\r\n"
            };
            write.write_all(reply).await.unwrap();
        }
        let _ = done.send(seen);
    });
    (port, transcript)
}

#[tokio::test]
async fn sign_up_mail_goes_out_over_smtp() {
    let (port, transcript) = fake_smtp().await;
    let test = start_hub(|config| {
        config.signup = SignupMode::Open;
        config.mail = MailSettings::Smtp(SmtpSettings {
            host: "127.0.0.1".to_string(),
            port,
            security: SmtpSecurity::Plain,
            user: Some("hub".to_string()),
            password: Some("secret".to_string()),
            from: "hub@example.com".to_string(),
        });
    })
    .await;
    browser(&test)
        .request_email("carl@example.com")
        .await
        .unwrap();
    let seen = transcript.await.unwrap();
    assert_eq!(seen[0], "EHLO localhost");
    assert!(
        seen.contains(&"AUTH PLAIN AGh1YgBzZWNyZXQ=".to_string()),
        "{seen:?}"
    );
    assert!(seen.contains(&"MAIL FROM:<hub@example.com>".to_string()));
    assert!(seen.contains(&"RCPT TO:<carl@example.com>".to_string()));
    assert!(seen.contains(&"Subject: Finish signing up to kelpie hub".to_string()));
    assert!(
        seen.iter()
            .any(|line| line.starts_with("http://localhost:8470/signup?verify=khe_"))
    );
    assert_eq!(seen.last().map(String::as_str), Some("QUIT"));
    assert!(test.hub.mail_log().is_empty());
}

#[test]
fn plain_smtp_is_refused_beyond_loopback() {
    let data = tempfile::tempdir().unwrap();
    let mut config = Config::new(data.path());
    config.mail = MailSettings::Smtp(SmtpSettings {
        host: "smtp.example.com".to_string(),
        port: 25,
        security: SmtpSecurity::Plain,
        user: Some("hub".to_string()),
        password: Some("secret".to_string()),
        from: "hub@example.com".to_string(),
    });
    let refused = Hub::open(config).err().unwrap().to_string();
    assert!(refused.contains("plain SMTP"), "{refused}");
}
