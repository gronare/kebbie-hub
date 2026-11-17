#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
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
