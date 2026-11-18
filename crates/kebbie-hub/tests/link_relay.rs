#[allow(dead_code)]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use common::*;
use kebbie_hub::store::Store;
use kebbie_hub::{Gated, Refusal};
use futures_util::{SinkExt, StreamExt};
use kebbie_hub_fakes::machine::MachineState;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn open(test: &TestHub, machine: &MachineState, query: &str) -> Result<Socket, u16> {
    let mut request = format!("{}/v1/m/link?{query}", test.url.replace("http://", "ws://")).into_client_request().unwrap();
    request.headers_mut().insert("authorization", format!("Bearer {}", machine.token()).parse().unwrap());
    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => Ok(socket),
        Err(WsError::Http(response)) => Err(response.status().as_u16()),
        Err(error) => panic!("{error}"),
    }
}

async fn ring(listening: &mut Socket) -> (String, String) {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), listening.next()).await.unwrap().unwrap().unwrap() {
            Message::Text(text) => {
                let rung: serde_json::Value = serde_json::from_str(&text).unwrap();
                return (rung["session"].as_str().unwrap().to_string(), rung["from"].as_str().unwrap().to_string());
            }
            _ => continue,
        }
    }
}

async fn binary(socket: &mut Socket) -> Option<Vec<u8>> {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), socket.next()).await.unwrap()? {
            Ok(Message::Binary(bytes)) => return Some(bytes.to_vec()),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

// The asker's socket opens only once the worker answered the ring, so it runs as its own task.
async fn linked(test: &TestHub, asker: &MachineState, worker: &MachineState, listening: &mut Socket) -> (Socket, Socket) {
    let query = format!("to={}", worker.machine);
    let (url, token) = (test.url.clone(), asker.token().to_string());
    let asking = tokio::spawn(async move {
        let mut request = format!("{}/v1/m/link?{query}", url.replace("http://", "ws://")).into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        tokio_tungstenite::connect_async(request).await.map(|(socket, _)| socket)
    });
    let (session, from) = ring(listening).await;
    assert_eq!(from, asker.machine.to_string(), "the ring names the asker");
    let answered = open(test, worker, &format!("accept={session}")).await.unwrap();
    (asking.await.unwrap().unwrap(), answered)
}

#[tokio::test]
async fn the_hub_passes_a_links_bytes_between_two_machines_of_one_account_both_ways() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let devbox = connected_with_board(&test, &mut person).await;
    let studio = connected_with_board(&test, &mut person).await;
    assert_eq!(open(&test, &studio, "listen=true").await.err(), Some(409), "only a worker waits for links");
    studio.set_worker("macos").await.unwrap();
    assert_eq!(open(&test, &devbox, &format!("to={}", studio.machine)).await.err(), Some(409), "a worker that is not listening is not linked");
    let mut listening = open(&test, &studio, "listen=true").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut asker, mut answered) = linked(&test, &devbox, &studio, &mut listening).await;
    asker.send(Message::Binary(b"client hello".to_vec().into())).await.unwrap();
    assert_eq!(binary(&mut answered).await.as_deref(), Some(b"client hello".as_slice()));
    answered.send(Message::Binary(vec![7u8; 600 * 1024].into())).await.unwrap();
    assert_eq!(binary(&mut asker).await.map(|bytes| bytes.len()), Some(600 * 1024));
    assert_eq!(test.hub.open_links(&devbox_account(&test, &devbox)), 1);
    asker.close(None).await.unwrap();
    assert_eq!(binary(&mut answered).await, None, "one side closing closes the other");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(test.hub.open_links(&devbox_account(&test, &devbox)), 0);
    assert!(test.hub.store().events().unwrap().iter().any(|(kind, detail)| kind == "link.relayed" && detail.as_deref().is_some_and(|detail| detail.contains("bytes=614412"))), "the hub logs how much it carried");
    assert_eq!(open(&test, &devbox, "accept=0123456789abcdef0123456789abcdef").await.err(), Some(404));
}

fn devbox_account(test: &TestHub, machine: &MachineState) -> String {
    test.hub.store().machine_owner(&machine.machine).unwrap().unwrap()
}

