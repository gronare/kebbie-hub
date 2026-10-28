use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kebbie_hub_protocol::{
    AccountKey, BugReport, Envelope, ErrorSignals, Id, MachineList, PublicKeys,
};
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

pub const PLAN_INACTIVE: &str = "plan-inactive";

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
    SecondCommitment,
}

impl PairingFailure {
    pub fn name(self) -> &'static str {
        match self {
            PairingFailure::Mismatch => "mismatch",
            PairingFailure::Declined => "declined",
            PairingFailure::Timeout => "timeout",
            PairingFailure::SecondCommitment => "second-commitment",
        }
    }

    pub fn parse(name: &str) -> Option<PairingFailure> {
        [
            PairingFailure::Mismatch,
            PairingFailure::Declined,
            PairingFailure::Timeout,
            PairingFailure::SecondCommitment,
        ]
        .into_iter()
        .find(|failure| failure.name() == name)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingOpened {
    pub pairing: String,
    pub code: String,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairingView {
    pub pairing: String,
    pub state: PairingState,
    pub reason: Option<PairingFailure>,
    pub expires_at: u64,
    pub machine: Id,
    pub device: Option<Id>,
    pub commitment: Option<Bytes32>,
    pub machine_keys: Option<PublicKeys>,
    pub machine_nonce: Option<Bytes32>,
    pub device_keys: Option<PublicKeys>,
    pub device_nonce: Option<Bytes32>,
    #[serde(default)]
    pub machine_name: Option<String>,
    #[serde(default)]
    pub account: Option<Id>,
    #[serde(default)]
    pub account_key: Option<AccountKey>,
    #[serde(default)]
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
pub struct CommitRequest {
    pub code: String,
    pub commitment: Bytes32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Committed {
    pub pairing: String,
    pub device: Id,
    pub token: Option<String>,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevealRequest {
    pub keys: PublicKeys,
    pub nonce: Bytes32,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WrapperUpload {
    pub key_version: u32,
    pub wrapped: String,
    pub signature: Bytes64,
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
    pub email: Option<String>,
    #[serde(default)]
    pub github: Option<String>,
    #[serde(default)]
    pub plan: PlanView,
    #[serde(default)]
    pub billing_url: Option<String>,
    #[serde(default)]
    pub key: Option<AccountKey>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    #[default]
    Active,
    Inactive,
}

impl PlanStatus {
    pub fn name(self) -> &'static str {
        match self {
            PlanStatus::Active => "active",
            PlanStatus::Inactive => "inactive",
        }
    }

    pub fn parse(name: &str) -> Option<PlanStatus> {
        [PlanStatus::Active, PlanStatus::Inactive]
            .into_iter()
            .find(|status| status.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanView {
    pub status: PlanStatus,
    pub until: Option<u64>,
    pub entitled: bool,
}

impl Default for PlanView {
    fn default() -> PlanView {
        PlanView {
            status: PlanStatus::Active,
            until: None,
            entitled: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanUpdate {
    pub status: PlanStatus,
    #[serde(default)]
    pub until: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPlan {
    pub account: String,
    pub handle: String,
    pub email: Option<String>,
    pub plan: PlanView,
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
    pub plans: bool,
    pub needs_setup: bool,
    #[serde(default)]
    pub github: bool,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceRequest {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceRegistered {
    pub device: Id,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceView {
    pub device: Id,
    pub name: Option<String>,
    pub created_at: u64,
    pub last_seen: Option<u64>,
    pub machines: Vec<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Devices {
    pub devices: Vec<DeviceView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmittedDevice {
    pub device: Id,
    pub name: Option<String>,
    pub paired_at: u64,
    pub last_seen: Option<u64>,
    pub notifications: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdmittedDevices {
    pub devices: Vec<AdmittedDevice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceName {
    pub name: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: PushKeys,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VapidKey {
    pub key: String,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportChallenge {
    pub nonce: Bytes32,
    pub difficulty: u8,
    pub expires: u64,
    pub tag: Bytes32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportUpload {
    pub report: BugReport,
    pub key: Bytes32,
    pub signature: Bytes64,
    pub challenge: ReportChallenge,
    pub counter: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportAccepted {
    pub report: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedReport {
    pub id: u64,
    pub key: Bytes32,
    pub received_at: u64,
    pub report: BugReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportInbox {
    pub reports: Vec<ReceivedReport>,
    pub quarantined: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalUpload {
    pub signals: ErrorSignals,
    pub key: Bytes32,
    pub signature: Bytes64,
    pub challenge: ReportChallenge,
    pub counter: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalsAccepted {
    pub rows: usize,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingAccount {
    pub account: String,
    pub handle: String,
    pub github: Option<String>,
    pub created_at: u64,
    pub deactivated_at: Option<u64>,
    pub plan: PlanView,
    pub machines: u64,
    pub active_machines: u64,
    pub last_seen: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BillingAccounts {
    pub accounts: Vec<BillingAccount>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingAccountDetail {
    pub account: String,
    pub handle: String,
    pub github: Option<String>,
    pub created_at: u64,
    pub created_via: Option<String>,
    pub deactivated_at: Option<u64>,
    pub plan: PlanView,
    pub passkeys: u64,
    pub last_seen: Option<u64>,
    pub machines: Vec<BillingMachine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingMachine {
    pub machine: Id,
    pub name: String,
    pub joined_at: u64,
    pub last_seen: Option<u64>,
    pub last_board_at: Option<u64>,
}

pub const EVENT_BOARD: &str = "board";
pub const EVENT_PAIRING: &str = "pairing";
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
pub struct PairingChanged {
    pub pairing: String,
    pub state: PairingState,
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
}
