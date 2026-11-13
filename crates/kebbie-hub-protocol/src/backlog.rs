use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::board::BlobRef;
use crate::message::{check, is_address, is_line};

pub const BACKLOG_BODY_MEDIA_TYPE: &str = "application/vnd.kebbie.backlog-item+json";
pub const MAX_LABEL_BYTES: usize = 256;
const MAX_LOCAL_ID_CHARS: usize = 64;
pub const MAX_SECTION_KEY_CHARS: usize = 64;
pub const MAX_HEADING_CHARS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ItemKey(String);

impl ItemKey {
    pub fn parse(text: &str) -> Result<ItemKey, &'static str> {
        let (home, rest) = text.split_once(':').ok_or("an item key is home:id")?;
        let fits = match home {
            "vault" => is_relative_label(rest),
            "local" | "kept" => is_local_id(rest),
            "found" => is_found_id(rest),
            _ => return Err("an item key's home is vault, local, kept or found"),
        };
        if fits { Ok(ItemKey(text.to_string())) } else { Err("an item key's id does not fit its home") }
    }

    pub fn vault(path: &str) -> Result<ItemKey, &'static str> {
        ItemKey::parse(&format!("vault:{path}"))
    }

    pub fn home(&self) -> HomeKind {
        match self.0.split_once(':').map(|(home, _)| home) {
            Some("vault") => HomeKind::Vault,
            Some("local") => HomeKind::Local,
            Some("found") => HomeKind::Found,
            _ => HomeKind::Kept,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ItemKey {
    type Error = &'static str;

    fn try_from(text: String) -> Result<ItemKey, &'static str> {
        ItemKey::parse(&text)
    }
}

impl From<ItemKey> for String {
    fn from(key: ItemKey) -> String {
        key.0
    }
}

impl fmt::Display for ItemKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub fn is_relative_label(text: &str) -> bool {
    (1..=MAX_LABEL_BYTES).contains(&text.len())
        && !text.starts_with(['/', '~'])
        && !text.chars().any(|char| char.is_control() || matches!(char, '\\' | ':'))
        && text.split('/').all(|segment| !matches!(segment, "" | "." | ".."))
}

fn is_local_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=MAX_LOCAL_ID_CHARS).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_found_id(text: &str) -> bool {
    text.strip_prefix('f').is_some_and(|number| (1..=MAX_FOUND_NUMBER_DIGITS).contains(&number.len()) && number.bytes().all(|byte| byte.is_ascii_digit()) && !number.starts_with('0'))
}

