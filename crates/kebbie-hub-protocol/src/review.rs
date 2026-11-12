use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::backlog::is_relative_label;
use crate::message::{check, is_address, is_full_sha, is_text};

pub const REVIEW_MEDIA_TYPE: &str = "application/vnd.kebbie.review+json";
pub const MAX_REVIEW_COMMENTS: usize = 50;
pub const MAX_REVIEW_COMMENT_CHARS: usize = 2_000;
pub const MAX_REVIEW_COMMENTS_BYTES: usize = 24 * 1024;
const MAX_SINCE_CHARS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewOffer {
    pub step: Option<u32>,
    pub files: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewOpen {
    pub address: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewComments {
    pub address: String,
    #[serde(default)]
    pub step: Option<u32>,
    pub comments: Vec<ReviewComment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewComment {
    pub path: String,
    pub line: u32,
    pub side: Side,
    #[serde(default)]
    pub sha: Option<String>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Old,
    New,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRead {
    pub address: String,
    pub path: String,
    pub read: bool,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptStep {
    pub address: String,
    pub step: u32,
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub address: String,
    pub since: String,
    pub head: String,
    pub step: Option<ReviewStep>,
    pub guide: Vec<GuideNote>,
    pub left_out: Option<String>,
    pub files: Vec<ReviewFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewStep {
    pub number: u32,
    pub title: String,
    pub sha: String,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideNote {
    pub path: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub add: u32,
    pub del: u32,
    pub content: String,
    pub read: bool,
    pub omitted: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hunk {
    pub head: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffLine {
    pub kind: LineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineKind {
    Context,
    Add,
    Del,
}

pub fn is_content_hash(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

impl ReviewOpen {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")
    }
}

impl ReviewComments {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")?;
        check((1..=MAX_REVIEW_COMMENTS).contains(&self.comments.len()), "a review sends one to fifty comments")?;
        check(self.step.is_none_or(|step| step >= 1), "steps count from one")?;
        for comment in &self.comments {
            check(is_relative_label(&comment.path), "a comment's path is not a relative path")?;
            check(comment.line >= 1, "lines count from one")?;
            check(comment.sha.as_deref().is_none_or(is_full_sha), "a comment's sha is not a full sha")?;
            check(is_text(&comment.text, MAX_REVIEW_COMMENT_CHARS), "a comment is empty or too long")?;
            check(!comment.text.chars().any(|char| char.is_control() && !matches!(char, '\n' | '\t')), "a comment holds control characters")?;
        }
        check(serde_json::to_vec(self).expect("comments always serialize").len() <= MAX_REVIEW_COMMENTS_BYTES, "the comments are too large to send together")
    }
}

impl ReviewRead {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")?;
        check(is_relative_label(&self.path), "path is not a relative path")?;
        check(is_content_hash(&self.content), "content is not a git object id")
    }
}

impl AcceptStep {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")?;
        check(self.step >= 1, "steps count from one")?;
        check(is_full_sha(&self.sha), "sha is not a full sha")
    }
}

impl Review {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")?;
        check(is_text(&self.since, MAX_SINCE_CHARS) && !self.since.chars().any(char::is_control), "since is not a short ref name")?;
        check(is_full_sha(&self.head), "head is not a full sha")?;
        if let Some(step) = &self.step {
            check(step.number >= 1, "steps count from one")?;
            check(is_full_sha(&step.sha), "a step's sha is not a full sha")?;
        }
        check(self.guide.iter().all(|note| is_relative_label(&note.path)), "a guide note's path is not a relative path")?;
        let mut paths = BTreeSet::new();
        for file in &self.files {
            check(is_relative_label(&file.path), "a file's path is not a relative path")?;
            check(paths.insert(file.path.as_str()), "a file is listed once")?;
            check(file.old_path.as_deref().is_none_or(is_relative_label), "a file's old path is not a relative path")?;
            check((file.status == FileStatus::Renamed) == file.old_path.is_some(), "only a renamed file names its old path")?;
            check(is_content_hash(&file.content), "a file's content is not a git object id")?;
            file.check_lines()?;
        }
        Ok(())
    }
}

impl ReviewFile {
    fn check_lines(&self) -> Result<(), &'static str> {
        if self.omitted {
            return check(self.hunks.is_empty(), "an omitted file carries no hunks");
        }
        let (mut add, mut del) = (0u32, 0u32);
        for line in self.hunks.iter().flat_map(|hunk| &hunk.lines) {
            let numbered = match line.kind {
                LineKind::Context => line.old.is_some() && line.new.is_some(),
                LineKind::Add => line.old.is_none() && line.new.is_some(),
                LineKind::Del => line.old.is_some() && line.new.is_none(),
            };
            check(numbered, "a line's numbers do not fit its kind")?;
            add += u32::from(line.kind == LineKind::Add);
            del += u32::from(line.kind == LineKind::Del);
        }
        check((add, del) == (self.add, self.del), "a file's counts differ from its lines")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "3f9a2d1c8e7b6a5f4d3c2b1a0f9e8d7c6b5a4f3e";

    fn file() -> ReviewFile {
        ReviewFile {
            path: "src/ci.rs".into(),
            old_path: None,
            status: FileStatus::Modified,
            add: 1,
            del: 1,
            content: "a".repeat(40),
            read: false,
            omitted: false,
            hunks: vec![Hunk {
                head: "@@ -1,2 +1,2 @@".into(),
                lines: vec![
                    DiffLine { kind: LineKind::Context, old: Some(1), new: Some(1), text: "fn main() {".into() },
                    DiffLine { kind: LineKind::Del, old: Some(2), new: None, text: "    old();".into() },
                    DiffLine { kind: LineKind::Add, old: None, new: Some(2), text: "    new();".into() },
                ],
            }],
        }
    }

    fn review() -> Review {
        Review {
            address: "retry-push@kebbie".into(),
            since: "main".into(),
            head: SHA.into(),
            step: Some(ReviewStep { number: 1, title: "Retry".into(), sha: SHA.into(), subject: "Retry a push".into(), body: String::new() }),
            guide: vec![GuideNote { path: "src/ci.rs".into(), text: "`Run` reads `runner_lost`.".into() }],
            left_out: None,
            files: vec![file()],
        }
    }

    #[test]
    fn a_review_holds_relative_paths_full_shas_and_lines_numbered_by_their_kind() {
        assert_eq!(review().check(), Ok(()));
        let json = serde_json::to_value(review()).unwrap();
        assert_eq!(json["files"][0]["hunks"][0]["lines"][1], serde_json::json!({ "kind": "del", "old": 2, "text": "    old();" }));
        assert_eq!(serde_json::from_value::<Review>(json.clone()).unwrap(), review());
        let wrong: [fn(&mut Review); 9] = [
            |review| review.files[0].path = "/Users/someone/shop/src/ci.rs".into(),
            |review| review.files[0].path = "../ci.rs".into(),
            |review| review.files.push(review.files[0].clone()),
            |review| review.files[0].content = "abc".into(),
            |review| review.files[0].add = 2,
            |review| review.files[0].hunks[0].lines[2].old = Some(2),
            |review| review.files[0].old_path = Some("src/old.rs".into()),
            |review| review.step.as_mut().unwrap().sha = "3f9a2d1".into(),
            |review| review.head = SHA.to_uppercase(),
        ];
        for (index, change) in wrong.iter().enumerate() {
            let mut changed = review();
            change(&mut changed);
            assert!(changed.check().is_err(), "change {index}");
        }
        let mut omitted = review();
        omitted.files[0].omitted = true;
        assert!(omitted.check().is_err(), "an omitted file carries no hunks");
        omitted.files[0].hunks.clear();
        assert_eq!(omitted.check(), Ok(()));
        let mut leaked = json;
        leaked["files"][0]["absolute"] = serde_json::json!("/Users/someone/shop/src/ci.rs");
        assert!(serde_json::from_value::<Review>(leaked).is_err());
    }

    #[test]
    fn comments_go_together_within_their_limits() {
        let comment = |text: &str| ReviewComment { path: "src/ci.rs".into(), line: 112, side: Side::Old, sha: Some(SHA.into()), text: text.into() };
        let comments = |list: Vec<ReviewComment>| ReviewComments { address: "retry-push@kebbie".into(), step: Some(1), comments: list };
        assert_eq!(comments(vec![comment("Should each try get its own deadline?")]).check(), Ok(()));
        assert!(comments(vec![]).check().is_err());
        assert!(comments(vec![comment("x"); MAX_REVIEW_COMMENTS + 1]).check().is_err());
        assert!(comments(vec![comment(" ")]).check().is_err());
        assert!(comments(vec![comment(&"x".repeat(MAX_REVIEW_COMMENT_CHARS + 1))]).check().is_err());
        assert!(comments(vec![comment(&"€".repeat(MAX_REVIEW_COMMENT_CHARS)); 5]).check().is_err(), "too large to send together");
        assert!(comments(vec![ReviewComment { line: 0, ..comment("x") }]).check().is_err());
        assert!(comments(vec![ReviewComment { sha: Some("3f9a2d1".into()), ..comment("x") }]).check().is_err());
        assert!(comments(vec![ReviewComment { path: "/etc/passwd".into(), ..comment("x") }]).check().is_err());
        assert!(comments(vec![comment("Clear it\u{1b}[2J")]).check().is_err(), "a terminal escape never reaches Claude's input box");
        assert_eq!(comments(vec![comment("One line\n\tand an indented one")]).check(), Ok(()));
        let json = serde_json::json!({ "address": "retry-push@kebbie", "comments": [{ "path": "src/ci.rs", "line": 3, "side": "new", "text": "Keep it" }] });
        let bare: ReviewComments = serde_json::from_value(json).unwrap();
        assert_eq!((bare.step, bare.comments[0].sha.as_ref(), bare.comments[0].side), (None, None, Side::New));
    }

    #[test]
    fn a_read_mark_names_the_content_it_holds_for_and_an_accept_names_its_step_and_sha() {
        let read = |content: &str| ReviewRead { address: "retry-push@kebbie".into(), path: "src/ci.rs".into(), read: true, content: content.into() };
        assert_eq!(read(&"a".repeat(40)).check(), Ok(()));
        assert_eq!(read(&"a".repeat(64)).check(), Ok(()));
        assert!(read("abc1234").check().is_err());
        assert!(read(&"A".repeat(40)).check().is_err());
        let accept = |step, sha: &str| AcceptStep { address: "retry-push@kebbie".into(), step, sha: sha.into() };
        assert_eq!(accept(1, SHA).check(), Ok(()));
        assert!(accept(0, SHA).check().is_err());
        assert!(accept(1, "3f9a2d1").check().is_err());
        assert!(ReviewOpen { address: "retry-push".into() }.check().is_err());
    }
}
