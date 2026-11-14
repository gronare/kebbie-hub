use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::board::BlobRef;
use crate::ids::{Id, Prefix};
use crate::message::{check, is_collapse, is_text};

pub const LEAD_THREAD_MEDIA_TYPE: &str = "application/vnd.kebbie.lead-thread+json";
pub const MAX_LEAD_PAGE_TURNS: usize = 50;
pub const MAX_LEAD_TURN_CHARS: usize = 16_000;
pub const MAX_LEAD_MESSAGE_CHARS: usize = 4_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeadRef {
    pub thread: BlobRef,
    pub working_since: Option<u64>,
    pub replied_at: Option<u64>,
}

/// One page of the thread, oldest turn first; `older` is the page before it, sealed the same way, or none when the machine keeps nothing earlier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeadThread {
    pub turns: Vec<Turn>,
    pub older: Option<BlobRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Turn {
    /// What you said: a message, or away mode switched; without a device it was at the computer.
    You {
        id: String,
        at: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        away: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery: Option<Delivery>,
    },
    Lead { id: String, at: u64, text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Delivery {
    WaitingTurn,
    WaitingDraft,
    Delivered { at: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeadMessage {
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Away {
    pub on: bool,
}

impl Turn {
    pub fn id(&self) -> &str {
        match self {
            Turn::You { id, .. } | Turn::Lead { id, .. } => id,
        }
    }

    pub fn at(&self) -> u64 {
        match self {
            Turn::You { at, .. } | Turn::Lead { at, .. } => *at,
        }
    }

    fn check(&self) -> Result<(), &'static str> {
        check(is_collapse(self.id()), "a turn id is 1-64 of A-Z, a-z, 0-9, - and _")?;
        match self {
            Turn::Lead { text, .. } => is_turn_text(text),
            Turn::You { at, device, text, away, delivery, .. } => {
                check(device.as_ref().is_none_or(|device| device.prefix() == Prefix::Device), "device is not a device id")?;
                match (text, away) {
                    (Some(text), None) => is_turn_text(text)?,
                    (None, Some(_)) => check(delivery.is_none(), "away mode switched is not delivered to the lead")?,
                    _ => return Err("your turn says a text or switches away mode, one of them"),
                }
                if text.is_some() {
                    check(device.is_some() == delivery.is_some(), "a message from a device says how it was delivered, one at the computer does not")?;
                }
                match delivery {
                    Some(Delivery::Delivered { at: delivered }) => check(delivered >= at, "a message is delivered after it was written"),
                    _ => Ok(()),
                }
            }
        }
    }
}

fn is_turn_text(text: &str) -> Result<(), &'static str> {
    check(is_text(text, MAX_LEAD_TURN_CHARS), "a turn is empty or too long")?;
    check(!text.chars().any(|char| char.is_control() && !matches!(char, '\n' | '\t')), "a turn holds control characters")
}

impl LeadThread {
    pub fn read(bytes: &[u8]) -> Result<LeadThread, &'static str> {
        let thread: LeadThread = serde_json::from_slice(bytes).map_err(|_| "a page is not a lead thread's JSON")?;
        thread.check()?;
        Ok(thread)
    }

    pub fn check(&self) -> Result<(), &'static str> {
        check(self.turns.len() <= MAX_LEAD_PAGE_TURNS, "a page holds at most 50 turns")?;
        check(self.older.is_none() || !self.turns.is_empty(), "an empty page has nothing before it")?;
        check(self.older.as_ref().is_none_or(|older| older.media_type == LEAD_THREAD_MEDIA_TYPE), "the page before is a lead thread page")?;
        self.turns.iter().try_for_each(Turn::check)?;
        check(self.turns.windows(2).all(|pair| pair[0].at() <= pair[1].at()), "a page lists its turns oldest first")?;
        let ids: BTreeSet<&str> = self.turns.iter().map(Turn::id).collect();
        check(ids.len() == self.turns.len(), "a page names each turn once")
    }
}

impl LeadRef {
    pub fn check(&self) -> Result<(), &'static str> {
        check(self.thread.media_type == LEAD_THREAD_MEDIA_TYPE, "the lead's thread is a lead thread page")
    }
}

