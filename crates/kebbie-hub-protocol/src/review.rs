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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptStep {
    pub address: String,
    pub step: u32,
    pub head: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "ReviewWire")]
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
    pub commits: Vec<ReviewCommit>,
    pub files: Vec<ReviewFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCommit {
    pub sha: String,
    pub subject: String,
    pub body: String,
    pub files: Vec<ReviewFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewWire {
    address: String,
    since: String,
    head: String,
    step: Option<StepWire>,
    guide: Vec<GuideNote>,
    left_out: Option<String>,
    files: Vec<ReviewFile>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StepWire {
    Commits(ReviewStep),
    OneCommit(OneCommitStep),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OneCommitStep {
    number: u32,
    title: String,
    sha: String,
    subject: String,
    body: String,
}

impl From<ReviewWire> for Review {
    fn from(wire: ReviewWire) -> Self {
        let (step, files) = match wire.step {
            None => (None, wire.files),
            Some(StepWire::Commits(step)) => (Some(step), wire.files),
            Some(StepWire::OneCommit(one)) => {
                let commit = ReviewCommit { sha: one.sha, subject: one.subject, body: one.body, files: wire.files.clone() };
                (Some(ReviewStep { number: one.number, title: one.title, commits: vec![commit], files: wire.files }), Vec::new())
            }
        };
        Review { address: wire.address, since: wire.since, head: wire.head, step, guide: wire.guide, left_out: wire.left_out, files }
    }
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
        check(is_address(&self.address), "address is not task@repo")?;
        check(self.step.is_none_or(|step| step >= 1), "steps count from one")
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
        check(is_content_hash(&self.content), "content is not a git object id")?;
        check(self.sha.as_deref().is_none_or(is_full_sha), "a read mark's commit is not a full sha")
    }
}

impl AcceptStep {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")?;
        check(self.step >= 1, "steps count from one")?;
        check(is_full_sha(&self.head), "head is not a full sha")
    }
}

impl Review {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_address(&self.address), "address is not task@repo")?;
        check(is_text(&self.since, MAX_SINCE_CHARS) && !self.since.chars().any(char::is_control), "since is not a short ref name")?;
        check(is_full_sha(&self.head), "head is not a full sha")?;
        check(self.guide.iter().all(|note| is_relative_label(&note.path)), "a guide note's path is not a relative path")?;
        check_files(&self.files)?;
        let Some(step) = &self.step else {
            return Ok(());
        };
        check(self.files.is_empty(), "a step's files are the step's own, not the review's")?;
        check(step.number >= 1, "steps count from one")?;
        check(!step.commits.is_empty(), "a step holds at least one commit")?;
        check(step.commits.last().is_some_and(|last| last.sha == self.head), "a step's review ends at its last commit")?;
        let mut shas = BTreeSet::new();
        for commit in &step.commits {
            check(is_full_sha(&commit.sha), "a commit's sha is not a full sha")?;
            check(shas.insert(commit.sha.as_str()), "a commit is listed once")?;
            check_files(&commit.files)?;
        }
        check_files(&step.files)
    }
}

impl ReviewStep {
    pub fn head(&self) -> Option<&str> {
        self.commits.last().map(|commit| commit.sha.as_str())
    }
}

