use serde::{Deserialize, Serialize};

use crate::blob::{BLOB_OVERHEAD, MAX_BLOB_BYTES};
use crate::board::BlobRef;
use crate::ids::{Id, Prefix};
use crate::message::{check, is_text, is_token};

pub const JOB_BUNDLE_MEDIA_TYPE: &str = "application/vnd.kebbie.git-bundle";
pub const MAX_JOB_COMMAND_CHARS: usize = 8_000;
pub const MAX_JOB_BUNDLE_PARTS: usize = 128;
pub const MAX_JOB_LOG_BYTES: usize = 12 * 1024;
pub const MAX_JOB_ARTIFACTS: usize = 16;
pub const MAX_JOB_REASON_CHARS: usize = 500;
pub const MAX_MACHINE_MESSAGE_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub repo: String,
    pub name: String,
    pub command: String,
    pub task: String,
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundle: Vec<BlobRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobReport {
    pub job: Id,
    pub seq: u32,
    pub log: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<JobEnd>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JobEnd {
    Exited {
        code: i32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<BlobRef>,
    },
    NeedsFull {},
    Refused { reason: String },
    Failed { reason: String },
}

impl Job {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_token(&self.repo), "repo is not a plain name")?;
        check(is_token(&self.name), "name is not a plain name")?;
        check(is_token(&self.task), "task is not a plain name")?;
        check(is_text(&self.command, MAX_JOB_COMMAND_CHARS), "the command is empty or too long")?;
        check(!self.command.chars().any(|char| char.is_control() && !matches!(char, '\n' | '\t')), "the command holds control characters")?;
        check(is_commit(&self.commit), "commit is not a full lowercase sha")?;
        check(self.base.as_deref().is_none_or(is_commit), "base is not a full lowercase sha")?;
        check(self.bundle.len() <= MAX_JOB_BUNDLE_PARTS, "the bundle has too many parts")?;
        check(self.bundle.iter().all(is_blob), "a bundle part is not a blob")?;
        check(self.on.as_deref().is_none_or(is_token), "on is not a plain name")?;
        fits(self)
    }
}

impl JobReport {
    pub fn check(&self) -> Result<(), &'static str> {
        check(self.job.prefix() == Prefix::Command, "job is not a command id")?;
        check(self.log.len() <= MAX_JOB_LOG_BYTES, "the log is too long")?;
        match &self.end {
            None | Some(JobEnd::NeedsFull {}) => {}
            Some(JobEnd::Exited { artifacts, .. }) => {
                check(artifacts.len() <= MAX_JOB_ARTIFACTS, "too many artifacts")?;
                check(artifacts.iter().all(is_blob), "an artifact is not a blob")?;
            }
            Some(JobEnd::Refused { reason } | JobEnd::Failed { reason }) => {
                check(is_text(reason, MAX_JOB_REASON_CHARS), "the reason is empty or too long")?;
                check(!reason.chars().any(|char| char.is_control() && char != '\n'), "the reason holds control characters")?;
            }
        }
        fits(self)
    }
}

fn fits(payload: &impl Serialize) -> Result<(), &'static str> {
    check(serde_json::to_vec(payload).expect("a machine message always serializes").len() <= MAX_MACHINE_MESSAGE_BYTES, "the message is too large to send")
}

fn is_blob(blob: &BlobRef) -> bool {
    blob.id.prefix() == Prefix::Blob && blob.bytes <= (MAX_BLOB_BYTES - BLOB_OVERHEAD) as u64
}

