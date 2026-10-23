mod alert;
mod authorize;
mod board;
mod bug_report;
mod crypto;
mod envelope;
mod ids;
mod message;
mod pairing;
#[cfg(any(test, feature = "test-vectors"))]
pub mod vectors;

pub use alert::{Alert, AlertKind, AlertState};
pub use authorize::{
    Accepted, Applied, AppliedCommands, Dropped, Machine, Refusal, Role, TrustedDevice, authorize,
};
pub use board::{
    Agent, AgentStatus, BlobRef, Board, CardRef, Checkout, Ci, CiState, Git, Group, Item, Prompt,
    PromptOption, PushPermission, Question, QuestionOption, Recover, Report, RoutineSummary,
    Verdict, VerdictOutcome,
};
pub use bug_report::{
    BugReport, BugReportError, InstallKey, MAX_DIFFICULTY, MAX_FIELD_CHARS, MAX_TEXT_BYTES,
    MAX_TITLE_CHARS, REPORT_DOMAIN, REPORT_POW_DOMAIN, REPORT_SIGN_DOMAIN, leading_zero_bits,
    meets, pow_hash, solve, verify_report,
};
pub use crypto::{
    CLOCK_SKEW, HPKE_INFO, OpenError, PublicKeys, Randomness, SealError, SecretKeys,
    key_commitment, open, seal, verify,
};
pub use envelope::{Envelope, Header, Kind, ShapeError, Urgency};
pub use ids::{Id, IdError, Prefix};
pub use message::{
    Answer, CommandResult, Event, MAX_OPTIONS, MAX_PROMPT_CHARS, Notice, Outcome, Payload,
    PushApproval, Reason, RunRoutine, StartTask,
};
pub use pairing::{CommitmentMismatch, DevicePairing, MachinePairing, commitment, comparison_code};

pub const PROTOCOL_VERSION: u16 = 1;
pub const SCHEMA: u16 = 1;
