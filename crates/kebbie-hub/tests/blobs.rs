#[allow(dead_code)]
mod common;

use common::*;
use kebbie_hub::store::BLOB_KEEP_SECONDS;
use kebbie_hub_fakes::{HubClient, now};
use kebbie_hub_protocol::{BlobRef, Id, MAX_BLOB_BYTES, open_blob, seal_blob};

const PLAN: &str = "## Steps\n\n1. Book the refund on okapi-lantern-55c1 confirmation.\n";

fn with_plan(blob: &BlobRef) -> kebbie_hub_protocol::Board {
    let mut board = board(MARKER);
    board.items[0].question.as_mut().unwrap().plan = Some(blob.clone());
    board
}

#[tokio::test]
async fn a_plan_reaches_the_account_sealed_and_the_hub_never_holds_its_text() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    let (blob, ciphertext) = seal_blob(PLAN.as_bytes(), "text/markdown");
    machine.put_blob(&blob.id, ciphertext.clone()).await.unwrap();
    machine.put_blob(&blob.id, ciphertext.clone()).await.unwrap();
    machine.post_board(with_plan(&blob)).await.unwrap();
    assert_eq!(rows(&test, "blobs"), 1, "a second PUT of the same blob stores nothing new");

    let boards = person.boards().await.unwrap();
    let read = boards[0].board.as_ref().unwrap();
    let plan = read.items[0].question.as_ref().unwrap().plan.clone().unwrap();
    let fetched = person.blob(&plan.id).await.unwrap();
    assert_eq!(fetched, ciphertext);
    assert_eq!(open_blob(&plan, &fetched).unwrap(), PLAN.as_bytes());

    let stored = database_bytes(&test);
    assert!(holds(&stored, &ciphertext), "the hub keeps the ciphertext");
    assert!(!holds(&stored, b"okapi-lantern-55c1"), "the plan's text never reaches the hub's files");
    assert!(!holds(&stored, blob.key.as_bytes()), "nor the blob's key");
}

#[tokio::test]
async fn another_account_and_a_signed_out_browser_never_get_the_blob() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let machine = connected_with_board(&test, &mut person).await;
    let (blob, ciphertext) = seal_blob(PLAN.as_bytes(), "text/markdown");
    machine.put_blob(&blob.id, ciphertext).await.unwrap();

    let mut stranger = member(&test).await;
    let _theirs = connected_with_board(&test, &mut stranger).await;
    assert_eq!(status_of(stranger.blob(&blob.id).await), 404);
    assert_eq!(status_of(browser(&test).blob(&blob.id).await), 401);
    let unknown = Id::blob(b"never uploaded");
    assert_eq!(status_of(person.blob(&unknown).await), 404);
    assert_eq!(person.blob(&blob.id).await.unwrap().len(), PLAN.len() + kebbie_hub_protocol::BLOB_OVERHEAD);
}

#[tokio::test]
async fn an_upload_must_be_named_by_its_own_hash_and_fit_the_cap() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let machine = connected_with_board(&test, &mut person).await;
    let (blob, ciphertext) = seal_blob(PLAN.as_bytes(), "text/markdown");
    let (other, _) = seal_blob(b"another plan", "text/markdown");

    let refused = machine.put_blob(&other.id, ciphertext.clone()).await.unwrap_err();
    assert_eq!(refused.to_string(), "the hub answered 400 id-mismatch");
    let client = machine.client();
    assert_eq!(status_of(client.put_bytes("/v1/m/blobs/brd_01HZX3K1B6V2W5Y8Z0A3C6E9GJ", ciphertext.clone()).await), 400);
    let huge = vec![7u8; MAX_BLOB_BYTES + 1];
    assert_eq!(status_of(client.put_bytes(&format!("/v1/m/blobs/{}", Id::blob(&huge)), huge).await), 413);
    let anonymous = HubClient::new(&test.url, None);
    assert_eq!(status_of(anonymous.put_bytes(&format!("/v1/m/blobs/{}", blob.id), ciphertext).await), 401);
    assert_eq!(rows(&test, "blobs"), 0);
}

#[tokio::test]
async fn a_blob_lives_two_weeks_after_its_machine_last_put_it() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let machine = connected_with_board(&test, &mut person).await;
    let (blob, ciphertext) = seal_blob(PLAN.as_bytes(), "text/markdown");
    machine.put_blob(&blob.id, ciphertext).await.unwrap();
    let put_at = now();

    test.hub.store().sweep(put_at + BLOB_KEEP_SECONDS - 60).unwrap();
    assert_eq!(rows(&test, "blobs"), 1);
    test.hub.store().sweep(put_at + BLOB_KEEP_SECONDS + 60).unwrap();
    assert_eq!(rows(&test, "blobs"), 0);
}
