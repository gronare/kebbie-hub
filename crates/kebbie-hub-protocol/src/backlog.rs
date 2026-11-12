use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::board::BlobRef;

pub const BACKLOG_BODY_MEDIA_TYPE: &str = "application/vnd.kebbie.backlog-item+json";
pub const MAX_LABEL_BYTES: usize = 256;
const MAX_LOCAL_ID_CHARS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ItemKey(String);

impl ItemKey {
    pub fn parse(text: &str) -> Result<ItemKey, &'static str> {
        let (home, rest) = text.split_once(':').ok_or("an item key is home:id")?;
        let fits = match home {
            "vault" => is_relative_label(rest),
            "local" | "kept" => is_local_id(rest),
            _ => return Err("an item key's home is vault, local or kept"),
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
        if !seen.insert(&row.item) {
            return Err(serde::de::Error::custom("an item is listed once"));
        }
    }
    Ok(rows)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogBody {
    pub background: String,
    pub goal: String,
    pub what_we_know: String,
    pub questions: Vec<OpenQuestion>,
    pub out_of_scope: String,
    pub source: String,
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
        for good in ["vault:Projects/refund-rounding.md", "vault:Projects/Två ord.md", "local:01JB7Q2M3N", "kept:idea-4411"] {
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
        ] {
            assert!(ItemKey::parse(bad).is_err(), "{bad}");
        }
        assert!(ItemKey::parse(&format!("vault:{}", "a".repeat(MAX_LABEL_BYTES))).is_ok());
        assert!(ItemKey::parse(&format!("vault:{}", "a".repeat(MAX_LABEL_BYTES + 1))).is_err());
        assert_eq!(ItemKey::vault("Projects/x.md").unwrap().as_str(), "vault:Projects/x.md");
        assert_eq!(ItemKey::parse("kept:a").unwrap().home(), HomeKind::Kept);
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
}