#[tokio::test]
async fn the_relay_never_links_machines_of_different_accounts_nor_answers_for_another_machine() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let devbox = connected_with_board(&test, &mut person).await;
    let studio = connected_with_board(&test, &mut person).await;
    let laptop = connected_with_board(&test, &mut person).await;
    let mut stranger = member(&test).await;
    let theirs = connected(&test, &mut stranger).await;
    theirs.set_worker("linux").await.unwrap();
    let _their_listening = open(&test, &theirs, "listen=true").await.unwrap();
    studio.set_worker("macos").await.unwrap();
    let mut listening = open(&test, &studio, "listen=true").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(open(&test, &devbox, &format!("to={}", theirs.machine)).await.err(), Some(404), "a machine of another account is unknown");
    assert_eq!(open(&test, &theirs, &format!("to={}", studio.machine)).await.err(), Some(404), "and so is this account's machine to theirs");
    assert_eq!(open(&test, &devbox, &format!("to={}", devbox.machine)).await.err(), Some(404), "no link to itself");

    let query = format!("to={}", studio.machine);
    let (url, token) = (test.url.clone(), devbox.token().to_string());
    let asking = tokio::spawn(async move {
        let mut request = format!("{}/v1/m/link?{query}", url.replace("http://", "ws://")).into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        tokio_tungstenite::connect_async(request).await.map(|(socket, _)| socket)
    });
    let (session, _) = ring(&mut listening).await;
    assert_eq!(open(&test, &laptop, &format!("accept={session}")).await.err(), Some(404), "only the machine rung answers");
    assert_eq!(open(&test, &theirs, &format!("accept={session}")).await.err(), Some(404));
    assert!(open(&test, &studio, &format!("accept={session}")).await.is_ok());
    assert!(asking.await.unwrap().is_ok());
}

#[tokio::test]
async fn a_link_ends_at_its_byte_limit_and_an_unanswered_ring_is_refused() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_max_bytes = 1024;
        config.relay_answer_wait = Duration::from_millis(300);
    })
    .await;
    let mut person = member(&test).await;
    let devbox = connected_with_board(&test, &mut person).await;
    let studio = connected_with_board(&test, &mut person).await;
    studio.set_worker("macos").await.unwrap();
    let mut listening = open(&test, &studio, "listen=true").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut asker, mut answered) = linked(&test, &devbox, &studio, &mut listening).await;
    asker.send(Message::Binary(vec![1u8; 1000].into())).await.unwrap();
    assert_eq!(binary(&mut answered).await.map(|bytes| bytes.len()), Some(1000));
    asker.send(Message::Binary(vec![1u8; 100].into())).await.unwrap();
    assert_eq!(binary(&mut answered).await, None, "past its byte limit the hub closes the link");
    assert_eq!(open(&test, &devbox, &format!("to={}", studio.machine)).await.err(), Some(409), "a worker that does not answer within the wait is no link");
}

async fn refused_with(test: &TestHub, machine: &MachineState, query: &str) -> (u16, String) {
    let mut request = format!("{}/v1/m/link?{query}", test.url.replace("http://", "ws://")).into_client_request().unwrap();
    request.headers_mut().insert("authorization", format!("Bearer {}", machine.token()).parse().unwrap());
    match tokio_tungstenite::connect_async(request).await {
        Err(WsError::Http(response)) => {
            let body: serde_json::Value = serde_json::from_slice(response.body().as_deref().unwrap_or_default()).unwrap_or_default();
            (response.status().as_u16(), body["error"].as_str().unwrap_or_default().to_string())
        }
        other => panic!("the hub opened the link: {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn the_relay_passes_the_plans_gate_and_with_no_plans_it_is_open() {
    let refusing = Arc::new(AtomicBool::new(true));
    let gate = refusing.clone();
    let refuse_relay = move |_: &Store, _: &str, gated: Gated| match gated {
        Gated::Relay if gate.load(Ordering::SeqCst) => Err(Refusal { url: Some("https://example.com/pro".into()), pro: true }),
        _ => Ok(()),
    };
    let test = start_hub_with(|config| config.connect_interval = 0, Some(Box::new(refuse_relay)), Router::new()).await;
    let mut person = member(&test).await;
    let devbox = connected_with_board(&test, &mut person).await;
    let studio = connected_with_board(&test, &mut person).await;
    studio.set_worker("macos").await.unwrap();
    assert_eq!(refused_with(&test, &studio, "listen=true").await, (402, kebbie_hub_api::PRO_REQUIRED.to_string()), "a plan without the relay refuses the wait");
    assert_eq!(refused_with(&test, &devbox, &format!("to={}", studio.machine)).await, (402, kebbie_hub_api::PRO_REQUIRED.to_string()), "and the ask");
    refusing.store(false, Ordering::SeqCst);
    let mut listening = open(&test, &studio, "listen=true").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (mut asker, mut answered) = linked(&test, &devbox, &studio, &mut listening).await;
    asker.send(Message::Binary(b"hello".to_vec().into())).await.unwrap();
    assert_eq!(binary(&mut answered).await.as_deref(), Some(b"hello".as_slice()));

    let open_hub = start_hub(|config| config.connect_interval = 0).await;
    assert!(!open_hub.hub.has_plans());
    let mut person = member(&open_hub).await;
    let devbox = connected_with_board(&open_hub, &mut person).await;
    let studio = connected_with_board(&open_hub, &mut person).await;
    studio.set_worker("linux").await.unwrap();
    let mut listening = open(&open_hub, &studio, "listen=true").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    linked(&open_hub, &devbox, &studio, &mut listening).await;
}

fn ended(test: &TestHub) -> Vec<String> {
    test.hub.store().events().unwrap().into_iter().filter(|(kind, _)| kind == "link.relayed").filter_map(|(_, detail)| detail?.split("ended=").nth(1).map(str::to_string)).collect()
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn pair(test: &TestHub) -> (MachineState, MachineState, Socket) {
    let mut person = member(test).await;
    let devbox = connected_with_board(test, &mut person).await;
    let studio = connected_with_board(test, &mut person).await;
    studio.set_worker("macos").await.unwrap();
    let listening = open(test, &studio, "listen=true").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    (devbox, studio, listening)
}

#[tokio::test]
async fn a_peer_that_stops_reading_ends_its_link_and_frees_its_place() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_send_wait = Duration::from_millis(200);
    })
    .await;
    let (devbox, studio, mut listening) = pair(&test).await;
    let (mut asker, _stalled) = linked(&test, &devbox, &studio, &mut listening).await;
    let account = devbox_account(&test, &devbox);
    let flooding = tokio::spawn(async move {
        for _ in 0..256 {
            if asker.send(Message::Binary(vec![0u8; 1024 * 1024].into())).await.is_err() {
                break;
            }
        }
    });
    until("a worker that reads nothing stalls the link until the hub ends it", || ended(&test).contains(&"stalled".to_string())).await;
    assert_eq!(test.hub.open_links(&account), 0);
    flooding.abort();
}

