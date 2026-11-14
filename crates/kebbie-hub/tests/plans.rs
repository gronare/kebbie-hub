#[allow(dead_code)]
mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use common::*;
use kebbie_hub::store::Store;
use kebbie_hub::{Gated, Plans, Refusal};
use kebbie_hub_api::{HubPlan, PLAN_INACTIVE, PRO_REQUIRED, PlanState};
use kebbie_hub_fakes::HubError;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{ConnectError, ConnectPolled, MachineState, connect};
use kebbie_hub_protocol::{Answer, Kind, Payload, PushApproval};

const BILLING: &str = "https://example.com/pro";
const WEEK: u64 = 7 * 86_400;

#[derive(Default)]
struct Policy {
    pro: AtomicBool,
    closed: AtomicBool,
    seen: Mutex<Vec<Gated>>,
}

fn machines(store: &Store, account: &str) -> u64 {
    store.connection().query_row("SELECT count(*) FROM machines WHERE account_id = ?1", [account], |row| row.get(0)).unwrap()
}

struct Shared(Arc<Policy>);

impl Plans for Shared {
    fn check(&self, store: &Store, account: &str, gated: Gated) -> Result<(), Refusal> {
        self.0.seen.lock().unwrap().push(gated);
        if self.0.closed.load(Ordering::SeqCst) {
            return Err(Refusal { url: Some(BILLING.to_string()), pro: false });
        }
        let free = match gated {
            Gated::Command(kind) => matches!(kind, Kind::Answer | Kind::PromptAnswer | Kind::ItemAnswer | Kind::ReviewOpen | Kind::ReviewRead | Kind::DeviceDecision),
            Gated::Machine => machines(store, account) == 0,
            Gated::Device | Gated::Notifications => true,
        };
        if free || self.0.pro.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(Refusal { url: Some(BILLING.to_string()), pro: true })
        }
    }

    fn plan(&self, _: &Store, _: &str) -> Option<HubPlan> {
        let state = if self.0.pro.load(Ordering::SeqCst) { PlanState::Trial { until: 4_000_000_000 } } else { PlanState::Free };
        Some(HubPlan { state, pro: vec!["push".to_string()], machines: Some(1), trial_days: 14, billing: Some(BILLING.to_string()) })
    }

    fn history(&self, _: &Store, _: &str) -> Option<u64> {
        (!self.0.pro.load(Ordering::SeqCst)).then_some(WEEK)
    }
}

async fn planned() -> (TestHub, Arc<Policy>) {
    let policy = Arc::new(Policy::default());
    let test = start_hub_with(|config| config.connect_interval = 0, Some(Box::new(Shared(Arc::clone(&policy)))), Router::new()).await;
    (test, policy)
}

async fn commanding(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    (machine, person)
}

fn answer() -> Payload {
    Payload::Answer(Answer { address: "refunds@shop".to_string(), question: "1761069540-4411-0-question".to_string(), choice: Some(0), comment: String::new() })
}

fn push() -> Payload {
    Payload::Push(PushApproval { address: "refunds@shop".to_string(), head: "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a".to_string(), card_sha: None })
}

fn code_of(error: HubError) -> (u16, String, Option<String>) {
    let url = match &error {
        HubError::ProRequired { url } | HubError::CommandsRefused { url } => url.clone(),
        _ => None,
    };
    (error.status().unwrap_or_default(), error.code().unwrap_or_default().to_string(), url)
}

#[tokio::test]
async fn the_gate_sees_each_commands_kind_and_refuses_only_the_pro_ones() {
    let (test, policy) = planned().await;
    let (machine, mut person) = commanding(&test).await;
    person.command(&machine.machine, &answer()).await.unwrap();
    let refused = person.command(&machine.machine, &push()).await.unwrap_err();
    assert_eq!(code_of(refused), (402, PRO_REQUIRED.to_string(), Some(BILLING.to_string())));
    assert!(policy.seen.lock().unwrap().ends_with(&[Gated::Command(Kind::Answer), Gated::Command(Kind::Push)]));

    policy.pro.store(true, Ordering::SeqCst);
    person.command(&machine.machine, &push()).await.unwrap();

    policy.closed.store(true, Ordering::SeqCst);
    let closed = person.command(&machine.machine, &answer()).await.unwrap_err();
    assert_eq!(code_of(closed), (402, PLAN_INACTIVE.to_string(), Some(BILLING.to_string())), "a refusal that is not about Pro keeps the code kebbie matches");
}