pub fn is_commit(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{SecretKeys, key_commitment, open, seal};
    use crate::device::{DeviceCert, DeviceKind};
    use crate::envelope::{Header, Kind, ShapeError};
    use crate::message::Payload;

    const SHA: &str = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";

    fn blob(bytes: u64) -> BlobRef {
        BlobRef { id: Id::blob(b"part"), key: "k".repeat(43), media_type: JOB_BUNDLE_MEDIA_TYPE.into(), bytes }
    }

    fn job() -> Job {
        Job { repo: "kebbie".into(), name: "ios-tests".into(), command: "xcodebuild test -scheme Kebbie".into(), task: "email-column".into(), commit: SHA.into(), base: None, bundle: vec![blob(1024)], on: Some("macos".into()) }
    }

    fn report(end: Option<JobEnd>) -> JobReport {
        JobReport { job: Id::command(1, [1; 10]), seq: 0, log: String::new(), end }
    }

    fn machine(seed: u8) -> Id {
        Id::ulid(Prefix::Machine, 1, [seed; 10]).unwrap()
    }

    fn header(kind: Kind, from: Id, to: Id) -> Header {
        let mut header = Header::new(Id::new(Prefix::Command).unwrap(), kind, from, to, 1_000);
        header.kc = key_commitment(&[0; 32]);
        header
    }

    #[test]
    fn a_job_names_a_plain_repo_task_and_full_commit_and_holds_its_bounds() {
        assert_eq!(Payload::Job(job()).validate(), Ok(()));
        let with = |change: &dyn Fn(&mut Job)| {
            let mut job = job();
            change(&mut job);
            Payload::Job(job).validate()
        };
        assert_eq!(with(&|job| job.commit = "a".repeat(64)), Ok(()), "a SHA-256 repository names its commits in 64 characters");
        assert_eq!(with(&|job| job.base = Some("b".repeat(40))), Ok(()));
        assert_eq!(with(&|job| job.command = "x".repeat(MAX_JOB_COMMAND_CHARS)), Ok(()));
        assert_eq!(with(&|job| job.command = "make test\n\tmake lint".into()), Ok(()));
        assert_eq!(with(&|job| job.bundle = vec![blob(1); MAX_JOB_BUNDLE_PARTS]), Ok(()));
        assert_eq!(with(&|job| job.bundle = vec![blob((MAX_BLOB_BYTES - crate::BLOB_OVERHEAD) as u64)]), Ok(()));
        for (refused, change) in [
            ("short commit", &(|job: &mut Job| job.commit = "9f3c2a1".into()) as &dyn Fn(&mut Job)),
            ("uppercase commit", &|job| job.commit = SHA.to_uppercase()),
            ("short base", &|job| job.base = Some("abc".into())),
            ("repo as a path", &|job| job.repo = "../kebbie".into()),
            ("name with a space", &|job| job.name = "ios tests".into()),
            ("blank task", &|job| job.task = String::new()),
            ("blank command", &|job| job.command = " ".into()),
            ("long command", &|job| job.command = "x".repeat(MAX_JOB_COMMAND_CHARS + 1)),
            ("escape in the command", &|job| job.command = "ls\u{1b}[2J".into()),
            ("too many parts", &|job| job.bundle = vec![blob(1); MAX_JOB_BUNDLE_PARTS + 1]),
            ("a part past a blob", &|job| job.bundle = vec![blob((MAX_BLOB_BYTES - crate::BLOB_OVERHEAD) as u64 + 1)]),
            ("a part that is not a blob", &|job| job.bundle[0].id = Id::command(1, [1; 10])),
            ("on as a sentence", &|job| job.on = Some("my Mac".into())),
            ("too large to send", &|job| job.command = "€".repeat(MAX_JOB_COMMAND_CHARS) + &"€".repeat(MAX_JOB_COMMAND_CHARS)),
        ] {
            assert!(with(change).is_err(), "{refused}");
        }
        assert!(with(&|job| job.command = "€".repeat(MAX_JOB_COMMAND_CHARS)).is_ok(), "eight thousand characters fit even when each is three bytes");
        let json = serde_json::to_value(Payload::Job(Job { base: None, bundle: Vec::new(), on: None, ..job() })).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "job", "repo": "kebbie", "name": "ios-tests", "command": "xcodebuild test -scheme Kebbie", "task": "email-column", "commit": SHA }));
        let mut extra = json;
        extra["env"] = serde_json::json!({ "TOKEN": "x" });
        assert!(serde_json::from_value::<Payload>(extra).is_err());
    }

    #[test]
    fn a_report_streams_bounded_output_and_ends_once_with_an_outcome() {
        assert_eq!(Payload::JobReport(report(None)).validate(), Ok(()), "a report may carry no output yet");
        assert_eq!(Payload::JobReport(JobReport { log: "x".repeat(MAX_JOB_LOG_BYTES), ..report(None) }).validate(), Ok(()));
        assert!(Payload::JobReport(JobReport { log: "x".repeat(MAX_JOB_LOG_BYTES + 1), ..report(None) }).validate().is_err());
        assert!(Payload::JobReport(JobReport { log: "é".repeat(MAX_JOB_LOG_BYTES / 2 + 1), ..report(None) }).validate().is_err(), "the bound is in bytes");
        assert!(Payload::JobReport(JobReport { job: Id::new(Prefix::Notice).unwrap(), ..report(None) }).validate().is_err());
        let exited = |artifacts: usize| report(Some(JobEnd::Exited { code: -1, artifacts: vec![blob(10); artifacts] }));
        assert_eq!(Payload::JobReport(exited(MAX_JOB_ARTIFACTS)).validate(), Ok(()));
        assert!(Payload::JobReport(exited(MAX_JOB_ARTIFACTS + 1)).validate().is_err());
        let reason = |text: &str| Payload::JobReport(report(Some(JobEnd::Failed { reason: text.into() }))).validate();
        assert_eq!(reason("xcodebuild is not installed"), Ok(()));
        assert!(reason("").is_err());
        assert!(reason(&"x".repeat(MAX_JOB_REASON_CHARS + 1)).is_err());
        assert!(Payload::JobReport(report(Some(JobEnd::Refused { reason: "a\u{7}".into() }))).validate().is_err());
        for (end, json) in [
            (JobEnd::Exited { code: 65, artifacts: Vec::new() }, serde_json::json!({ "outcome": "exited", "code": 65 })),
            (JobEnd::NeedsFull {}, serde_json::json!({ "outcome": "needs-full" })),
            (JobEnd::Refused { reason: "not a worker for kebbie".into() }, serde_json::json!({ "outcome": "refused", "reason": "not a worker for kebbie" })),
        ] {
            let payload = Payload::JobReport(report(Some(end)));
            assert_eq!(serde_json::to_value(&payload).unwrap()["end"], json);
            assert_eq!(serde_json::from_value::<Payload>(serde_json::to_value(&payload).unwrap()).unwrap(), payload);
        }
        let stray = serde_json::json!({ "type": "job-report", "job": Id::command(1, [1; 10]), "seq": 0, "log": "", "end": { "outcome": "needs-full", "code": 0 } });
        assert!(serde_json::from_value::<Payload>(stray).is_err());
    }

    #[test]
    fn a_machine_message_goes_from_one_machine_to_another_without_a_certificate_and_lives_a_day_at_most() {
        assert_eq!(Kind::ALL.iter().filter(|kind| kind.is_machine_message()).copied().collect::<Vec<_>>(), [Kind::Job, Kind::JobReport, Kind::Addresses]);
        assert_eq!((Kind::Addresses.default_ttl(), Kind::Addresses.max_ttl()), (86_400, 86_400), "a machine announces its addresses for a day at a time");
        for kind in [Kind::Job, Kind::JobReport] {
            assert_eq!((kind.is_command(), kind.id_prefix(), kind.default_ttl(), kind.max_ttl()), (false, Prefix::Command, 3_600, 86_400), "{}", kind.name());
            assert_eq!(header(kind, machine(1), machine(2)).check_shape(), Ok(()));
            assert!(matches!(header(kind, machine(1), machine(1)).check_shape(), Err(ShapeError::Malformed(_))), "a machine sends no job to itself");
            assert!(matches!(header(kind, Id::account([3; 16]), machine(2)).check_shape(), Err(ShapeError::Malformed(_))));
            let mut long = header(kind, machine(1), machine(2));
            long.exp = long.at + 86_401;
            assert!(long.check_shape().is_err());
            let keys = SecretKeys::generate();
            let cert = DeviceCert::issue(&keys, &Id::account([3; 16]), Id::ulid(Prefix::Device, 1, [4; 10]).unwrap(), "iPhone", DeviceKind::Web, keys.public(), 1).unwrap();
            let mut certified = header(kind, cert.device.clone(), machine(2));
            certified.cert = Some(cert.clone());
            assert!(certified.check_shape().is_err(), "a device sends no job");
            let mut carried = header(kind, machine(1), machine(2));
            carried.cert = Some(cert);
            assert!(carried.check_shape().is_err(), "a job carries no certificate");
        }
        assert!(header(Kind::Board, machine(1), machine(2)).check_shape().is_err(), "a board still comes from its own machine");
        assert_eq!(header(Kind::Result, machine(1), machine(1)).check_shape(), Ok(()));
    }

    #[test]
    fn a_job_is_sealed_by_the_sending_machine_to_the_worker_itself_and_the_account() {
        let (master, worker, account) = (SecretKeys::generate(), SecretKeys::generate(), SecretKeys::generate());
        let (master_id, worker_id, account_id) = (machine(1), machine(2), Id::account([3; 16]));
        let recipients = [(worker_id.clone(), worker.public()), (master_id.clone(), master.public()), (account_id.clone(), account.public())];
        let payload = Payload::Job(job());
        let envelope = seal(Header::new(Id::new(Prefix::Command).unwrap(), Kind::Job, master_id.clone(), worker_id.clone(), 1_000), &payload, &master, &recipients).unwrap();
        assert_eq!(open(&envelope, &worker_id, &worker, &master_id, &master.public(), 1_000), Ok(payload.clone()));
        assert_eq!(open(&envelope, &account_id, &account, &master_id, &master.public(), 1_000), Ok(payload.clone()));
        assert_eq!(open(&envelope, &worker_id, &worker, &master_id, &worker.public(), 1_000), Err(crate::OpenError::BadSignature));
        let alone = envelope.for_recipient(&worker_id).unwrap();
        assert_eq!(open(&alone, &worker_id, &worker, &master_id, &master.public(), 1_000), Ok(payload));
    }
}