fn check_files(files: &[ReviewFile]) -> Result<(), &'static str> {
    let mut paths = BTreeSet::new();
    for file in files {
        check(is_relative_label(&file.path), "a file's path is not a relative path")?;
        check(paths.insert(file.path.as_str()), "a file is listed once")?;
        check(file.old_path.as_deref().is_none_or(is_relative_label), "a file's old path is not a relative path")?;
        check((file.status == FileStatus::Renamed) == file.old_path.is_some(), "only a renamed file names its old path")?;
        check(is_content_hash(&file.content), "a file's content is not a git object id")?;
        file.check_lines()?;
    }
    Ok(())
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
    const FIRST: &str = "a41c9e07d2b35f8e61a0c4d7b9e2f13a5c6d8e90";

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

    fn commit(sha: &str, content: char) -> ReviewCommit {
        ReviewCommit { sha: sha.into(), subject: "Retry a push".into(), body: String::new(), files: vec![ReviewFile { content: content.to_string().repeat(40), ..file() }] }
    }

    fn review() -> Review {
        Review {
            address: "retry-push@kebbie".into(),
            since: "main".into(),
            head: SHA.into(),
            step: Some(ReviewStep { number: 1, title: "Retry".into(), commits: vec![commit(FIRST, 'b'), commit(SHA, 'a')], files: vec![file()] }),
            guide: vec![GuideNote { path: "src/ci.rs".into(), text: "`Run` reads `runner_lost`.".into() }],
            left_out: None,
            files: Vec::new(),
        }
    }

    #[test]
    fn a_review_holds_relative_paths_full_shas_and_lines_numbered_by_their_kind() {
        assert_eq!(review().check(), Ok(()));
        let json = serde_json::to_value(review()).unwrap();
        assert_eq!(json["step"]["files"][0]["hunks"][0]["lines"][1], serde_json::json!({ "kind": "del", "old": 2, "text": "    old();" }));
        assert_eq!(serde_json::from_value::<Review>(json.clone()).unwrap(), review());
        let wrong: [fn(&mut Review); 14] = [
            |review| review.step.as_mut().unwrap().files[0].path = "/Users/someone/shop/src/ci.rs".into(),
            |review| review.step.as_mut().unwrap().commits[0].files[0].path = "../ci.rs".into(),
            |review| review.step.as_mut().unwrap().files.push(file()),
            |review| review.step.as_mut().unwrap().files[0].content = "abc".into(),
            |review| review.step.as_mut().unwrap().commits[1].files[0].add = 2,
            |review| review.step.as_mut().unwrap().files[0].hunks[0].lines[2].old = Some(2),
            |review| review.step.as_mut().unwrap().files[0].old_path = Some("src/old.rs".into()),
            |review| review.step.as_mut().unwrap().commits[0].sha = "3f9a2d1".into(),
            |review| review.step.as_mut().unwrap().commits[0].sha = SHA.into(),
            |review| review.step.as_mut().unwrap().commits.clear(),
            |review| review.step.as_mut().unwrap().commits.reverse(),
            |review| review.files.push(file()),
            |review| review.step.as_mut().unwrap().number = 0,
            |review| review.head = SHA.to_uppercase(),
        ];
        for (index, change) in wrong.iter().enumerate() {
            let mut changed = review();
            change(&mut changed);
            assert!(changed.check().is_err(), "change {index}");
        }
        let mut omitted = review();
        omitted.step.as_mut().unwrap().files[0].omitted = true;
        assert!(omitted.check().is_err(), "an omitted file carries no hunks");
        omitted.step.as_mut().unwrap().files[0].hunks.clear();
        assert_eq!(omitted.check(), Ok(()));
        let mut leaked = json;
        leaked["step"]["files"][0]["absolute"] = serde_json::json!("/Users/someone/shop/src/ci.rs");
        assert!(serde_json::from_value::<Review>(leaked).is_err());
    }

    #[test]
    fn a_review_of_the_whole_branch_holds_its_files_itself() {
        let branch = Review { step: None, files: vec![file()], ..review() };
        assert_eq!(branch.check(), Ok(()));
        let mut twice = branch;
        twice.files.push(file());
        assert!(twice.check().is_err(), "a file is listed once");
    }

    #[test]
    fn a_review_from_a_machine_not_yet_updated_opens_as_a_step_of_one_commit() {
        let old = serde_json::json!({
            "address": "retry-push@kebbie", "since": "main", "head": SHA, "guide": [], "left_out": null,
            "step": { "number": 1, "title": "Retry", "sha": SHA, "subject": "Retry a push", "body": "Why." },
            "files": [serde_json::to_value(file()).unwrap()],
        });
        let review: Review = serde_json::from_value(old).unwrap();
        assert_eq!(review.check(), Ok(()));
        let step = review.step.unwrap();
        assert_eq!((step.commits.len(), step.commits[0].sha.as_str(), step.commits[0].body.as_str(), step.head()), (1, SHA, "Why.", Some(SHA)));
        assert_eq!((step.commits[0].files.clone(), step.files, review.files), (vec![file()], vec![file()], Vec::new()));
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
    fn a_read_mark_names_the_content_and_commit_it_holds_for_and_an_accept_names_its_step_and_head() {
        let read = |content: &str, sha: Option<&str>| ReviewRead { address: "retry-push@kebbie".into(), path: "src/ci.rs".into(), read: true, content: content.into(), sha: sha.map(Into::into) };
        assert_eq!(read(&"a".repeat(40), None).check(), Ok(()));
        assert_eq!(read(&"a".repeat(64), Some(SHA)).check(), Ok(()));
        assert!(read("abc1234", None).check().is_err());
        assert!(read(&"A".repeat(40), None).check().is_err());
        assert!(read(&"a".repeat(40), Some("3f9a2d1")).check().is_err());
        assert_eq!(serde_json::to_value(read(&"a".repeat(40), None)).unwrap().get("sha"), None, "a mark on the whole step names no commit");
        let accept = |step, head: &str| AcceptStep { address: "retry-push@kebbie".into(), step, head: head.into() };
        assert_eq!(accept(1, SHA).check(), Ok(()));
        assert!(accept(0, SHA).check().is_err());
        assert!(accept(1, "3f9a2d1").check().is_err());
        let open = |step| ReviewOpen { address: "retry-push@kebbie".into(), step };
        assert_eq!((open(None).check(), open(Some(2)).check()), (Ok(()), Ok(())));
        assert!(open(Some(0)).check().is_err());
        assert_eq!(serde_json::to_value(open(None)).unwrap(), serde_json::json!({ "address": "retry-push@kebbie" }));
        assert!(ReviewOpen { address: "retry-push".into(), step: None }.check().is_err());
    }
}