#[tokio::test]
async fn a_second_machine_is_refused_at_approval_and_the_waiting_machine_hears_it_when_the_person_stops() {
    let (test, policy) = planned().await;
    let mut person = member(&test).await;
    connected(&test, &mut person).await;

    let mut second = connect(&test.url, "devbox").await.unwrap();
    let refused = person.approve_connect(&second.link(), false).await.err().unwrap();
    assert_eq!(code_of(refused), (402, PRO_REQUIRED.to_string(), Some(BILLING.to_string())));
    assert!(matches!(second.poll().await.unwrap(), ConnectPolled::Pending), "the machine keeps waiting while the person decides");
    person.stop_connect(&second.started.user_code, true).await.unwrap();
    match second.poll().await {
        Err(ConnectError::Hub(error)) => assert_eq!(code_of(error), (402, PRO_REQUIRED.to_string(), Some(BILLING.to_string()))),
        other => panic!("the machine heard {:?}", other.err()),
    }

    let cancelled = connect(&test.url, "devbox").await.unwrap();
    person.stop_connect(&cancelled.started.user_code, false).await.unwrap();
    assert!(matches!(cancelled.wait(WAIT).await, Err(ConnectError::Denied)), "a plain cancel stays a cancel");

    let later = connect(&test.url, "devbox").await.unwrap();
    assert_eq!(code_of(person.approve_connect(&later.link(), false).await.err().unwrap()).1, PRO_REQUIRED);
    policy.pro.store(true, Ordering::SeqCst);
    person.approve_connect(&later.link(), false).await.unwrap();
    later.wait(WAIT).await.unwrap();
    assert_eq!(person.boards().await.unwrap().len(), 2, "the same request goes through once the plan includes it");
}

#[tokio::test]
async fn a_stop_for_pro_reads_as_a_cancel_when_the_plan_takes_the_machine_after_all() {
    let (test, policy) = planned().await;
    let mut person = member(&test).await;
    connected(&test, &mut person).await;
    let waiting = connect(&test.url, "devbox").await.unwrap();
    person.stop_connect(&waiting.started.user_code, true).await.unwrap();
    policy.pro.store(true, Ordering::SeqCst);
    assert!(matches!(waiting.wait(WAIT).await, Err(ConnectError::Denied)));
}

#[tokio::test]
async fn the_account_view_carries_the_policys_plan_and_a_hub_without_plans_has_none() {
    let (test, policy) = planned().await;
    let mut person = member(&test).await;
    let plan = person.account().await.unwrap().plan.unwrap();
    assert_eq!((plan.state, plan.pro, plan.machines, plan.trial_days), (PlanState::Free, vec!["push".to_string()], Some(1), 14));
    policy.pro.store(true, Ordering::SeqCst);
    assert_eq!(person.account().await.unwrap().plan.unwrap().state, PlanState::Trial { until: 4_000_000_000 });

    let open = start_hub(|_| {}).await;
    let mut someone = member(&open).await;
    assert_eq!(someone.account().await.unwrap().plan, None);
    let text = someone.get::<serde_json::Value>("/v1/a/account").await.unwrap();
    assert!(text.get("plan").is_none(), "a hub without plans sends no plan field: {text}");
}

#[tokio::test]
async fn a_devices_history_reaches_as_far_back_as_the_plan_says() {
    let (test, policy) = planned().await;
    let (machine, mut person) = commanding(&test).await;
    person.command(&machine.machine, &answer()).await.unwrap();
    person.command(&machine.machine, &answer()).await.unwrap();
    let device = person.device_cert().unwrap().device.clone();
    rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .execute("UPDATE commands SET queued_at = queued_at - ?1 WHERE rowid = (SELECT min(rowid) FROM commands)", [WEEK + 60])
        .unwrap();
    assert_eq!(person.device_commands(&device, None).await.unwrap().commands.len(), 1, "a week back on the free plan");
    policy.pro.store(true, Ordering::SeqCst);
    assert_eq!(person.device_commands(&device, None).await.unwrap().commands.len(), 2);
}
