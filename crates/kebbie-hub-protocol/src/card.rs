use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::backlog::{HomeKind, ItemKey};
use crate::board::{DecidedBy, is_subtask_id};
use crate::message::{check, is_line};

pub const CARD_MEDIA_TYPE: &str = "application/vnd.kebbie.card+json";
pub const MAX_BUILT_LINES: usize = 6;
pub const MAX_CARD_LINE_CHARS: usize = 600;
pub const MAX_DECISIONS: usize = 64;
pub const MAX_DECISION_CHARS: usize = 1_000;
pub const MAX_NOTICED: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_at: Option<u64>,
    pub built: Vec<String>,
    pub decisions: Vec<CardDecision>,
    pub noticed: Vec<Noticed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Box<Card>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardDecision {
    pub text: String,
    pub by: DecidedBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<DecisionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionSource {
    Question,
    Method,
    Obstacle,
    Plan,
    Message,
    ThroughLead,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Noticed {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<ItemKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<NoticedChoice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticedChoice {
    Subtask,
    Task,
    Kept,
}

impl Card {
    pub fn read(bytes: &[u8]) -> Result<Card, &'static str> {
        let card: Card = serde_json::from_slice(bytes).map_err(|_| "a card is not a card's JSON")?;
        card.check()?;
        Ok(card)
    }

    pub fn check(&self) -> Result<(), &'static str> {
        self.check_lines()?;
        let Some(previous) = &self.previous else {
            return Ok(());
        };
        check(self.changed_at.is_some(), "a card that changed says when")?;
        check(previous.previous.is_none(), "a card carries one version before it, not its whole history")?;
        check(previous.version < self.version, "the version before a card is an earlier one")?;
        previous.check_lines()
    }

    fn check_lines(&self) -> Result<(), &'static str> {
        check(self.version >= 1, "card versions count from one")?;
        check(self.built.len() <= MAX_BUILT_LINES, "a card's Built holds at most six lines")?;
        check(self.built.iter().all(|line| is_line(line, MAX_CARD_LINE_CHARS)), "a Built line is empty, too long or not one line")?;
        check(self.decisions.len() <= MAX_DECISIONS, "a card lists at most 64 decisions")?;
        for (index, decision) in self.decisions.iter().enumerate() {
            check(is_line(&decision.text, MAX_DECISION_CHARS), "a decision is empty, too long or not one line")?;
            check(decision.subtask.as_deref().is_none_or(is_subtask_id), "a decision's subtask is named by its id")?;
            let later = |replacement: &u32| (*replacement as usize) > index && (*replacement as usize) < self.decisions.len();
            check(decision.replaced_by.as_ref().is_none_or(later), "a decision is replaced by a later decision on the same card")?;
        }
        check(self.noticed.len() <= MAX_NOTICED, "a card lists at most 32 things noticed")?;
        for noticed in &self.noticed {
            check(is_line(&noticed.text, MAX_CARD_LINE_CHARS), "a noticed line is empty, too long or not one line")?;
            check(noticed.found.as_ref().is_none_or(|key| key.home() == HomeKind::Found), "a noticed line names an item tasks found")?;
            check(noticed.chosen.is_none() || noticed.found.is_some(), "only a filed noticed line has a choice made on it")?;
        }
        let filed: Vec<&ItemKey> = self.noticed.iter().filter_map(|noticed| noticed.found.as_ref()).collect();
        check(filed.iter().collect::<BTreeSet<_>>().len() == filed.len(), "a card names each found item once")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> serde_json::Value {
        serde_json::json!({
            "version": 2,
            "changed_at": 1_761_069_600u64,
            "built": ["`mindin lint` reads the index and answers in under a second"],
            "decisions": [
                { "text": "Frontmatter is stored in the index", "by": "investigator", "source": "obstacle", "at": 1_761_060_000u64, "subtask": "index" },
                { "text": "Keep the lint in one module", "by": "lead", "source": "method", "replaced_by": 2 },
                { "text": "Split notes.py into four modules", "by": "you", "source": "plan" },
                { "text": "Narrower except around the probe", "by": "task" },
            ],
            "noticed": [
                { "text": "The remote client can't tell a missing log from an outage", "found": "found:f3" },
                { "text": "`keep_an_admin` writes without a locked transaction", "found": "found:f4", "chosen": "kept" },
                { "text": "The README's install section names an old flag" },
            ],
            "previous": { "version": 1, "built": ["`mindin lint` reads the index"], "decisions": [], "noticed": [] },
        })
    }

    #[test]
    fn a_card_holds_what_was_built_who_decided_what_and_what_was_noticed() {
        let read = Card::read(card().to_string().as_bytes()).unwrap();
        assert_eq!(read.decisions[1].replaced_by, Some(2));
        assert_eq!(read.decisions[3].by, DecidedBy::Task);
        assert_eq!(read.noticed[1].chosen, Some(NoticedChoice::Kept));
        assert_eq!(read.previous.as_ref().unwrap().version, 1);
        assert_eq!(serde_json::to_value(&read).unwrap(), card());
    }

    #[test]
    fn a_card_refuses_what_it_cannot_show() {
        let change = |pointer: &str, value: serde_json::Value| {
            let mut changed = card();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            match changed.pointer_mut(parent).unwrap() {
                serde_json::Value::Array(list) => list[key.parse::<usize>().unwrap()] = value,
                parent => parent[key] = value,
            }
            changed
        };
        for (name, wrong) in [
            ("version zero", change("/version", serde_json::json!(0))),
            ("seven Built lines", change("/built", serde_json::json!(["a", "b", "c", "d", "e", "f", "g"]))),
            ("a Built line over two lines", change("/built/0", serde_json::json!("one\ntwo"))),
            ("an empty decision", change("/decisions/0/text", serde_json::json!(" "))),
            ("a decider kebbie does not know", change("/decisions/0/by", serde_json::json!("reviewer"))),
            ("a source kebbie does not know", change("/decisions/0/source", serde_json::json!("hunch"))),
            ("a decision replaced by itself", change("/decisions/1/replaced_by", serde_json::json!(1))),
            ("a decision replaced by an earlier one", change("/decisions/1/replaced_by", serde_json::json!(0))),
            ("a decision replaced by one not on the card", change("/decisions/1/replaced_by", serde_json::json!(4))),
            ("a decision's subtask named by a path", change("/decisions/0/subtask", serde_json::json!("../index"))),
            ("a noticed line naming a kept item", change("/noticed/0/found", serde_json::json!("kept:idea-1"))),
            ("a choice on a line not filed", change("/noticed/2/chosen", serde_json::json!("kept"))),
            ("a found item listed twice", change("/noticed/1/found", serde_json::json!("found:f3"))),
            ("a choice kebbie does not know", change("/noticed/0/chosen", serde_json::json!("dismissed"))),
            ("a previous version that is not earlier", change("/previous/version", serde_json::json!(2))),
            ("a change without when", { let mut changed = card(); changed.as_object_mut().unwrap().remove("changed_at"); changed }),
            ("a previous version with its own previous", change("/previous/previous", card()["previous"].clone())),
            ("a field the card does not have", change("/confirm", serde_json::json!(["check it"]))),
        ] {
            assert!(Card::read(wrong.to_string().as_bytes()).is_err(), "{name}");
        }
    }
}
