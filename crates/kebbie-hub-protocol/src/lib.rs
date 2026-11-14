mod account;
mod alert;
mod authorize;
mod backlog;
mod blob;
mod board;
mod card;
mod connect;
mod crypto;
mod device;
mod envelope;
mod ids;
mod lead;
mod message;
mod review;
#[cfg(any(test, feature = "test-vectors"))]
pub mod vectors;

pub use account::{
    ACCOUNT_COMMIT_DOMAIN, AccountFingerprints, AccountHandoff, AccountKey, CommitmentMismatch,
    ENCRYPTION_FINGERPRINT_DOMAIN, FieldTooLong, HUB_PRF_SALT_DOMAIN, ListedMachine, MACHINE_LIST_DOMAIN, MachineList,
    SIGNING_FINGERPRINT_DOMAIN, Unlock,
    WRAP_AD_DOMAIN, WRAP_KEY_INFO, WRAP_NONCE_LEN, WRAPPED_LEN, WRAPPER_SIGN_DOMAIN, WrongAccountKey,
    account_commitment, hub_prf_salt, pin_account, sign_wrapper,
    wrap_associated_data, wrap_key, wrapper_statement,
};
pub use alert::{Alert, AlertKind, AlertState};
pub use authorize::{Applied, AppliedCommands, Verified, Dropped, Machine, Refusal, authorize};
pub use backlog::{
    BACKLOG_BODY_MEDIA_TYPE, BacklogBody, BacklogRow, BacklogState, BodySection, Found, Home, HomeKind, ItemKey, MAX_FOUND_SOURCES, MAX_HEADING_CHARS,
    MAX_LABEL_BYTES, MAX_SECTION_KEY_CHARS, OpenQuestion, Priority, is_relative_label, is_section_key, is_text_hash, text_hash,
};
pub use blob::{BLOB_DOMAIN, BLOB_OVERHEAD, BlobError, MAX_BLOB_BYTES, open_blob, seal_blob, seal_blob_with};
pub use board::{
    Added, Agent, Assessment, AssessmentWord, Judgement, MAX_ASSESSMENT_REASON_CHARS, CommitTests, DecidedBy, DecisionKind, AgentStatus, BlobRef, Board, CardRef, Checkout, Ci, CiState, Counts, DeviceStanding, Git, Group, Item,
    KebbieQuestion, KebbieQuestionKind, Later, LaterState, CLOSE_OPTIONS, MAX_KEBBIE_QUESTION_CHARS, MAX_KEBBIE_QUESTION_ID_CHARS, MAX_KEBBIE_QUESTIONS,
    MAX_MEANING_CHARS, MAX_STAGE_DETAIL_CHARS, MAX_STAGE_NOTE_CHARS, MAX_STAGE_TITLE_CHARS, MAX_STAGES, Requirement, RequirementKind, Stage, StageState,
    VERIFY_OPTIONS, MAX_ADDED_REASON_CHARS, MAX_MOVED_REASON_CHARS, MAX_OBSTACLE_CHARS, MAX_RULE_NAME_CHARS, MAX_RULE_NAMES, MAX_SUBTASK_ID_CHARS, NextPart, NextRow,
    Moved, Obstacle, ObstacleDecision, ObstacleState, OrderRow, OverrideBy, Prompt, PromptOption, PushPermission, Pushable,
    Question, QuestionOption, Recover, Report, RoutineSummary, Rules, RulesOverride, RulesState, SecondOpinion, Subtask, SubtaskState,
    TestsState, Verdict, VerdictOutcome, is_subtask_id, pushable_of, unlock_stages,
};
pub use card::{CARD_MEDIA_TYPE, Card, CardDecision, DecisionSource, MAX_BUILT_LINES, MAX_CARD_LINE_CHARS, MAX_DECISION_CHARS, MAX_DECISIONS, MAX_NOTICED, Noticed, NoticedChoice};
pub use connect::{CONNECT_FINGERPRINT_DOMAIN, ConnectSecret, machine_fingerprint};
pub use crypto::{
    CLOCK_SKEW, HPKE_INFO, OpenError, PublicKeys, Randomness, SealError, SecretKeys,
    key_commitment, open, open_command, seal, seal_command, verify, verify_command,
};
pub use device::{
    BadCert, DEVICE_CERT_DOMAIN, DEVICE_FINGERPRINT_DOMAIN, DeviceCert, DeviceKey, DeviceKind,
    DeviceTrust, MAX_DEVICE_NAME_CHARS, Trust, TrustAccount, VerifiedDevice, device_fingerprint,
};
pub use envelope::{Envelope, Header, KEPT_ANSWER_TTL, Kind, ShapeError, Urgency};
pub use ids::{Id, IdError, Prefix};
pub use lead::{Away, Delivery, LEAD_THREAD_MEDIA_TYPE, LeadMessage, LeadRef, LeadThread, MAX_LEAD_MESSAGE_CHARS, MAX_LEAD_PAGE_TURNS, MAX_LEAD_TURN_CHARS, Turn};
pub use message::{
    Answer, CommandResult, DeviceDecision, FoundAdd, FoundDismiss, FoundKeep, FoundStart, Idea, ItemAnswer, ItemEdit, MAX_IDEA_CHARS, MAX_ITEM_ANSWER_CHARS,
    MAX_ITEM_EDIT_BYTES, MAX_ITEM_TITLE_CHARS, MAX_SECTION_CHARS,
    MAX_NOTICE_BODY_CHARS, MAX_NOTICE_BYTES, MAX_NOTICE_TITLE_CHARS, MAX_OPTIONS, MAX_PROMPT_CHARS,
    MAX_RETRACTS, Notice, NoticeCategory, Outcome, Payload, PromptAnswer, PushApproval, Reason, RunRoutine, StartItem,
    StartTask, Target,
};
pub use review::{
    AcceptStep, AcceptSubtask, DiffLine, FileStatus, GuideNote, Hunk, LineKind, MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS,
    MAX_REVIEW_COMMENTS_BYTES, REVIEW_MEDIA_TYPE, Review, ReviewComment, ReviewComments, ReviewFile, ReviewOffer,
    ReviewCommit, ReviewOpen, ReviewRead, ReviewSend, ReviewSubtask, Side, is_content_hash,
};

pub const PROTOCOL_VERSION: u16 = 1;
pub const SCHEMA: u16 = 1;