pub const MAX_FOUND_NUMBER_DIGITS: usize = 9;
pub const MAX_FOUND_SOURCES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogRow {
    pub item: ItemKey,
    pub title: String,
    pub priority: Priority,
    pub repo: Option<String>,
    pub state: BacklogState,
    pub filed: u64,
    pub home: Home,
    pub questions: u32,
    /// Blocking questions still waiting on the user; one with a pending_answer does not count.
    pub blocking: u32,
    pub goal: Option<String>,
    pub body: Option<BlobRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<Found>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Found {
    pub sources: Vec<String>,
    pub count: u32,
}

impl Found {
    fn check(&self) -> Result<(), &'static str> {
        check((1..=MAX_FOUND_SOURCES).contains(&self.sources.len()), "a found item names one to sixteen tasks that found it")?;
        check(self.sources.iter().all(|source| is_address(source)), "a found item's source is not task@repo")?;
        check(self.sources.iter().collect::<BTreeSet<_>>().len() == self.sources.len(), "a found item names each task once")?;
        check(self.count as usize >= self.sources.len(), "a found item was found at least once by each task it names")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Priority {
    High,
    Medium,
    Low,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BacklogState {
    ToWrite,
    Drafting,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Home {
    pub kind: HomeKind,
    #[serde(deserialize_with = "relative_label")]
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HomeKind {
    Vault,
    Local,
    Kept,
    Found,
}

fn relative_label<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let label = String::deserialize(deserializer)?;
    if !is_relative_label(&label) {
        return Err(serde::de::Error::custom("a home label is a short relative path"));
    }
    Ok(label)
}

pub(crate) fn backlog_rows<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<BacklogRow>>, D::Error> {
    let rows = Option::<Vec<BacklogRow>>::deserialize(deserializer)?;
    let mut seen = BTreeSet::new();
    for row in rows.iter().flatten() {
        if row.item.home() != row.home.kind {
            return Err(serde::de::Error::custom("an item key names its own home"));
        }
        if row.blocking > row.questions {
            return Err(serde::de::Error::custom("more questions block than are open"));
        }
        if (row.home.kind == HomeKind::Found) != row.found.is_some() {
            return Err(serde::de::Error::custom("a found item, and only a found item, says who found it"));
        }
        if let Some(problem) = row.found.as_ref().and_then(|found| found.check().err()) {
            return Err(serde::de::Error::custom(problem));
        }
        if !seen.insert(&row.item) {
            return Err(serde::de::Error::custom("an item is listed once"));
        }
    }
    Ok(rows)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BodyFields", into = "BodyFields")]
pub struct BacklogBody {
    pub sections: Vec<BodySection>,
    pub questions: Vec<OpenQuestion>,
    /// The section the open questions are shown under; none puts them after the last section.
    pub questions_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodySection {
    pub key: String,
    pub heading: String,
    pub text: String,
    pub required: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyFields {
    sections: Vec<BodySection>,
    questions: Vec<OpenQuestion>,
    questions_key: Option<String>,
}

impl TryFrom<BodyFields> for BacklogBody {
    type Error = &'static str;

    fn try_from(fields: BodyFields) -> Result<BacklogBody, &'static str> {
        let body = BacklogBody { sections: fields.sections, questions: fields.questions, questions_key: fields.questions_key };
        body.check()?;
        Ok(body)
    }
}

impl From<BacklogBody> for BodyFields {
    fn from(body: BacklogBody) -> BodyFields {
        BodyFields { sections: body.sections, questions: body.questions, questions_key: body.questions_key }
    }
}

impl BacklogBody {
    pub fn check(&self) -> Result<(), &'static str> {
        let mut keys = BTreeSet::new();
        for section in &self.sections {
            check(is_section_key(&section.key), "a section's key is not a short lowercase word")?;
            check(keys.insert(section.key.as_str()), "a section is listed once")?;
            check(is_line(&section.heading, MAX_HEADING_CHARS), "a section's heading is empty, too long or not one line")?;
        }
        check(self.questions_key.as_deref().is_none_or(|key| keys.contains(key)), "the questions sit under a section the body does not have")
    }

    pub fn section(&self, key: &str) -> Option<&BodySection> {
        self.sections.iter().find(|section| section.key == key)
    }
}

pub fn is_section_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=MAX_SECTION_KEY_CHARS).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_'))
}

/// What an item edit names as its base: the lowercase hex SHA-256 of the text's UTF-8 bytes.
pub fn text_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn is_text_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenQuestion {
    pub text: String,
    pub blocks: bool,
    pub pending_answer: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_key_is_a_vault_path_or_a_local_id_never_a_path_on_the_machine() {
        for good in ["vault:Projects/refund-rounding.md", "vault:Projects/Två ord.md", "local:01JB7Q2M3N", "kept:idea-4411", "found:f1", "found:f204"] {
            assert!(ItemKey::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "Projects/x.md",
            "vault:/Users/someone/vault/Projects/x.md",
            "vault:~/vault/x.md",
            "vault:Projects/../../etc/passwd",
            "vault:Projects//x.md",
            "vault:C:\\vault\\x.md",
            "vault:host:/x.md",
            "vault:Projects/x\n.md",
            "local:../x",
            "local:",
            "kept:a b",
            "task:refunds",
            "found:f",
            "found:f0",
            "found:f01",
            "found:7",
            "found:fx",
            "found:f1234567890",
        ] {
            assert!(ItemKey::parse(bad).is_err(), "{bad}");
        }
        assert!(ItemKey::parse(&format!("vault:{}", "a".repeat(MAX_LABEL_BYTES))).is_ok());
        assert!(ItemKey::parse(&format!("vault:{}", "a".repeat(MAX_LABEL_BYTES + 1))).is_err());
        assert_eq!(ItemKey::vault("Projects/x.md").unwrap().as_str(), "vault:Projects/x.md");
        assert_eq!(ItemKey::parse("kept:a").unwrap().home(), HomeKind::Kept);
        assert_eq!(ItemKey::parse("found:f3").unwrap().home(), HomeKind::Found);
    }

    #[test]
    fn a_row_names_its_home_by_its_key_and_never_more_blocking_questions_than_open_ones() {
        let row = serde_json::json!({
            "item": "vault:Projects/x.md", "title": "Round refunds", "priority": "high", "repo": "shop", "state": "ready",
            "filed": 1_761_000_000u64, "home": { "kind": "vault", "label": "Projects/x.md" }, "questions": 2, "blocking": 1,
            "goal": "Refunds round like invoices", "body": null,
        });
        let rows = |rows: serde_json::Value| backlog_rows(rows);
        assert!(rows(serde_json::json!([row.clone()])).is_ok());
        let mut local = row.clone();
        local["home"]["kind"] = serde_json::json!("local");
        let mut blocking = row.clone();
        blocking["blocking"] = serde_json::json!(3);
        let mut absolute = row.clone();
        absolute["home"]["label"] = serde_json::json!("/Users/someone/vault/Projects/x.md");
        for wrong in [serde_json::json!([local]), serde_json::json!([blocking]), serde_json::json!([absolute]), serde_json::json!([row.clone(), row])] {
            assert!(rows(wrong.clone()).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_found_row_says_which_tasks_found_it_and_how_often_and_no_other_row_does() {
        let row = serde_json::json!({
            "item": "found:f3", "title": "The remote client can't tell a missing log from an outage", "priority": "none", "repo": "mindin", "state": "ready",
            "filed": 1_761_000_000u64, "home": { "kind": "found", "label": "found by tasks" }, "questions": 0, "blocking": 0, "goal": null, "body": null,
            "found": { "sources": ["lint-index@mindin", "split-notes@mindin"], "count": 3 },
        });
        let rows = |rows: serde_json::Value| backlog_rows(rows);
        let read = rows(serde_json::json!([row.clone()])).unwrap().unwrap();
        assert_eq!(read[0].found, Some(Found { sources: vec!["lint-index@mindin".into(), "split-notes@mindin".into()], count: 3 }));
        let change = |pointer: &str, value: serde_json::Value| {
            let mut changed = row.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            serde_json::json!([changed])
        };
        for wrong in [
            change("/found", serde_json::Value::Null),
            change("/found/sources", serde_json::json!([])),
            change("/found/sources", serde_json::json!(["lint-index"])),
            change("/found/sources", serde_json::json!(["lint-index@mindin", "lint-index@mindin"])),
            change("/found/count", serde_json::json!(1)),
            change("/found/sources", serde_json::json!((0..=MAX_FOUND_SOURCES).map(|n| format!("t{n}@mindin")).collect::<Vec<_>>())),
            change("/home/kind", serde_json::json!("kept")),
        ] {
            assert!(rows(wrong.clone()).is_err(), "{wrong}");
        }
        let mut kept = row;
        kept["item"] = serde_json::json!("kept:idea-1");
        kept["home"]["kind"] = serde_json::json!("kept");
        assert!(rows(serde_json::json!([kept.clone()])).is_err(), "only a found item says who found it");
        kept.as_object_mut().unwrap().remove("found");
        assert!(rows(serde_json::json!([kept])).is_ok());
    }

    #[test]
    fn a_body_is_its_templates_sections_in_order_with_the_questions_under_one_of_them() {
        let section = |key: &str| serde_json::json!({ "key": key, "heading": "Goal", "text": "", "required": true });
        let body = |sections: Vec<serde_json::Value>, under: Option<&str>| serde_json::from_value::<BacklogBody>(serde_json::json!({ "sections": sections, "questions": [], "questions_key": under }));
        assert_eq!(body(vec![section("goal"), section("risks")], Some("risks")).unwrap().section("goal").map(|one| one.required), Some(true));
        assert!(body(vec![section("goal")], None).is_ok());
        assert!(body(vec![section("goal")], Some("risks")).is_err());
        assert!(body(vec![section("goal"), section("goal")], None).is_err());
        assert!(body(vec![section("Goal")], None).is_err());
        assert!(body(vec![section(&"a".repeat(MAX_SECTION_KEY_CHARS + 1))], None).is_err());
    }

    #[test]
    fn an_edit_names_its_base_as_the_sha256_of_the_text_it_edited() {
        assert_eq!(text_hash(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(is_text_hash(&text_hash("Goal")));
        assert!(!is_text_hash(&text_hash("Goal").to_uppercase()));
    }
}
