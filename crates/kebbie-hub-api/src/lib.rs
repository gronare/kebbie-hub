use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_protocol::{AccountKey, DeviceCert, DeviceKind, Envelope, Id, MachineList, NoticeCategory, PublicKeys};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
pub use webauthn_rs_proto::{
    CreationChallengeResponse, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};

pub const MAX_WAIT_SECONDS: u64 = 25;
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_CHARS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bytes32(pub [u8; 32]);

impl fmt::Debug for Bytes32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl Serialize for Bytes32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Bytes32, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = URL_SAFE_NO_PAD.decode(&text).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map(Bytes32)
            .map_err(|_| D::Error::custom("expected 32 bytes"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bytes64(pub [u8; 64]);

impl fmt::Debug for Bytes64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl Serialize for Bytes64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes64 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Bytes64, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = URL_SAFE_NO_PAD.decode(&text).map_err(D::Error::custom)?;
        bytes
            .try_into()
            .map(Bytes64)
            .map_err(|_| D::Error::custom("expected 64 bytes"))
    }
}

pub fn code_from_random(random: [u8; CODE_CHARS]) -> String {
    let symbols: String = random
        .iter()
        .map(|byte| CODE_ALPHABET[usize::from(byte & 31)] as char)
        .collect();
    format!("{}-{}", &symbols[..4], &symbols[4..])
}