impl LeadMessage {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_text(&self.text, MAX_LEAD_MESSAGE_CHARS), "the message is empty or too long")?;
        check(!self.text.chars().any(|char| char.is_control() && !matches!(char, '\n' | '\t')), "the message holds control characters")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> serde_json::Value {
        serde_json::json!({
            "turns": [
                { "by": "you", "id": "t-1", "at": 1_761_060_000u64, "text": "Take refunds through the obstacle flow." },
                { "by": "lead", "id": "t-2", "at": 1_761_060_060u64, "text": "Starting. Tests, lint, then the review." },
                { "by": "you", "id": "cmd_0199a3e0-0000-7000-8000-000000000001", "at": 1_761_062_000u64, "device": "dev_01K7Z0000000000000000000PH", "text": "Do the last-admin subtask first.", "delivery": { "state": "delivered", "at": 1_761_062_030u64 } },
                { "by": "you", "id": "cmd_0199a3e0-0000-7000-8000-000000000002", "at": 1_761_062_100u64, "device": "dev_01K7Z0000000000000000000PH", "text": "And hold the push.", "delivery": { "state": "waiting-turn" } },
                { "by": "you", "id": "cmd_0199a3e0-0000-7000-8000-000000000003", "at": 1_761_062_200u64, "device": "dev_01K7Z0000000000000000000PH", "away": true },
            ],
            "older": null,
        })
    }

    #[test]
    fn a_page_holds_your_turns_and_the_leads_oldest_first() {
        let read = LeadThread::read(page().to_string().as_bytes()).unwrap();
        assert_eq!(read.turns.len(), 5);
        assert!(matches!(read.turns[3], Turn::You { delivery: Some(Delivery::WaitingTurn), .. }));
        assert_eq!(serde_json::to_value(&read).unwrap(), page());
    }

    #[test]
    fn a_page_refuses_what_it_cannot_show() {
        let change = |pointer: &str, value: serde_json::Value| {
            let mut changed = page();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            match changed.pointer_mut(parent).unwrap() {
                serde_json::Value::Array(list) => list[key.parse::<usize>().unwrap()] = value,
                parent => parent[key] = value,
            }
            changed
        };
        let without = |pointer: &str| {
            let mut changed = page();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            changed.pointer_mut(parent).unwrap().as_object_mut().unwrap().remove(key);
            changed
        };
        for (name, wrong) in [
            ("a tool call", change("/turns/1/by", serde_json::json!("tool"))),
            ("an empty reply", change("/turns/1/text", serde_json::json!(" "))),
            ("a terminal escape", change("/turns/1/text", serde_json::json!("done\u{1b}[2J"))),
            ("a reply past its length", change("/turns/1/text", serde_json::json!("x".repeat(MAX_LEAD_TURN_CHARS + 1)))),
            ("turns out of order", change("/turns/1/at", serde_json::json!(1))),
            ("one id twice", change("/turns/1/id", serde_json::json!("t-1"))),
            ("an id with a slash", change("/turns/0/id", serde_json::json!("../t"))),
            ("a phone message without its delivery", without("/turns/2/delivery")),
            ("a computer message with a delivery", change("/turns/0/delivery", serde_json::json!({ "state": "waiting-draft" }))),
            ("delivered before it was written", change("/turns/2/delivery/at", serde_json::json!(1))),
            ("a text and away at once", change("/turns/4/text", serde_json::json!("away"))),
            ("neither text nor away", without("/turns/4/away")),
            ("away delivered", change("/turns/4/delivery", serde_json::json!({ "state": "waiting-turn" }))),
            ("a device that is a machine", change("/turns/2/device", serde_json::json!("mch_01K7Z0000000000000000000PH"))),
            ("a delivery state kebbie does not know", change("/turns/3/delivery/state", serde_json::json!("lost"))),
            ("a lead reply naming a device", change("/turns/1/device", serde_json::json!("dev_01K7Z0000000000000000000PH"))),
            ("an empty page with a page before", serde_json::json!({ "turns": [], "older": { "id": "blb_".to_string() + &"0".repeat(64), "key": "k", "media_type": LEAD_THREAD_MEDIA_TYPE, "bytes": 1 } })),
            ("a field the page does not have", { let mut changed = page(); changed["cursor"] = serde_json::json!(3); changed }),
        ] {
            assert!(LeadThread::read(wrong.to_string().as_bytes()).is_err(), "{name}");
        }
    }
}
