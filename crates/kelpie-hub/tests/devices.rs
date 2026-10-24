#[allow(dead_code)]
mod common;

use common::*;
use kelpie_hub::store::DEVICE_SEEN_EVERY;
use kelpie_hub_fakes::HubClient;
use kelpie_hub_protocol::Id;

fn last_seen(test: &TestHub, machine: &Id, device: &Id) -> Option<u64> {
    test.hub
        .store()
        .admitted_device_views(machine)
        .unwrap()
        .into_iter()
        .find(|view| &view.device == device)
        .and_then(|view| view.last_seen)
}

#[tokio::test]
async fn a_machine_lists_only_the_devices_it_admitted_with_their_name_pairing_last_read_and_notifications()
 {
    let test = start_hub(|_| {}).await;
    let mut mac = joined_machine(&test).await;
    let mut studio = joined_machine(&test).await;
    let mut shared = phone(&test).await;
    let mut own = phone(&test).await;
    let shared_id = pair_both(&mut mac, &mut shared, true).await.0.unwrap();
    pair_both(&mut studio, &mut shared, true).await.0.unwrap();
    let own_id = pair_both(&mut studio, &mut own, true).await.0.unwrap();
    assert_eq!(shared.rename("  Safari on Mac ").await.unwrap(), "Safari on Mac");
    test.hub
        .store()
        .subscribe_push(&shared_id, "https://push.example/one", &[4; 65], &[1; 16], 1)
        .unwrap();

    let listed = mac.listed_devices().await.unwrap();
    assert_eq!(listed.len(), 1, "only the device this machine admitted");
    let device = &listed[0];
    assert_eq!(device.device, shared_id);
    assert_eq!(device.name.as_deref(), Some("Safari on Mac"));
    assert!(device.notifications);
    assert!(device.paired_at > 0 && device.last_seen.is_some());

    let studio_sees: Vec<(Id, bool)> = studio
        .listed_devices()
        .await
        .unwrap()
        .into_iter()
        .map(|view| (view.device, view.notifications))
        .collect();
    assert_eq!(studio_sees, [(shared_id.clone(), true), (own_id.clone(), false)]);

    let raw: serde_json::Value = mac.client().get("/v1/m/devices", &[]).await.unwrap();
    let keys: Vec<&str> = raw["devices"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["device", "last_seen", "name", "notifications", "paired_at"], "nothing about keys, tokens or other machines");

    let anonymous = HubClient::new(&test.url, None);
    assert_eq!(
        status_of(anonymous.get::<serde_json::Value>("/v1/m/devices", &[]).await),
        401
    );
}

#[tokio::test]
async fn a_machine_removing_a_device_keeps_it_on_the_machines_that_admitted_it_too() {
    let test = start_hub(|_| {}).await;
    let mut mac = joined_machine(&test).await;
    let mut studio = joined_machine(&test).await;
    let mut shared = phone(&test).await;
    let device = pair_both(&mut mac, &mut shared, true).await.0.unwrap();
    pair_both(&mut studio, &mut shared, true).await.0.unwrap();
    mac.post_board(board(MARKER)).await.unwrap();
    studio.post_board(board(MARKER)).await.unwrap();
    assert_eq!(shared.boards().await.unwrap().len(), 2);

    mac.remove_device(&device).await.unwrap();
    assert!(mac.listed_devices().await.unwrap().is_empty());
    assert_eq!(studio.listed_devices().await.unwrap()[0].device, device);
    let boards = shared.boards().await.unwrap();
    assert_eq!(boards.len(), 1, "the device still reads the other machine's board");
    assert_eq!(boards[0].machine, studio.machine);
    assert_eq!(
        status_of(
            mac.client()
                .delete(&format!("/v1/m/devices/{device}"))
                .await
        ),
        404,
        "a device this machine no longer admits"
    );
    test.hub.sweep().unwrap();
    assert!(test.hub.store().device_exists(&device).unwrap(), "an account's device outlives its admissions");
}

#[tokio::test]
async fn a_device_renames_itself_with_the_same_check_as_a_machine() {
    let test = start_hub(|_| {}).await;
    let phone = phone(&test).await;
    assert_eq!(phone.rename("iPhone").await.unwrap(), "iPhone");
    for bad in [" ".to_string(), "x".repeat(65), "a\u{7}b".to_string()] {
        let refused = phone
            .client()
            .put::<_, serde_json::Value>("/v1/d/name", &serde_json::json!({ "name": bad }))
            .await;
        assert_eq!(status_of(refused), 400);
    }
}

#[tokio::test]
async fn a_device_is_seen_at_most_once_a_minute() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    let device = pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let store = || test.hub.store();
    let first = last_seen(&test, &machine.machine, &device).unwrap();
    store().touch_device(&device, first + DEVICE_SEEN_EVERY - 1).unwrap();
    assert_eq!(last_seen(&test, &machine.machine, &device), Some(first), "a read within the minute writes nothing");
    store().touch_device(&device, first + DEVICE_SEEN_EVERY).unwrap();
    assert_eq!(last_seen(&test, &machine.machine, &device), Some(first + DEVICE_SEEN_EVERY));
    phone.boards().await.unwrap();
    assert_eq!(
        last_seen(&test, &machine.machine, &device),
        Some(first + DEVICE_SEEN_EVERY),
        "a board read inside the minute leaves it"
    );
}