pub fn normalize_code(typed: &str) -> Option<String> {
    let symbols: Vec<u8> = typed
        .bytes()
        .filter(|byte| !matches!(byte, b'-' | b' '))
        .map(|byte| match byte.to_ascii_uppercase() {
            b'O' => b'0',
            b'I' | b'L' => b'1',
            other => other,
        })
        .collect();
    if symbols.len() != CODE_CHARS || !symbols.iter().all(|byte| CODE_ALPHABET.contains(byte)) {
        return None;
    }
    let text = String::from_utf8(symbols).expect("the alphabet is ASCII");
    Some(format!("{}-{}", &text[..4], &text[4..]))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// The code of the 402 a command gate answers with; kebbie matches it, so it never changes.
pub const PLAN_INACTIVE: &str = "plan-inactive";
// The code of the 402 for one action the account's plan does not include, such as a push or a second machine.
pub const PRO_REQUIRED: &str = "pro-required";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum PlanState {
    Free,
    Trial { until: u64 },
    Pro,
    Expired { until: u64, trial: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubPlan {
    #[serde(flatten)]
    pub state: PlanState,
    // The command kinds that need Pro; every other kind is free.
    pub pro: Vec<String>,
    // How many machines the plan connects; None is no limit.
    pub machines: Option<u64>,
    pub trial_days: u64,
    pub billing: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinRequest {
    pub code: String,
    pub name: String,
    pub keys: PublicKeys,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Joined {
    pub machine: Id,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineName {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardUpload {
    pub rev: u64,
    pub envelope: Envelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardStored {
    pub rev: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub seq: u64,
    pub envelope: Envelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inbox {
    pub seq: u64,
    pub messages: Vec<QueuedMessage>,
    pub board_rev: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairingState {
    Open,
    Committed,
    Replied,
    Revealed,
    Admitted,
    Refused,
    Failed,
    Expired,
}

impl PairingState {
    pub fn name(self) -> &'static str {
        match self {
            PairingState::Open => "open",
            PairingState::Committed => "committed",
            PairingState::Replied => "replied",
            PairingState::Revealed => "revealed",
            PairingState::Admitted => "admitted",
            PairingState::Refused => "refused",
            PairingState::Failed => "failed",
            PairingState::Expired => "expired",
        }
    }

    pub fn parse(name: &str) -> Option<PairingState> {
        [
            PairingState::Open,
            PairingState::Committed,
            PairingState::Replied,
            PairingState::Revealed,
            PairingState::Admitted,
            PairingState::Refused,
            PairingState::Failed,
            PairingState::Expired,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }

    pub fn is_final(self) -> bool {
        matches!(
            self,
            PairingState::Admitted
                | PairingState::Refused
                | PairingState::Failed
                | PairingState::Expired
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PairingFailure {
    Mismatch,
    Declined,
    Timeout,
}

impl PairingFailure {
    pub fn name(self) -> &'static str {
        match self {
            PairingFailure::Mismatch => "mismatch",
            PairingFailure::Declined => "declined",
            PairingFailure::Timeout => "timeout",
        }
    }

    pub fn parse(name: &str) -> Option<PairingFailure> {
        [
            PairingFailure::Mismatch,
            PairingFailure::Declined,
            PairingFailure::Timeout,
        ]
        .into_iter()
        .find(|failure| failure.name() == name)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingView {
    pub pairing: String,
    pub state: PairingState,
    pub reason: Option<PairingFailure>,
    pub expires_at: u64,
    pub machine: Id,
    pub commitment: Option<Bytes32>,
    pub machine_keys: Option<PublicKeys>,
    pub machine_nonce: Option<Bytes32>,
    pub machine_name: Option<String>,
    pub account: Id,
    pub account_key: Option<AccountKey>,
    pub account_nonce: Option<Bytes32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingReply {
    pub keys: PublicKeys,
    pub nonce: Bytes32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingRefusal {
    pub reason: PairingFailure,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountReveal {
    pub key: AccountKey,
    pub nonce: Bytes32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardEntry {
    pub machine: Id,
    pub name: String,
    pub last_seen: Option<u64>,
    pub machine_keys: PublicKeys,
    pub rev: Option<u64>,
    pub stored_at: Option<u64>,
    pub envelope: Option<Envelope>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedMachineList {
    pub list: MachineList,
    pub signature: Bytes64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Boards {
    pub boards: Vec<BoardEntry>,
    pub list: Option<SignedMachineList>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RegisterBegin {
    #[serde(default)]
    pub invite: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoginBegin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreationCeremony {
    pub ceremony: String,
    pub options: CreationChallengeResponse,
    pub prf_salt: Bytes32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestCeremony {
    pub ceremony: String,
    pub options: RequestChallengeResponse,
    #[serde(default)]
    pub prf_salts: BTreeMap<String, Bytes32>,
    pub prf_salt: Bytes32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unlocking {
    pub credential: String,
    pub prf_salt: Bytes32,
    pub wrapped: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedIn {
    #[serde(flatten)]
    pub account: AccountView,
    pub unlock: Unlocking,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountKeySetup {
    pub key: AccountKey,
    pub credential: String,
    pub wrapped: String,
    pub signature: Bytes64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prf_salt: Option<Bytes32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WrapperUpload {
    pub key_version: u32,
    pub wrapped: String,
    pub signature: Bytes64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prf_salt: Option<Bytes32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterFinish {
    pub ceremony: String,
    pub credential: RegisterPublicKeyCredential,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginFinish {
    pub ceremony: String,
    pub credential: PublicKeyCredential,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    pub account: String,
    pub handle: String,
    pub created_at: u64,
    #[serde(default)]
    pub owner: bool,
    #[serde(default)]
    pub github: Option<String>,
    #[serde(default)]
    pub key: Option<AccountKey>,
    // False for a session email or GitHub opened: it reaches the account, and what needs the account key waits for a passkey.
    #[serde(default = "key_access_default")]
    pub key_access: bool,
    // Set by a hub with plans; a hub without them has no limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<HubPlan>,
}

fn key_access_default() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignupMode {
    Invite,
    Open,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubInfo {
    pub signup: SignupMode,
    // kebbie releases that read this answer require the field; it says whether the hub has plans.
    pub plans: bool,
    pub needs_setup: bool,
    #[serde(default)]
    pub github: bool,
    #[serde(default)]
    pub commands: bool,
    // Set when the embedding service keeps account email and serves /v1/a/email.
    #[serde(default)]
    pub email: bool,
    // Set when the embedding service signs people in by email code; a new account from `kebbie hub connect` then starts with an email or GitHub.
    #[serde(default)]
    pub email_signin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vapid: Option<String>,
}

pub const MAX_EMAIL_CHARS: usize = 254;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountEmail {
    pub email: Option<String>,
    pub verified: bool,
    #[serde(default)]
    pub sent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailUpdate {
    pub email: String,
}

pub fn clean_email(text: &str) -> Option<String> {
    let email = text.trim();
    let (local, domain) = email.rsplit_once('@')?;
    let valid = email.chars().count() <= MAX_EMAIL_CHARS
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|char| char.is_control() || char.is_whitespace() || matches!(char, '<' | '>' | ',' | ';' | '"'));
    valid.then(|| email.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyView {
    pub passkey: String,
    pub name: Option<String>,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    #[serde(default)]
    pub unlocks: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Passkeys {
    pub passkeys: Vec<PasskeyView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandUpload {
    pub envelope: Envelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandQueued {
    pub command: Id,
    pub seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultUpload {
    pub envelope: Envelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultStored {
    pub command: Id,
    pub seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultEntry {
    pub seq: u64,
    pub machine: Id,
    pub command: Id,
    pub stored_at: u64,
    pub envelope: Envelope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Results {
    pub cursor: u64,
    pub results: Vec<ResultEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceUpload {
    pub cert: DeviceCert,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub device: Id,
    pub name: String,
    pub kind: DeviceKind,
    pub signing: Bytes32,
    pub fingerprint: String,
    pub cert: DeviceCert,
    pub created_at: u64,
    pub last_seen_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<DevicePush>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicePush {
    pub categories: Vec<NoticeCategory>,
    pub subscribed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushSubscribe {
    pub endpoint: String,
    pub keys: PushKeys,
    pub categories: Vec<NoticeCategory>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoticeCategories {
    pub categories: Vec<NoticeCategory>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifiedDevice {
    pub cert: DeviceCert,
    #[serde(default)]
    pub notifications: Option<Vec<NoticeCategory>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifiedDevices {
    pub devices: Vec<NotifiedDevice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoticeUpload {
    pub envelope: Envelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticePosted {
    pub notice: Id,
    pub pushes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Devices {
    pub devices: Vec<DeviceView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCommand {
    pub command: Id,
    pub machine: Id,
    pub kind: String,
    pub queued_at: u64,
    pub delivered_at: Option<u64>,
    pub envelope: Envelope,
    pub result: Option<Envelope>,
    pub result_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCommands {
    pub commands: Vec<DeviceCommand>,
    pub cursor: Option<u64>,
}

pub const CONNECT_PENDING: &str = "authorization_pending";
pub const CONNECT_SLOW_DOWN: &str = "slow_down";
pub const CONNECT_DENIED: &str = "access_denied";
pub const CONNECT_EXPIRED: &str = "expired_token";
pub const CONNECT_UNKNOWN: &str = "invalid_grant";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectStart {
    pub name: String,
    pub keys: PublicKeys,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectStarted {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectPoll {
    pub device_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connected {
    pub machine: Id,
    pub token: String,
    pub pairing: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectState {
    Pending,
    Approved,
    Denied,
}

impl ConnectState {
    pub fn name(self) -> &'static str {
        match self {
            ConnectState::Pending => "pending",
            ConnectState::Approved => "approved",
            ConnectState::Denied => "denied",
        }
    }

    pub fn parse(name: &str) -> Option<ConnectState> {
        [
            ConnectState::Pending,
            ConnectState::Approved,
            ConnectState::Denied,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectRequestView {
    pub code: String,
    pub name: String,
    pub state: ConnectState,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectApproval {
    pub code: String,
    #[serde(default)]
    pub commitment: Option<Bytes32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectApproved {
    pub machine: Id,
    pub name: String,
    pub pairing: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectDenial {
    pub code: String,
    // True when the person stops because a second machine is part of Pro; the waiting machine then hears the 402.
    #[serde(default)]
    pub pro: bool,
}

pub const EVENT_BOARD: &str = "board";
pub const EVENT_MACHINE_REMOVED: &str = "machine-removed";
pub const EVENT_RESULT: &str = "result";
pub const EVENT_MACHINES: &str = "machines";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachinesChanged {
    pub counter: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultReady {
    pub machine: Id,
    pub command: Id,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardChanged {
    pub machine: Id,
    pub rev: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineRemoved {
    pub machine: Id,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_survive_the_way_people_type_them() {
        let code = code_from_random([0, 1, 2, 3, 31, 30, 29, 28]);
        assert_eq!(code, "0123-ZYXW");
        assert_eq!(normalize_code("o123 zyxw").as_deref(), Some("0123-ZYXW"));
        assert_eq!(normalize_code("0l23-ZYXW").as_deref(), Some("0123-ZYXW"));
        assert_eq!(normalize_code("0123-ZYXU"), None);
        assert_eq!(normalize_code("0123-ZYX"), None);
    }

    #[test]
    fn an_email_is_one_address_or_nothing() {
        assert_eq!(clean_email(" ada@example.com ").as_deref(), Some("ada@example.com"));
        for refused in ["", "ada", "@example.com", "ada@example", "ada@.com", "ada@example.", "a da@example.com", "Ada <ada@example.com>", "ada@example.com,bob@example.com", "ada@example.com\r\nBcc: x@y.z"] {
            assert_eq!(clean_email(refused), None, "{refused}");
        }
        assert_eq!(clean_email(&format!("{}@example.com", "a".repeat(250))), None);
    }

    // The HubInfo released kebbie 0.37 parses, from kebbie-hub 8d64e3f: no deny_unknown_fields, so added fields pass.
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct ReleasedHubInfo {
        signup: SignupMode,
        plans: bool,
        needs_setup: bool,
        #[serde(default)]
        github: bool,
    }

    #[test]
    fn released_kebbie_still_reads_a_hub_info_that_advertises_email() {
        let info = HubInfo { signup: SignupMode::Open, plans: true, needs_setup: false, github: true, commands: true, email: true, email_signin: true, vapid: Some("BAAA".into()) };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"email\":true") && json.contains("\"email_signin\":true"), "{json}");
        let released: ReleasedHubInfo = serde_json::from_str(&json).unwrap();
        assert!(released.plans && released.github);
        let older: HubInfo = serde_json::from_str(r#"{"signup":"invite","plans":false,"needs_setup":true}"#).unwrap();
        assert!(!older.email && !older.email_signin);
    }

    #[test]
    fn an_account_view_without_key_access_reads_as_a_passkey_session() {
        let view: AccountView = serde_json::from_str(r#"{"account":"acc_x","handle":"k1","created_at":1}"#).unwrap();
        assert!(view.key_access);
    }
}