#[tokio::test]
async fn only_relayed_bytes_keep_a_link_open_and_a_text_message_ends_it() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_idle = Duration::from_millis(300);
    })
    .await;
    let (devbox, studio, mut listening) = pair(&test).await;
    let (mut asker, _answered) = linked(&test, &devbox, &studio, &mut listening).await;
    for _ in 0..8 {
        let _ = asker.send(Message::Ping(Vec::new().into())).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(ended(&test), ["idle"], "pings are not traffic");
    let (mut asker, mut answered) = linked(&test, &devbox, &studio, &mut listening).await;
    asker.send(Message::Text("hello".into())).await.unwrap();
    assert_eq!(binary(&mut answered).await, None);
    until("a text message ends the link", || ended(&test).len() == 2).await;
    assert_eq!(ended(&test)[1], "text");
}

#[tokio::test]
async fn an_asker_that_leaves_during_the_ring_leaves_no_waiting_session() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_answer_wait = Duration::from_secs(5);
    })
    .await;
    let (devbox, studio, mut listening) = pair(&test).await;
    let query = format!("to={}", studio.machine);
    let (url, token) = (test.url.clone(), devbox.token().to_string());
    let asking = tokio::spawn(async move {
        let mut request = format!("{}/v1/m/link?{query}", url.replace("http://", "ws://")).into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        tokio_tungstenite::connect_async(request).await.map(|(socket, _)| socket)
    });
    let (session, _) = ring(&mut listening).await;
    assert_eq!(test.hub.relay_rings_waiting(), 1);
    asking.abort();
    until("the hub forgets the ring of an asker that is gone", || test.hub.relay_rings_waiting() == 0).await;
    assert_eq!(open(&test, &studio, &format!("accept={session}")).await.err(), Some(404));
}

#[tokio::test]
async fn removing_a_machine_ends_its_link_and_its_wait() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_check = Duration::from_millis(100);
    })
    .await;
    let (devbox, studio, mut listening) = pair(&test).await;
    let (_asker, mut answered) = linked(&test, &devbox, &studio, &mut listening).await;
    assert!(test.hub.store().remove_machine(&devbox.machine).unwrap());
    assert_eq!(binary(&mut answered).await, None, "the link of a removed asker ends");
    until("the link is logged as removed", || ended(&test) == ["removed"]).await;
    assert!(test.hub.store().remove_machine(&studio.machine).unwrap());
    assert_eq!(binary(&mut listening).await, None, "and a removed worker's wait ends");
}

#[tokio::test]
async fn a_waiting_worker_that_answers_no_pings_is_let_go() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_ping = Duration::from_millis(100);
    })
    .await;
    let (devbox, studio, _silent) = pair(&test).await;
    assert_eq!(test.hub.relay_sockets(), 1);
    until("a wait that misses two pings ends", || test.hub.relay_sockets() == 0).await;
    assert_eq!(open(&test, &devbox, &format!("to={}", studio.machine)).await.err(), Some(409));
}

#[tokio::test]
async fn the_relay_holds_at_most_its_sockets_across_the_hub() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.relay_max_sockets = 1;
    })
    .await;
    let (devbox, studio, _listening) = pair(&test).await;
    devbox.set_worker("linux").await.unwrap();
    assert_eq!(open(&test, &devbox, "listen=true").await.err(), Some(503));
    assert_eq!(open(&test, &devbox, &format!("to={}", studio.machine)).await.err(), Some(503));
}
