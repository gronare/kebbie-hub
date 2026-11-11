mod account;
mod alert;
mod authorize;
mod blob;
mod board;
mod connect;
mod crypto;
mod envelope;
mod ids;
mod message;
mod pairing;
#[cfg(any(test, feature = "test-vectors"))]
pub mod vectors;

pub use account::{
    ACCOUNT_COMMIT_DOMAIN, AccountFingerprints, AccountHandoff, AccountKey, DEVICE_LIST_DOMAIN,
    DeviceList, ListedDevice,
    ENCRYPTION_FINGERPRINT_DOMAIN, FieldTooLong, HUB_PRF_SALT_DOMAIN, ListedMachine, MACHINE_LIST_DOMAIN, MachineList,
    SIGNING_FINGERPRINT_DOMAIN, Unlock,
    WRAP_AD_DOMAIN, WRAP_KEY_INFO, WRAP_NONCE_LEN, WRAPPED_LEN, WRAPPER_SIGN_DOMAIN, WrongAccountKey,
    account_commitment, hub_prf_salt, pin_account, sign_wrapper,
    wrap_associated_data, wrap_key, wrapper_statement,
};
pub use alert::{Alert, AlertKind, AlertState};
pub use authorize::{
    Accepted, Applied, AppliedCommands, Dropped, Machine, Refusal, Role, TrustedDevice, authorize,
};
pub use blob::{BLOB_DOMAIN, BLOB_OVERHEAD, BlobError, MAX_BLOB_BYTES, open_blob, seal_blob, seal_blob_with};
pub use board::{
    Agent, AgentStatus, BlobRef, Board, CardRef, Checkout, Ci, CiState, Counts, Git, Group, Item,
    Later, LaterState, NextPart, NextRow, Prompt, PromptOption, PushPermission, Question,
    QuestionOption, Recover, Report, RoutineSummary, SecondOpinion, Step, StepState, Steps,
    Verdict, VerdictOutcome,
};
pub use connect::{CONNECT_FINGERPRINT_DOMAIN, ConnectSecret, machine_fingerprint};
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
