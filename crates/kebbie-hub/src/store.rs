use std::path::Path;

use kebbie_hub_api::{AdmittedDevice, ConnectState, PairingFailure, PairingState, PlanStatus};
use kebbie_hub_protocol::{
    AccountKey, BugReport, ErrorSignals, Header, Id, MachineList, PublicKeys,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

pub type Result<T> = rusqlite::Result<T>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS machines (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    signing_key BLOB NOT NULL,
    encryption_key BLOB NOT NULL,
    token_hash BLOB NOT NULL UNIQUE,
    last_seq INTEGER NOT NULL DEFAULT 0,
    joined_at INTEGER NOT NULL,
    last_seen INTEGER
);
CREATE TABLE IF NOT EXISTS join_codes (
    code_hash BLOB PRIMARY KEY,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE,
    user_handle BLOB NOT NULL UNIQUE,
    github_id INTEGER UNIQUE,
    email TEXT UNIQUE,
    email_verified_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS invites (
    code_hash BLOB PRIMARY KEY,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS passkeys (
    credential_id BLOB PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT,
    passkey TEXT NOT NULL,
    counter INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash BLOB PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ceremonies (
    id_hash BLOB PRIMARY KEY,
    purpose TEXT NOT NULL,
    account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    handle TEXT,
    user_handle BLOB,
    invite_hash BLOB,
    state TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE,
    account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT,
    signing_key BLOB,
    encryption_key BLOB,
    created_at INTEGER NOT NULL,
    last_seen INTEGER,
    last_result_seq INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS admissions (
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    admitted_at INTEGER NOT NULL,
    PRIMARY KEY (machine_id, device_id)
);
CREATE TABLE IF NOT EXISTS boards (
    machine_id TEXT PRIMARY KEY REFERENCES machines(id) ON DELETE CASCADE,
    rev INTEGER NOT NULL,
    envelope_id TEXT NOT NULL,
    envelope_at INTEGER NOT NULL,
    envelope TEXT NOT NULL,
    stored_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS queue (
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    envelope TEXT NOT NULL,
    queued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (machine_id, seq)
);
CREATE TABLE IF NOT EXISTS commands (
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    command_id TEXT NOT NULL,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    digest BLOB NOT NULL,
    seq INTEGER NOT NULL,
    queued_at INTEGER NOT NULL,
    delivered_at INTEGER,
    PRIMARY KEY (device_id, command_id),
    UNIQUE (machine_id, command_id)
);
CREATE INDEX IF NOT EXISTS commands_by_seq ON commands (machine_id, seq);
CREATE TABLE IF NOT EXISTS results (
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    command_id TEXT NOT NULL,
    digest BLOB NOT NULL,
    envelope TEXT NOT NULL,
    stored_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (device_id, seq),
    UNIQUE (device_id, command_id)
);
CREATE TABLE IF NOT EXISTS pairings (
    id TEXT PRIMARY KEY,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    code_hash BLOB NOT NULL UNIQUE,
    state TEXT NOT NULL,
    reason TEXT,
    device_id TEXT REFERENCES devices(id) ON DELETE SET NULL,
    commitment BLOB,
    machine_signing BLOB,
    machine_encryption BLOB,
    machine_nonce BLOB,
    device_signing BLOB,
    device_encryption BLOB,
    device_nonce BLOB,
    opened_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS push_subscriptions (
    device_id TEXT PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    endpoint TEXT NOT NULL UNIQUE,
    p256dh BLOB NOT NULL,
    auth BLOB NOT NULL,
    subscribed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS notices (
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    notice_id TEXT NOT NULL,
    posted_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (machine_id, notice_id)
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    machine_id TEXT,
    device_id TEXT,
    detail TEXT
);
";

const REPORTS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS report_keys (
    key BLOB PRIMARY KEY,
    status TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key BLOB NOT NULL REFERENCES report_keys(key) ON DELETE CASCADE,
    title TEXT NOT NULL,
    text TEXT NOT NULL,
    version TEXT NOT NULL,
    os TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    acked_at INTEGER
);
CREATE INDEX IF NOT EXISTS reports_by_key ON reports (key);
CREATE TABLE IF NOT EXISTS report_challenges (
    nonce BLOB PRIMARY KEY,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS admin_tokens (
    token_hash BLOB PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_used INTEGER
);
";

const SIGNALS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS signal_counts (
    day INTEGER NOT NULL,
    version TEXT NOT NULL,
    commit_id TEXT NOT NULL,
    os TEXT NOT NULL,
    arch TEXT NOT NULL,
    component TEXT NOT NULL,
    kind TEXT NOT NULL,
    vetted INTEGER NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, version, commit_id, os, arch, component, kind, vetted)
);
CREATE TABLE IF NOT EXISTS signal_senders (
    key BLOB NOT NULL,
    day INTEGER NOT NULL,
    batches INTEGER NOT NULL,
    PRIMARY KEY (key, day)
);
";

const CONNECT_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS connects (
    device_code_hash BLOB PRIMARY KEY,
    user_code_hash BLOB NOT NULL UNIQUE,
    name TEXT NOT NULL,
    signing_key BLOB NOT NULL,
    encryption_key BLOB NOT NULL,
    state TEXT NOT NULL,
    account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    machine_id TEXT,
    pairing_id TEXT,
    poll_interval INTEGER NOT NULL,
    polled_at INTEGER,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
";

const CONNECT_COLUMNS: &[(&str, &str, &str)] = &[
    ("accounts", "role", "TEXT NOT NULL DEFAULT 'member'"),
    ("accounts", "plan", "TEXT NOT NULL DEFAULT 'inactive'"),
    ("accounts", "plan_until", "INTEGER"),
    ("accounts", "deactivated_at", "INTEGER"),
    (
        "machines",
        "account_id",
        "TEXT REFERENCES accounts(id) ON DELETE SET NULL",
    ),
    ("invites", "kind", "TEXT NOT NULL DEFAULT 'invite'"),
    ("invites", "email", "TEXT"),
    ("admin_tokens", "scope", "TEXT NOT NULL DEFAULT 'reports'"),
];

fn add_columns(connection: &Connection, columns: &[(&str, &str, &str)]) -> Result<Vec<String>> {
    let mut added = Vec::new();
    for (table, column, definition) in columns {
        let present: bool = connection.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
            [column],
            |row| row.get(0),
        )?;
        if !present {
            connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
            added.push(format!("{table}.{column}"));
        }
    }
    Ok(added)
}

const ACCOUNT_KEY_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS account_keys (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key_version INTEGER NOT NULL,
    signing_key BLOB NOT NULL,
    encryption_key BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, key_version)
);
CREATE TABLE IF NOT EXISTS key_wrappers (
    credential_id BLOB PRIMARY KEY REFERENCES passkeys(credential_id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key_version INTEGER NOT NULL,
    wrapped BLOB NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS machine_lists (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    counter INTEGER NOT NULL,
    list TEXT NOT NULL,
    signature BLOB NOT NULL
);
";

const ACCOUNT_KEY_COLUMNS: &[(&str, &str, &str)] = &[
    ("passkeys", "prf_salt", "BLOB"),
    (
        "pairings",
        "account_id",
        "TEXT REFERENCES accounts(id) ON DELETE CASCADE",
    ),
    ("pairings", "account_key_version", "INTEGER"),
    ("ceremonies", "prf_salt", "BLOB"),
    ("connects", "signup_at", "INTEGER"),
];

const GITHUB_COLUMNS: &[(&str, &str, &str)] = &[("accounts", "github_login", "TEXT")];

const CREATED_VIA_COLUMNS: &[(&str, &str, &str)] = &[("accounts", "created_via", "TEXT")];

const SCHEMA_VERSION: i64 = 10;
const UPGRADES: &[(i64, &str)] = &[
    (5, REPORTS_SCHEMA),
    (6, SIGNALS_SCHEMA),
    (7, CONNECT_SCHEMA),
    (8, ACCOUNT_KEY_SCHEMA),
    (9, ""),
    (10, ""),
];
pub const REPORT_KEEP_SECONDS: u64 = 30 * 86_400;
pub const SIGNAL_KEEP_DAYS: u64 = 180;
pub const COMMAND_KEEP_SECONDS: u64 = 7 * 86_400;
const PAIRING_KEEP_SECONDS: u64 = 3_600;
const EVENT_KEEP_SECONDS: u64 = 30 * 86_400;
pub const DEVICE_SEEN_EVERY: u64 = 60;

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Debug)]
pub struct Pairing {
    pub id: String,
    pub machine: Id,
    pub state: PairingState,
    pub reason: Option<PairingFailure>,
    pub device: Option<Id>,
    pub commitment: Option<[u8; 32]>,
    pub machine_keys: Option<PublicKeys>,
    pub machine_nonce: Option<[u8; 32]>,
    pub device_keys: Option<PublicKeys>,
    pub device_nonce: Option<[u8; 32]>,
    pub account: Option<String>,
    pub account_key_version: Option<u32>,
    pub expires_at: u64,
}

impl Pairing {
    pub fn state_at(&self, now: u64) -> PairingState {
        if !self.state.is_final() && now >= self.expires_at {
            PairingState::Expired
        } else {
            self.state
        }
    }
}

pub enum BoardWrite {
    Stored,
    Stale,
    Replayed,
}

pub struct NewCommand<'a> {
    pub device: &'a Id,
    pub command: &'a Id,
    pub machine: &'a Id,
    pub kind: &'a str,
    pub digest: &'a [u8],
    pub envelope: &'a str,
    pub now: u64,
    pub expires_at: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandWrite {
    Queued(u64),
    Resent(u64),
    IdTaken,
}

pub struct NewResult<'a> {
    pub device: &'a Id,
    pub machine: &'a Id,
    pub command: &'a Id,
    pub digest: &'a [u8],
    pub envelope: &'a str,
    pub now: u64,
    pub expires_at: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResultWrite {
    Stored(u64),
    Resent(u64),
    Differs,
}

pub struct DeliveredCommand {
    pub device: Id,
    pub delivered: bool,
}

pub struct ResultHead {
    pub seq: u64,
    pub machine: Id,
    pub command: Id,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Subscribed {
    New,
    Moved(Id),
    Taken,
}

pub struct StoredSubscription {
    pub endpoint: String,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
}

pub struct StoredResult {
    pub seq: u64,
    pub machine: Id,
    pub command: Id,
    pub stored_at: u64,
    pub envelope: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let tables: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;
        let fresh = version == 0 && tables == 0;
        let upgradable = UPGRADES
            .iter()
            .skip_while(|(to, _)| *to != version + 1)
            .map(|(to, _)| *to)
            .last()
            == Some(SCHEMA_VERSION);
        if version != SCHEMA_VERSION && !fresh && !upgradable {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISMATCH),
                Some(format!(
                    "{} was written by another kebbie-hub version (schema {version}); start with an empty data directory",
                    path.display()
                )),
            ));
        }
        let mut connection = connection;
        let tx = connection.transaction()?;
        if fresh {
            tx.execute_batch(SCHEMA)?;
        }
        let from = if fresh { 4 } else { version };
        for (to, upgrade) in UPGRADES.iter().filter(|(to, _)| *to > from) {
            tx.execute_batch(upgrade)?;
            if *to == 7 {
                let added = add_columns(&tx, CONNECT_COLUMNS)?;
                if added.iter().any(|added| added == "accounts.plan") {
                    tx.execute("UPDATE accounts SET plan = 'active'", [])?;
                }
                if added.iter().any(|added| added == "accounts.role") {
                    tx.execute(
                        "UPDATE accounts SET role = 'owner' WHERE id = (SELECT id FROM accounts ORDER BY created_at, rowid LIMIT 1)",
                        [],
                    )?;
                }
            }
            if *to == 8 {
                add_columns(&tx, ACCOUNT_KEY_COLUMNS)?;
                tx.execute("DELETE FROM invites WHERE kind = 'email'", [])?;
                let unsalted: Vec<Vec<u8>> = tx
                    .prepare("SELECT credential_id FROM passkeys WHERE prf_salt IS NULL")?
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<_>>()?;
                for credential in unsalted {
                    tx.execute(
                        "UPDATE passkeys SET prf_salt = ?2 WHERE credential_id = ?1",
                        params![credential, crate::auth::random::<32>().as_slice()],
                    )?;
                }
            }
            if *to == 9 {
                add_columns(&tx, GITHUB_COLUMNS)?;
            }
            if *to == 10 {
                add_columns(&tx, CREATED_VIA_COLUMNS)?;
            }
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        Ok(Store { connection })
    }

    pub fn add_join_code(&self, code_hash: &[u8], expires_at: u64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO join_codes (code_hash, expires_at) VALUES (?1, ?2)",
            params![code_hash, expires_at],
        )?;
        Ok(())
    }

    pub fn join(
        &mut self,
        code_hash: &[u8],
        now: u64,
        machine: &Id,
        name: &str,
        keys: &PublicKeys,
        token_hash: &[u8],
    ) -> Result<bool> {
        let tx = self.connection.transaction()?;
        let used = tx.execute(
            "DELETE FROM join_codes WHERE code_hash = ?1 AND expires_at > ?2",
            params![code_hash, now],
        )?;
        if used == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO machines (id, name, signing_key, encryption_key, token_hash, joined_at, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![machine.as_str(), name, keys.signing, keys.encryption, token_hash, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn machine_by_token(&self, token_hash: &[u8]) -> Result<Option<Id>> {
        self.connection
            .query_row(
                "SELECT id FROM machines WHERE token_hash = ?1 AND NOT EXISTS (
                     SELECT 1 FROM accounts WHERE accounts.id = machines.account_id
                         AND accounts.deactivated_at IS NOT NULL)",
                [token_hash],
                |row| id_at(row, 0),
            )
            .optional()
    }

    pub fn machine_keys(&self, machine: &Id) -> Result<Option<PublicKeys>> {
        self.connection
            .query_row(
                "SELECT signing_key, encryption_key FROM machines WHERE id = ?1",
                [machine.as_str()],
                |row| keys_at(row, 0),
            )
            .optional()
    }

    pub fn device_by_token(&self, token_hash: &[u8]) -> Result<Option<Id>> {
        self.connection
            .query_row(
                "SELECT id FROM devices WHERE token_hash = ?1 AND NOT EXISTS (
                     SELECT 1 FROM accounts WHERE accounts.id = devices.account_id
                         AND accounts.deactivated_at IS NOT NULL)",
                [token_hash],
                |row| id_at(row, 0),
            )
            .optional()
    }

    pub fn machine_name(&self, machine: &Id) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT name FROM machines WHERE id = ?1",
                [machine.as_str()],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn rename_machine(&self, machine: &Id, name: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE machines SET name = ?2 WHERE id = ?1",
            params![machine.as_str(), name],
        )? == 1)
    }

    pub fn touch_machine(&self, machine: &Id, now: u64) -> Result<()> {
        self.connection.execute(
            "UPDATE machines SET last_seen = ?2 WHERE id = ?1",
            params![machine.as_str(), now],
        )?;
        Ok(())
    }

    pub fn touch_device(&self, device: &Id, now: u64) -> Result<()> {
        self.connection.execute(
            "UPDATE devices SET last_seen = ?2 WHERE id = ?1 AND (last_seen IS NULL OR last_seen + ?3 <= ?2)",
            params![device.as_str(), now, DEVICE_SEEN_EVERY],
        )?;
        Ok(())
    }

    pub fn board_rev(&self, machine: &Id) -> Result<Option<u64>> {
        self.connection
            .query_row(
                "SELECT rev FROM boards WHERE machine_id = ?1",
                [machine.as_str()],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn store_board(
        &mut self,
        machine: &Id,
        rev: u64,
        header: &Header,
        envelope: &str,
        now: u64,
    ) -> Result<BoardWrite> {
        let tx = self.connection.transaction()?;
        let current: Option<(u64, String, u64)> = tx
            .query_row(
                "SELECT rev, envelope_id, envelope_at FROM boards WHERE machine_id = ?1",
                [machine.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((current_rev, current_id, current_at)) = current {
            if current_id == header.id.as_str() || header.at < current_at {
                return Ok(BoardWrite::Replayed);
            }
            if header.at == current_at && rev <= current_rev {
                return Ok(BoardWrite::Stale);
            }
        }
        tx.execute(
            "INSERT INTO boards (machine_id, rev, envelope_id, envelope_at, envelope, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (machine_id) DO UPDATE SET
                 rev = ?2, envelope_id = ?3, envelope_at = ?4, envelope = ?5, stored_at = ?6",
            params![
                machine.as_str(),
                rev,
                header.id.as_str(),
                header.at,
                envelope,
                now
            ],
        )?;
        tx.commit()?;
        Ok(BoardWrite::Stored)
    }

    pub fn enqueue(
        &mut self,
        machine: &Id,
        envelope: &str,
        now: u64,
        expires_at: u64,
    ) -> Result<u64> {
        let tx = self.connection.transaction()?;
        let seq: u64 = tx.query_row(
            "UPDATE machines SET last_seq = last_seq + 1 WHERE id = ?1 RETURNING last_seq",
            [machine.as_str()],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO queue (machine_id, seq, envelope, queued_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![machine.as_str(), seq, envelope, now, expires_at],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    pub fn is_admitted(&self, machine: &Id, device: &Id) -> Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM admissions WHERE machine_id = ?1 AND device_id = ?2)",
            params![machine.as_str(), device.as_str()],
            |row| row.get(0),
        )
    }

    pub fn queued_count(&self, machine: &Id) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM queue WHERE machine_id = ?1",
            [machine.as_str()],
            |row| row.get(0),
        )
    }

    pub fn undelivered_count(&self, machine: &Id, device: &Id) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM commands JOIN queue
                 ON queue.machine_id = commands.machine_id AND queue.seq = commands.seq
             WHERE commands.machine_id = ?1 AND commands.device_id = ?2
                 AND commands.delivered_at IS NULL",
            params![machine.as_str(), device.as_str()],
            |row| row.get(0),
        )
    }

    pub fn resent_command(&self, device: &Id, command: &Id) -> Result<Option<(Vec<u8>, u64)>> {
        self.connection
            .query_row(
                "SELECT digest, seq FROM commands WHERE device_id = ?1 AND command_id = ?2",
                params![device.as_str(), command.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    pub fn queue_command(&mut self, new: &NewCommand<'_>) -> Result<CommandWrite> {
        let tx = self.connection.transaction()?;
        let held: Option<(String, Vec<u8>, u64)> = tx
            .query_row(
                "SELECT device_id, digest, seq FROM commands WHERE machine_id = ?1 AND command_id = ?2",
                params![new.machine.as_str(), new.command.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((device, digest, seq)) = held {
            return Ok(if device == new.device.as_str() && digest == new.digest {
                CommandWrite::Resent(seq)
            } else {
                CommandWrite::IdTaken
            });
        }
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM commands WHERE device_id = ?1 AND command_id = ?2)",
            params![new.device.as_str(), new.command.as_str()],
            |row| row.get(0),
        )?;
        if taken {
            return Ok(CommandWrite::IdTaken);
        }
        let seq: u64 = tx.query_row(
            "UPDATE machines SET last_seq = last_seq + 1 WHERE id = ?1 RETURNING last_seq",
            [new.machine.as_str()],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO queue (machine_id, seq, envelope, queued_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                new.machine.as_str(),
                seq,
                new.envelope,
                new.now,
                new.expires_at
            ],
        )?;
        tx.execute(
            "INSERT INTO commands (device_id, command_id, machine_id, kind, digest, seq, queued_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                new.device.as_str(),
                new.command.as_str(),
                new.machine.as_str(),
                new.kind,
                new.digest,
                seq,
                new.now
            ],
        )?;
        tx.commit()?;
        Ok(CommandWrite::Queued(seq))
    }

    pub fn mark_delivered(&self, machine: &Id, after: u64, through: u64, now: u64) -> Result<()> {
        self.connection.execute(
            "UPDATE commands SET delivered_at = ?4
             WHERE machine_id = ?1 AND seq > ?2 AND seq <= ?3 AND delivered_at IS NULL",
            params![machine.as_str(), after, through, now],
        )?;
        Ok(())
    }

    pub fn delivered_command(
        &self,
        machine: &Id,
        command: &Id,
    ) -> Result<Option<DeliveredCommand>> {
        self.connection
            .query_row(
                "SELECT device_id, delivered_at IS NOT NULL FROM commands
                 WHERE machine_id = ?1 AND command_id = ?2",
                params![machine.as_str(), command.as_str()],
                |row| {
                    Ok(DeliveredCommand {
                        device: id_at(row, 0)?,
                        delivered: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    pub fn store_result(&mut self, new: &NewResult<'_>) -> Result<ResultWrite> {
        let tx = self.connection.transaction()?;
        let held: Option<(Vec<u8>, u64)> = tx
            .query_row(
                "SELECT digest, seq FROM results WHERE device_id = ?1 AND command_id = ?2",
                params![new.device.as_str(), new.command.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((digest, seq)) = held {
            return Ok(if digest == new.digest {
                ResultWrite::Resent(seq)
            } else {
                ResultWrite::Differs
            });
        }
        let seq: u64 = tx.query_row(
            "UPDATE devices SET last_result_seq = last_result_seq + 1 WHERE id = ?1
             RETURNING last_result_seq",
            [new.device.as_str()],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO results (device_id, seq, machine_id, command_id, digest, envelope, stored_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                new.device.as_str(),
                seq,
                new.machine.as_str(),
                new.command.as_str(),
                new.digest,
                new.envelope,
                new.now,
                new.expires_at
            ],
        )?;
        tx.commit()?;
        Ok(ResultWrite::Stored(seq))
    }

    pub fn results_after(&self, device: &Id, after: u64, now: u64) -> Result<Vec<StoredResult>> {
        let mut statement = self.connection.prepare(
            "SELECT seq, machine_id, command_id, stored_at, envelope FROM results
             WHERE device_id = ?1 AND seq > ?2 AND expires_at > ?3 ORDER BY seq",
        )?;
        statement
            .query_map(params![device.as_str(), after, now], |row| {
                Ok(StoredResult {
                    seq: row.get(0)?,
                    machine: id_at(row, 1)?,
                    command: id_at(row, 2)?,
                    stored_at: row.get(3)?,
                    envelope: row.get(4)?,
                })
            })?
            .collect()
    }

    pub fn result_heads_after(&self, device: &Id, after: u64, now: u64) -> Result<Vec<ResultHead>> {
        let mut statement = self.connection.prepare(
            "SELECT seq, machine_id, command_id FROM results
             WHERE device_id = ?1 AND seq > ?2 AND expires_at > ?3 ORDER BY seq",
        )?;
        statement
            .query_map(params![device.as_str(), after, now], |row| {
                Ok(ResultHead {
                    seq: row.get(0)?,
                    machine: id_at(row, 1)?,
                    command: id_at(row, 2)?,
                })
            })?
            .collect()
    }

    pub fn acknowledge(&self, machine: &Id, through: u64) -> Result<()> {
        self.connection.execute(
            "DELETE FROM queue WHERE machine_id = ?1 AND seq <= ?2",
            params![machine.as_str(), through],
        )?;
        Ok(())
    }

    pub fn queued_after(&self, machine: &Id, after: u64) -> Result<Vec<(u64, String)>> {
        let mut statement = self.connection.prepare(
            "SELECT seq, envelope FROM queue WHERE machine_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        statement
            .query_map(params![machine.as_str(), after], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect()
    }

    pub fn open_pairing(
        &self,
        id: &str,
        machine: &Id,
        code_hash: &[u8],
        now: u64,
        expires_at: u64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO pairings (id, machine_id, code_hash, state, opened_at, expires_at)
             VALUES (?1, ?2, ?3, 'open', ?4, ?5)",
            params![id, machine.as_str(), code_hash, now, expires_at],
        )?;
        Ok(())
    }

    pub fn pairing(&self, id: &str) -> Result<Option<Pairing>> {
        Ok(self
            .pairings_where("id = ?1", rusqlite::types::Value::Text(id.to_string()))?
            .pop())
    }

    pub fn pairing_by_code(&self, code_hash: &[u8]) -> Result<Option<Pairing>> {
        Ok(self
            .pairings_where(
                "code_hash = ?1",
                rusqlite::types::Value::Blob(code_hash.to_vec()),
            )?
            .pop())
    }

    pub fn pairings_of_device(&self, device: &Id) -> Result<Vec<Pairing>> {
        self.pairings_where(
            "device_id = ?1",
            rusqlite::types::Value::Text(device.as_str().to_string()),
        )
    }

    fn pairings_where(&self, filter: &str, value: rusqlite::types::Value) -> Result<Vec<Pairing>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT id, machine_id, state, reason, device_id, commitment,
                    machine_signing, machine_encryption, machine_nonce,
                    device_signing, device_encryption, device_nonce, expires_at,
                    account_id, account_key_version
             FROM pairings WHERE {filter} ORDER BY opened_at, id"
        ))?;
        statement
            .query_map([value], |row| {
                Ok(Pairing {
                    id: row.get(0)?,
                    machine: id_at(row, 1)?,
                    state: PairingState::parse(&row.get::<_, String>(2)?)
                        .ok_or_else(|| corrupt(2))?,
                    reason: row
                        .get::<_, Option<String>>(3)?
                        .map(|name| PairingFailure::parse(&name).ok_or_else(|| corrupt(3)))
                        .transpose()?,
                    device: optional_id_at(row, 4)?,
                    commitment: bytes32_at(row, 5)?,
                    machine_keys: optional_keys_at(row, 6)?,
                    machine_nonce: bytes32_at(row, 8)?,
                    device_keys: optional_keys_at(row, 9)?,
                    device_nonce: bytes32_at(row, 11)?,
                    expires_at: row.get(12)?,
                    account: row.get(13)?,
                    account_key_version: row.get(14)?,
                })
            })?
            .collect()
    }

    pub fn save_pairing(&self, pairing: &Pairing) -> Result<()> {
        self.connection.execute(
            "UPDATE pairings SET state = ?2, reason = ?3, device_id = ?4, commitment = ?5,
                    machine_signing = ?6, machine_encryption = ?7, machine_nonce = ?8,
                    device_signing = ?9, device_encryption = ?10, device_nonce = ?11,
                    account_key_version = ?12
             WHERE id = ?1",
            params![
                pairing.id,
                pairing.state.name(),
                pairing.reason.map(PairingFailure::name),
                pairing.device.as_ref().map(Id::as_str),
                pairing.commitment,
                pairing.machine_keys.map(|keys| keys.signing),
                pairing.machine_keys.map(|keys| keys.encryption),
                pairing.machine_nonce,
                pairing.device_keys.map(|keys| keys.signing),
                pairing.device_keys.map(|keys| keys.encryption),
                pairing.device_nonce,
                pairing.account_key_version,
            ],
        )?;
        Ok(())
    }

    pub fn create_device(
        &self,
        device: &Id,
        token_hash: &[u8],
        account: Option<&str>,
        name: Option<&str>,
        now: u64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO devices (id, token_hash, account_id, name, created_at, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![device.as_str(), token_hash, account, name, now],
        )?;
        Ok(())
    }

    pub fn device_keys(&self, device: &Id) -> Result<Option<PublicKeys>> {
        self.connection
            .query_row(
                "SELECT signing_key, encryption_key FROM devices WHERE id = ?1",
                [device.as_str()],
                |row| optional_keys_at(row, 0),
            )
            .optional()
            .map(Option::flatten)
    }

    pub fn fix_device_keys(&self, device: &Id, keys: &PublicKeys) -> Result<()> {
        self.connection.execute(
            "UPDATE devices SET signing_key = ?2, encryption_key = ?3 WHERE id = ?1 AND signing_key IS NULL",
            params![device.as_str(), keys.signing, keys.encryption],
        )?;
        Ok(())
    }

    pub fn admit(&self, machine: &Id, device: &Id, now: u64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO admissions (machine_id, device_id, admitted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (machine_id, device_id) DO UPDATE SET admitted_at = ?3",
            params![machine.as_str(), device.as_str(), now],
        )?;
        Ok(())
    }

    pub fn log(
        &self,
        now: u64,
        kind: &str,
        machine: Option<&Id>,
        device: Option<&Id>,
        detail: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO events (at, kind, machine_id, device_id, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                now,
                kind,
                machine.map(Id::as_str),
                device.map(Id::as_str),
                detail
            ],
        )?;
        Ok(())
    }

    pub fn events(&self) -> Result<Vec<(String, Option<String>)>> {
        let mut statement = self
            .connection
            .prepare("SELECT kind, detail FROM events ORDER BY id")?;
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
    }

    pub fn admitted_devices(&self, machine: &Id) -> Result<Vec<Id>> {
        let mut statement = self
            .connection
            .prepare("SELECT device_id FROM admissions WHERE machine_id = ?1")?;
        statement
            .query_map([machine.as_str()], |row| id_at(row, 0))?
            .collect()
    }

    pub fn admitted_device_views(&self, machine: &Id) -> Result<Vec<AdmittedDevice>> {
        let mut statement = self.connection.prepare(
            "SELECT devices.id, devices.name, admissions.admitted_at, devices.last_seen,
                    EXISTS (SELECT 1 FROM push_subscriptions WHERE push_subscriptions.device_id = devices.id)
             FROM admissions JOIN devices ON devices.id = admissions.device_id
             WHERE admissions.machine_id = ?1 ORDER BY admissions.admitted_at, devices.id",
        )?;
        statement
            .query_map([machine.as_str()], |row| {
                Ok(AdmittedDevice {
                    device: id_at(row, 0)?,
                    name: row.get(1)?,
                    paired_at: row.get(2)?,
                    last_seen: row.get(3)?,
                    notifications: row.get(4)?,
                })
            })?
            .collect()
    }

    pub fn rename_device(&self, device: &Id, name: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE devices SET name = ?2 WHERE id = ?1",
            params![device.as_str(), name],
        )? == 1)
    }

    pub fn device_exists(&self, device: &Id) -> Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1)",
            [device.as_str()],
            |row| row.get(0),
        )
    }

    pub fn admitting_machines(&self, device: &Id) -> Result<Vec<Id>> {
        let mut statement = self.connection.prepare(
            "SELECT machine_id FROM admissions WHERE device_id = ?1 ORDER BY machine_id",
        )?;
        statement
            .query_map([device.as_str()], |row| id_at(row, 0))?
            .collect()
    }

    pub fn heads_for_account(&self, account: &str) -> Result<Vec<BoardHead>> {
        let mut statement = self.connection.prepare(
            "SELECT machines.id, boards.envelope_id, boards.rev
             FROM machines LEFT JOIN boards ON boards.machine_id = machines.id
             WHERE machines.account_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM connects WHERE connects.machine_id = machines.id)
             ORDER BY machines.id",
        )?;
        statement
            .query_map([account], |row| {
                let envelope: Option<String> = row.get(1)?;
                let rev: Option<u64> = row.get(2)?;
                Ok(BoardHead {
                    machine: id_at(row, 0)?,
                    board: envelope.zip(rev),
                })
            })?
            .collect()
    }

    pub fn boards_for_account(&self, account: &str) -> Result<Vec<AccountBoard>> {
        let mut statement = self.connection.prepare(
            "SELECT machines.id, machines.name, machines.last_seen, machines.signing_key,
                    machines.encryption_key, boards.rev, boards.stored_at, boards.envelope
             FROM machines LEFT JOIN boards ON boards.machine_id = machines.id
             WHERE machines.account_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM connects WHERE connects.machine_id = machines.id)
             ORDER BY machines.joined_at, machines.id",
        )?;
        statement
            .query_map([account], |row| {
                let rev: Option<u64> = row.get(5)?;
                let stored_at: Option<u64> = row.get(6)?;
                let envelope: Option<String> = row.get(7)?;
                Ok(AccountBoard {
                    machine: id_at(row, 0)?,
                    name: row.get(1)?,
                    last_seen: row.get(2)?,
                    keys: keys_at(row, 3)?,
                    board: rev.zip(stored_at).zip(envelope).map(|((rev, at), text)| (rev, at, text)),
                })
            })?
            .collect()
    }

    pub fn machine_list(&self, account: &str) -> Result<Option<(MachineList, [u8; 64])>> {
        self.connection
            .query_row(
                "SELECT list, signature FROM machine_lists WHERE account_id = ?1",
                [account],
                |row| {
                    let list: MachineList = serde_json::from_str(&row.get::<_, String>(0)?)
                        .map_err(|_| corrupt(0))?;
                    let signature: [u8; 64] = row
                        .get::<_, Vec<u8>>(1)?
                        .try_into()
                        .map_err(|_| corrupt(1))?;
                    Ok((list, signature))
                },
            )
            .optional()
    }

    pub fn set_machine_list(
        &mut self,
        account: &str,
        list: &MachineList,
        signature: &[u8; 64],
    ) -> Result<ListWrite> {
        let tx = self.connection.transaction()?;
        let held: Option<u64> = tx
            .query_row(
                "SELECT counter FROM machine_lists WHERE account_id = ?1",
                [account],
                |row| row.get(0),
            )
            .optional()?;
        if held.is_some_and(|held| list.counter <= held) {
            return Ok(ListWrite::Stale);
        }
        for listed in &list.machines {
            let owned: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM machines WHERE id = ?1 AND account_id = ?2
                     AND signing_key = ?3 AND encryption_key = ?4)",
                params![listed.machine.as_str(), account, listed.keys.signing, listed.keys.encryption],
                |row| row.get(0),
            )?;
            if !owned {
                return Ok(ListWrite::UnknownMachine);
            }
        }
        tx.execute(
            "INSERT INTO machine_lists (account_id, counter, list, signature) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (account_id) DO UPDATE SET counter = ?2, list = ?3, signature = ?4",
            params![
                account,
                list.counter,
                serde_json::to_string(list).expect("a machine list serializes"),
                signature.as_slice()
            ],
        )?;
        tx.commit()?;
        Ok(ListWrite::Stored)
    }

    pub fn account_key(&self, account: &str) -> Result<Option<AccountKey>> {
        self.connection
            .query_row(
                "SELECT key_version, signing_key, encryption_key FROM account_keys
                 WHERE account_id = ?1 ORDER BY key_version DESC LIMIT 1",
                [account],
                |row| Ok((row.get::<_, u32>(0)?, keys_at(row, 1)?)),
            )
            .optional()?
            .map(|(version, keys)| {
                let id = Id::parse(account).map_err(|_| corrupt(0))?;
                AccountKey::new(id, version, keys).ok_or_else(|| corrupt(0))
            })
            .transpose()
    }

    pub fn set_account_key(
        &mut self,
        key: &AccountKey,
        credential: &[u8],
        wrapped: &[u8],
        now: u64,
    ) -> Result<KeySet> {
        let tx = self.connection.transaction()?;
        let account = key.account.as_str();
        let held: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM account_keys WHERE account_id = ?1)",
            [account],
            |row| row.get(0),
        )?;
        if held {
            return Ok(KeySet::KeyExists);
        }
        let owned: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE credential_id = ?1 AND account_id = ?2)",
            params![credential, account],
            |row| row.get(0),
        )?;
        if !owned {
            return Ok(KeySet::UnknownPasskey);
        }
        tx.execute(
            "INSERT INTO account_keys (account_id, key_version, signing_key, encryption_key, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account, key.version, key.keys.signing, key.keys.encryption, now],
        )?;
        put_wrapper(&tx, account, credential, key.version, wrapped, now)?;
        tx.commit()?;
        Ok(KeySet::Set)
    }

    pub fn set_wrapper(
        &self,
        account: &str,
        credential: &[u8],
        version: u32,
        wrapped: &[u8],
        now: u64,
    ) -> Result<bool> {
        let owned: bool = self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE credential_id = ?1 AND account_id = ?2)",
            params![credential, account],
            |row| row.get(0),
        )?;
        if owned {
            put_wrapper(&self.connection, account, credential, version, wrapped, now)?;
        }
        Ok(owned)
    }

    pub fn wrapper(&self, credential: &[u8]) -> Result<Option<(u32, Vec<u8>)>> {
        self.connection
            .query_row(
                "SELECT key_version, wrapped FROM key_wrappers WHERE credential_id = ?1",
                [credential],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    pub fn remove_admission(&self, machine: &Id, device: &Id) -> Result<bool> {
        let tx = self.connection.unchecked_transaction()?;
        let removed = tx.execute(
            "DELETE FROM admissions WHERE machine_id = ?1 AND device_id = ?2",
            params![machine.as_str(), device.as_str()],
        )? > 0;
        drop_undelivered(&tx, device, Some(machine))?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn remove_machine(&self, machine: &Id) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM machines WHERE id = ?1", [machine.as_str()])?
            > 0)
    }

    pub fn machines(&self) -> Result<Vec<MachineRow>> {
        let mut statement = self.connection.prepare(
            "SELECT machines.id, machines.name, machines.joined_at, machines.last_seen, machines.account_id
             FROM machines
             WHERE NOT EXISTS (SELECT 1 FROM connects WHERE connects.machine_id = machines.id)
             ORDER BY machines.joined_at, machines.id",
        )?;
        statement
            .query_map([], |row| {
                Ok(MachineRow {
                    id: id_at(row, 0)?,
                    name: row.get(1)?,
                    joined_at: row.get(2)?,
                    last_seen: row.get(3)?,
                    owner: row.get(4)?,
                })
            })?
            .collect()
    }

    pub fn add_invite(&self, code_hash: &[u8], expires_at: u64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO invites (code_hash, expires_at) VALUES (?1, ?2)",
            params![code_hash, expires_at],
        )?;
        Ok(())
    }

    pub fn replace_setup_ticket(&self, code_hash: &[u8], expires_at: u64) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("DELETE FROM invites WHERE kind = 'owner'", [])?;
        tx.execute(
            "INSERT INTO invites (code_hash, expires_at, kind) VALUES (?1, ?2, 'owner')",
            params![code_hash, expires_at],
        )?;
        tx.commit()
    }

    pub fn ticket(&self, code_hash: &[u8], now: u64) -> Result<Option<Ticket>> {
        self.connection
            .query_row(
                "SELECT kind FROM invites WHERE code_hash = ?1 AND expires_at > ?2",
                params![code_hash, now],
                |row| Ticket::parse(&row.get::<_, String>(0)?).ok_or_else(|| corrupt(0)),
            )
            .optional()
    }

    pub fn account_count(&self) -> Result<u64> {
        self.connection
            .query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))
    }

    pub fn account_by_email(&self, email: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT id FROM accounts WHERE email = ?1 AND email_verified_at IS NOT NULL",
                [email],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn set_plan(&self, account: &str, status: PlanStatus, until: Option<u64>) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE accounts SET plan = ?2, plan_until = ?3 WHERE id = ?1",
            params![account, status.name(), until],
        )? > 0)
    }

    pub fn set_deactivated(&self, account: &str, at: Option<u64>) -> Result<bool> {
        let tx = self.connection.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE accounts SET deactivated_at = ?2 WHERE id = ?1 AND role != 'owner'",
            params![account, at],
        )? > 0;
        if changed && at.is_some() {
            tx.execute("DELETE FROM sessions WHERE account_id = ?1", [account])?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn make_owner(&self, account: &str, replace: bool, now: u64) -> Result<MadeOwner> {
        let tx = self.connection.unchecked_transaction()?;
        let Some((account, role, deactivated)) = tx
            .query_row(
                "SELECT id, role, deactivated_at IS NOT NULL FROM accounts WHERE id = ?1",
                [account],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get(2)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(MadeOwner::NoAccount);
        };
        if role == "owner" {
            return Ok(MadeOwner::AlreadyOwner);
        }
        if deactivated {
            return Ok(MadeOwner::Deactivated);
        }
        let others: Vec<String> = tx
            .prepare("SELECT id FROM accounts WHERE role = 'owner' ORDER BY id")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_>>()?;
        if !others.is_empty() && !replace {
            return Ok(MadeOwner::OtherOwner(others));
        }
        tx.execute(
            "UPDATE accounts SET role = 'member' WHERE role = 'owner'",
            [],
        )?;
        tx.execute(
            "UPDATE accounts SET role = 'owner' WHERE id = ?1",
            [&account],
        )?;
        tx.execute(
            "INSERT INTO events (at, kind, detail) VALUES (?1, 'account.owner', ?2)",
            params![now, format!("{account} by=cli")],
        )?;
        tx.commit()?;
        Ok(MadeOwner::Made { replaced: others })
    }

    pub fn machine_owner(&self, machine: &Id) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT account_id FROM machines WHERE id = ?1",
                [machine.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
    }

    pub fn device_account(&self, device: &Id) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT account_id FROM devices WHERE id = ?1",
                [device.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
    }

    pub fn create_connect(&self, new: &NewConnect<'_>) -> Result<()> {
        self.connection.execute(
            "INSERT INTO connects (device_code_hash, user_code_hash, name, signing_key, encryption_key,
                                   state, poll_interval, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?8)",
            params![
                new.device_code_hash,
                new.user_code_hash,
                new.name,
                new.keys.signing,
                new.keys.encryption,
                new.interval,
                new.now,
                new.expires_at
            ],
        )?;
        Ok(())
    }

    fn connects_where(&self, filter: &str, hash: &[u8]) -> Result<Option<Connect>> {
        connect_where(&self.connection, filter, hash)
    }

    pub fn connect_by_device_code(&self, hash: &[u8]) -> Result<Option<Connect>> {
        self.connects_where("device_code_hash", hash)
    }

    pub fn connect_by_user_code(&self, hash: &[u8]) -> Result<Option<Connect>> {
        self.connects_where("user_code_hash", hash)
    }

    pub fn connect_open_for_sign_up(&self, user_code_hash: &[u8], now: u64) -> Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM connects WHERE user_code_hash = ?1 AND state = 'pending'
                            AND expires_at > ?2 AND signup_at IS NULL)",
            params![user_code_hash, now],
            |row| row.get(0),
        )
    }

    pub fn polled_connect(&self, device_code_hash: &[u8], now: u64, interval: u64) -> Result<()> {
        self.connection.execute(
            "UPDATE connects SET polled_at = ?2, poll_interval = ?3 WHERE device_code_hash = ?1",
            params![device_code_hash, now, interval],
        )?;
        Ok(())
    }

    pub fn approve_connect(&mut self, approval: &ConnectApproval<'_>) -> Result<Option<Connect>> {
        let tx = self.connection.transaction()?;
        let changed = tx.execute(
            "UPDATE connects SET state = 'approved', account_id = ?3, machine_id = ?4, pairing_id = ?5
             WHERE user_code_hash = ?1 AND state = 'pending' AND expires_at > ?2",
            params![
                approval.user_code_hash,
                approval.now,
                approval.account,
                approval.machine.as_str(),
                approval.pairing.map(|pairing| pairing.id)
            ],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        let (name, keys) = tx.query_row(
            "SELECT name, signing_key, encryption_key FROM connects WHERE user_code_hash = ?1",
            [approval.user_code_hash],
            |row| Ok((row.get::<_, String>(0)?, keys_at(row, 1)?)),
        )?;
        tx.execute(
            "INSERT INTO machines (id, name, signing_key, encryption_key, token_hash, joined_at, account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                approval.machine.as_str(),
                name,
                keys.signing,
                keys.encryption,
                approval.placeholder_token_hash,
                approval.now,
                approval.account
            ],
        )?;
        if let Some(pairing) = approval.pairing {
            tx.execute(
                "INSERT INTO pairings (id, machine_id, code_hash, state, account_id, commitment, opened_at, expires_at)
                 VALUES (?1, ?2, ?3, 'committed', ?4, ?5, ?6, ?7)",
                params![
                    pairing.id,
                    approval.machine.as_str(),
                    pairing.code_hash,
                    approval.account,
                    pairing.commitment.as_slice(),
                    approval.now,
                    pairing.expires_at
                ],
            )?;
        }
        let connect = connect_where(&tx, "user_code_hash", approval.user_code_hash)?;
        tx.commit()?;
        Ok(connect)
    }

    pub fn deny_connect(&self, user_code_hash: &[u8], now: u64) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE connects SET state = 'denied'
             WHERE user_code_hash = ?1 AND state = 'pending' AND expires_at > ?2",
            params![user_code_hash, now],
        )? > 0)
    }

    pub fn end_connect(&self, device_code_hash: &[u8]) -> Result<()> {
        self.connection.execute(
            "DELETE FROM connects WHERE device_code_hash = ?1",
            [device_code_hash],
        )?;
        Ok(())
    }

    pub fn redeem_connect(
        &mut self,
        device_code_hash: &[u8],
        token_hash: &[u8],
        now: u64,
    ) -> Result<Option<(Id, Option<String>)>> {
        let tx = self.connection.transaction()?;
        let taken: Option<(String, Option<String>)> = tx
            .query_row(
                "DELETE FROM connects
                 WHERE device_code_hash = ?1 AND state = 'approved' AND expires_at > ?2
                 RETURNING machine_id, pairing_id",
                params![device_code_hash, now],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((machine, pairing)) = taken else {
            return Ok(None);
        };
        let machine = Id::parse(&machine).map_err(|_| corrupt(0))?;
        tx.execute(
            "UPDATE machines SET token_hash = ?2, last_seen = ?3 WHERE id = ?1",
            params![machine.as_str(), token_hash, now],
        )?;
        tx.commit()?;
        Ok(Some((machine, pairing)))
    }

    pub fn handle_taken(&self, handle: &str) -> Result<bool> {
        handle_held(&self.connection, handle)
    }

    pub fn passkey_account(&self, credential: &[u8]) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT account_id FROM passkeys WHERE credential_id = ?1",
                [credential],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn account_by_user_handle(&self, user_handle: &[u8]) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT id FROM accounts WHERE user_handle = ?1",
                [user_handle],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn account(&self, account: &str) -> Result<Option<AccountRow>> {
        self.connection
            .query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = ?1"),
                [account],
                account_row,
            )
            .optional()
    }

    pub fn save_ceremony(&self, ceremony: &Ceremony) -> Result<()> {
        self.connection.execute(
            "INSERT INTO ceremonies (id_hash, purpose, account_id, handle, user_handle, invite_hash, state, expires_at, prf_salt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                ceremony.id_hash,
                ceremony.purpose.name(),
                ceremony.account,
                ceremony.handle,
                ceremony.user_handle.map(|bytes| bytes.to_vec()),
                ceremony.invite_hash,
                ceremony.state,
                ceremony.expires_at,
                ceremony.prf_salt.map(|salt| salt.to_vec()),
            ],
        )?;
        Ok(())
    }

    pub fn take_ceremony(
        &self,
        id_hash: &[u8],
        purposes: &[Purpose],
        now: u64,
    ) -> Result<Option<Ceremony>> {
        let ceremony = self
            .connection
            .query_row(
                "DELETE FROM ceremonies WHERE id_hash = ?1
                 RETURNING id_hash, purpose, account_id, handle, user_handle, invite_hash, state, expires_at, prf_salt",
                [id_hash],
                |row| {
                    Ok(Ceremony {
                        id_hash: row.get(0)?,
                        purpose: Purpose::parse(&row.get::<_, String>(1)?)
                            .ok_or_else(|| corrupt(1))?,
                        account: row.get(2)?,
                        handle: row.get(3)?,
                        user_handle: row
                            .get::<_, Option<Vec<u8>>>(4)?
                            .map(|bytes| bytes.try_into().map_err(|_| corrupt(4)))
                            .transpose()?,
                        invite_hash: row.get(5)?,
                        state: row.get(6)?,
                        expires_at: row.get(7)?,
                        prf_salt: bytes32_at(row, 8)?,
                    })
                },
            )
            .optional()?;
        Ok(ceremony
            .filter(|ceremony| purposes.contains(&ceremony.purpose) && ceremony.expires_at > now))
    }

    pub fn sign_up(&mut self, signup: &SignUp<'_>) -> Result<SignUpOutcome> {
        let tx = self.connection.transaction()?;
        let Some(kind) = spend_ticket(&tx, signup.invite_hash, signup.connect, signup.now)? else {
            return Ok(SignUpOutcome::InviteSpent);
        };
        if handle_held(&tx, signup.handle)? {
            return Ok(SignUpOutcome::HandleTaken);
        }
        let reused: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE credential_id = ?1)",
            [signup.credential_id],
            |row| row.get(0),
        )?;
        if reused {
            return Ok(SignUpOutcome::CredentialTaken);
        }
        tx.execute(
            "INSERT INTO accounts (id, handle, user_handle, created_at, role, created_via)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                signup.account,
                signup.handle,
                signup.user_handle.to_vec(),
                signup.now,
                role_of(kind),
                kind.created_via()
            ],
        )?;
        tx.execute(
            "INSERT INTO passkeys (credential_id, account_id, name, passkey, created_at, prf_salt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                signup.credential_id,
                signup.account,
                signup.name,
                signup.passkey,
                signup.now,
                signup.prf_salt.as_slice()
            ],
        )?;
        tx.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![signup.session_hash, signup.account, signup.now, signup.session_expires_at],
        )?;
        tx.commit()?;
        Ok(SignUpOutcome::Created(kind))
    }

    pub fn sign_up_github(&mut self, signup: &GitHubSignUp<'_>) -> Result<SignUpOutcome> {
        let tx = self.connection.transaction()?;
        let Some(kind) = spend_ticket(&tx, signup.invite_hash, signup.connect, signup.now)? else {
            return Ok(SignUpOutcome::InviteSpent);
        };
        if handle_held(&tx, signup.handle)? {
            return Ok(SignUpOutcome::HandleTaken);
        }
        let linked: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE github_id = ?1)",
            [signup.github_id],
            |row| row.get(0),
        )?;
        if linked {
            return Ok(SignUpOutcome::GitHubTaken);
        }
        tx.execute(
            "INSERT INTO accounts (id, handle, user_handle, created_at, role, github_id, github_login, created_via)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                signup.account,
                signup.handle,
                signup.user_handle.to_vec(),
                signup.now,
                role_of(kind),
                signup.github_id,
                signup.github_login,
                kind.created_via()
            ],
        )?;
        tx.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![signup.session_hash, signup.account, signup.now, signup.session_expires_at],
        )?;
        tx.commit()?;
        Ok(SignUpOutcome::Created(kind))
    }

    pub fn account_by_github(&self, github_id: u64) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT id FROM accounts WHERE github_id = ?1",
                [github_id],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn set_github_login(&self, account: &str, login: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE accounts SET github_login = ?2 WHERE id = ?1",
            params![account, login],
        )?;
        Ok(())
    }

    pub fn link_github(&self, account: &str, github_id: u64, login: &str) -> Result<bool> {
        match self.connection.execute(
            "UPDATE accounts SET github_id = ?2, github_login = ?3 WHERE id = ?1",
            params![account, github_id, login],
        ) {
            Ok(changed) => Ok(changed > 0),
            Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    pub fn unlink_github(&mut self, account: &str) -> Result<GitHubUnlink> {
        let tx = self.connection.transaction()?;
        let linked: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE id = ?1 AND github_id IS NOT NULL)",
            [account],
            |row| row.get(0),
        )?;
        if !linked {
            return Ok(GitHubUnlink::NotLinked);
        }
        let passkeys: u64 = tx.query_row(
            "SELECT count(*) FROM passkeys WHERE account_id = ?1",
            [account],
            |row| row.get(0),
        )?;
        if passkeys == 0 {
            return Ok(GitHubUnlink::LastSignIn);
        }
        tx.execute(
            "UPDATE accounts SET github_id = NULL, github_login = NULL WHERE id = ?1",
            [account],
        )?;
        tx.commit()?;
        Ok(GitHubUnlink::Removed)
    }

    pub fn passkeys(&self, account: &str) -> Result<Vec<StoredPasskey>> {
        let mut statement = self.connection.prepare(
            "SELECT credential_id, name, passkey, created_at, last_used_at, counter, prf_salt,
                    EXISTS (SELECT 1 FROM key_wrappers WHERE key_wrappers.credential_id = passkeys.credential_id)
             FROM passkeys WHERE account_id = ?1 ORDER BY created_at, credential_id",
        )?;
        statement
            .query_map([account], |row| {
                Ok(StoredPasskey {
                    credential_id: row.get(0)?,
                    name: row.get(1)?,
                    passkey: row.get(2)?,
                    created_at: row.get(3)?,
                    last_used_at: row.get(4)?,
                    counter: row.get(5)?,
                    prf_salt: bytes32_at(row, 6)?.ok_or_else(|| corrupt(6))?,
                    unlocks: row.get(7)?,
                })
            })?
            .collect()
    }

    pub fn add_passkey(
        &self,
        account: &str,
        credential_id: &[u8],
        name: Option<&str>,
        passkey: &str,
        prf_salt: &[u8; 32],
        now: u64,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO passkeys (credential_id, account_id, name, passkey, created_at, prf_salt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (credential_id) DO NOTHING",
            params![credential_id, account, name, passkey, now, prf_salt.as_slice()],
        )? > 0)
    }

    pub fn use_passkey(
        &self,
        credential_id: &[u8],
        passkey: Option<&str>,
        counter: u32,
        now: u64,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE passkeys SET last_used_at = ?2, passkey = coalesce(?3, passkey), counter = ?4
             WHERE credential_id = ?1",
            params![credential_id, now, passkey, counter],
        )?;
        Ok(())
    }

    pub fn remove_passkey(&mut self, account: &str, credential_id: &[u8]) -> Result<Removal> {
        let tx = self.connection.transaction()?;
        let count: u64 = tx.query_row(
            "SELECT count(*) FROM passkeys WHERE account_id = ?1",
            [account],
            |row| row.get(0),
        )?;
        let owned: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE account_id = ?1 AND credential_id = ?2)",
            params![account, credential_id],
            |row| row.get(0),
        )?;
        if !owned {
            return Ok(Removal::Missing);
        }
        if count <= 1 {
            return Ok(Removal::Last);
        }
        let (wrappers, this_wraps): (u64, bool) = tx.query_row(
            "SELECT count(*), coalesce(sum(credential_id = ?2), 0) > 0 FROM key_wrappers WHERE account_id = ?1",
            params![account, credential_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if this_wraps && wrappers <= 1 {
            return Ok(Removal::LastUnlocking);
        }
        tx.execute(
            "DELETE FROM passkeys WHERE account_id = ?1 AND credential_id = ?2",
            params![account, credential_id],
        )?;
        tx.commit()?;
        Ok(Removal::Removed)
    }

    pub fn create_session(
        &self,
        token_hash: &[u8],
        account: &str,
        now: u64,
        expires_at: u64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![token_hash, account, now, expires_at],
        )?;
        Ok(())
    }

    pub fn session_account(&self, token_hash: &[u8], now: u64) -> Result<Option<String>> {
        Ok(self.session(token_hash, now)?.map(|(account, _)| account))
    }

    pub fn session(&self, token_hash: &[u8], now: u64) -> Result<Option<(String, u64)>> {
        self.connection
            .query_row(
                "SELECT sessions.account_id, sessions.created_at FROM sessions
                 JOIN accounts ON accounts.id = sessions.account_id
                 WHERE sessions.token_hash = ?1 AND sessions.expires_at > ?2
                     AND accounts.deactivated_at IS NULL",
                params![token_hash, now],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    pub fn refresh_session(&self, token_hash: &[u8], now: u64) -> Result<()> {
        self.connection.execute(
            "UPDATE sessions SET created_at = ?2 WHERE token_hash = ?1 AND expires_at > ?2",
            params![token_hash, now],
        )?;
        Ok(())
    }

    pub fn end_session(&self, token_hash: &[u8]) -> Result<()> {
        self.connection
            .execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    }

    pub fn devices_of_account(&self, account: &str) -> Result<Vec<DeviceRow>> {
        let mut statement = self.connection.prepare(
            "SELECT devices.id, devices.name, devices.created_at, devices.last_seen,
                    group_concat(admissions.machine_id, ' ')
             FROM devices LEFT JOIN admissions ON admissions.device_id = devices.id
             WHERE devices.account_id = ?1
             GROUP BY devices.id ORDER BY devices.created_at, devices.id",
        )?;
        statement
            .query_map([account], |row| {
                let machines = row
                    .get::<_, Option<String>>(4)?
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(|text| Id::parse(text).map_err(|_| corrupt(4)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(DeviceRow {
                    id: id_at(row, 0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    last_seen: row.get(3)?,
                    machines,
                })
            })?
            .collect()
    }

    pub fn remove_account_device(&self, account: &str, device: &Id) -> Result<bool> {
        let tx = self.connection.unchecked_transaction()?;
        let owned: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND account_id = ?2)",
            params![device.as_str(), account],
            |row| row.get(0),
        )?;
        if !owned {
            return Ok(false);
        }
        drop_undelivered(&tx, device, None)?;
        tx.execute("DELETE FROM devices WHERE id = ?1", [device.as_str()])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn subscribe_push(
        &self,
        device: &Id,
        endpoint: &str,
        p256dh: &[u8],
        auth: &[u8],
        now: u64,
    ) -> Result<Subscribed> {
        let tx = self.connection.unchecked_transaction()?;
        let holder: Option<(Id, bool)> = tx
            .query_row(
                "SELECT holder.id, holder.account_id IS NOT NULL
                     AND holder.account_id = (SELECT account_id FROM devices WHERE id = ?2)
                 FROM push_subscriptions JOIN devices AS holder ON holder.id = push_subscriptions.device_id
                 WHERE push_subscriptions.endpoint = ?1 AND push_subscriptions.device_id != ?2",
                params![endpoint, device.as_str()],
                |row| Ok((id_at(row, 0)?, row.get::<_, Option<bool>>(1)?.unwrap_or(false))),
            )
            .optional()?;
        let outcome = match holder {
            Some((_, false)) => return Ok(Subscribed::Taken),
            Some((from, true)) => {
                tx.execute(
                    "DELETE FROM push_subscriptions WHERE device_id = ?1",
                    [from.as_str()],
                )?;
                Subscribed::Moved(from)
            }
            None => Subscribed::New,
        };
        tx.execute(
            "INSERT INTO push_subscriptions (device_id, endpoint, p256dh, auth, subscribed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (device_id) DO UPDATE SET
                 endpoint = ?2, p256dh = ?3, auth = ?4, subscribed_at = ?5",
            params![device.as_str(), endpoint, p256dh, auth, now],
        )?;
        tx.commit()?;
        Ok(outcome)
    }

    pub fn unsubscribe_push(&self, device: &Id) -> Result<bool> {
        Ok(self.connection.execute(
            "DELETE FROM push_subscriptions WHERE device_id = ?1",
            [device.as_str()],
        )? > 0)
    }

    pub fn drop_push_endpoint(&self, device: &Id, endpoint: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "DELETE FROM push_subscriptions WHERE device_id = ?1 AND endpoint = ?2",
            params![device.as_str(), endpoint],
        )? > 0)
    }

    pub fn push_subscription(&self, device: &Id) -> Result<Option<StoredSubscription>> {
        self.connection
            .query_row(
                "SELECT endpoint, p256dh, auth FROM push_subscriptions WHERE device_id = ?1",
                [device.as_str()],
                |row| {
                    Ok(StoredSubscription {
                        endpoint: row.get(0)?,
                        p256dh: row.get(1)?,
                        auth: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn record_notice(
        &self,
        machine: &Id,
        notice: &Id,
        now: u64,
        expires_at: u64,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO notices (machine_id, notice_id, posted_at, expires_at)
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
            params![machine.as_str(), notice.as_str(), now, expires_at],
        )? > 0)
    }

    pub fn spend_challenge(&self, nonce: &[u8; 32], expires_at: u64) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO report_challenges (nonce, expires_at) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            params![nonce.as_slice(), expires_at],
        )? > 0)
    }

    pub fn store_report(&mut self, new: &NewReport<'_>) -> Result<ReportWrite> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO report_keys (key, status, first_seen, last_seen) VALUES (?1, 'quarantine', ?2, ?2)
             ON CONFLICT (key) DO UPDATE SET last_seen = ?2",
            params![new.key.as_slice(), new.now],
        )?;
        let status: String = tx.query_row(
            "SELECT status FROM report_keys WHERE key = ?1",
            [new.key.as_slice()],
            |row| row.get(0),
        )?;
        let status = KeyStatus::parse(&status).ok_or_else(|| corrupt(0))?;
        let waiting: u64 = tx.query_row(
            "SELECT count(*) FROM reports JOIN report_keys ON report_keys.key = reports.key
             WHERE report_keys.status = 'quarantine'",
            [],
            |row| row.get(0),
        )?;
        let from_key: u64 = tx.query_row(
            "SELECT count(*) FROM reports WHERE key = ?1",
            [new.key.as_slice()],
            |row| row.get(0),
        )?;
        let full = status == KeyStatus::Quarantine
            && (waiting >= new.max_quarantined || from_key >= new.max_quarantined_per_key);
        let report = new.report;
        let id: u64 = tx.query_row(
            "INSERT INTO reports (key, title, text, version, os, received_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
            params![new.key.as_slice(), report.title, report.text, report.version, report.os, new.now],
            |row| row.get(0),
        )?;
        let dropped = status == KeyStatus::Blocked || full;
        if dropped {
            tx.execute("DELETE FROM reports WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(if dropped {
            ReportWrite::Dropped(id, status)
        } else {
            ReportWrite::Stored(id, status)
        })
    }

    pub fn reports(
        &self,
        status: KeyStatus,
        after: u64,
        unacked: bool,
        limit: u64,
    ) -> Result<Vec<StoredReport>> {
        let mut statement = self.connection.prepare(
            "SELECT reports.id, reports.key, report_keys.status, reports.received_at, reports.acked_at,
                    reports.title, reports.text, reports.version, reports.os
             FROM reports JOIN report_keys ON report_keys.key = reports.key
             WHERE report_keys.status = ?1 AND reports.id > ?2 AND (?3 = 0 OR reports.acked_at IS NULL)
             ORDER BY reports.id LIMIT ?4",
        )?;
        statement
            .query_map(params![status.name(), after, unacked, limit], |row| {
                Ok(StoredReport {
                    id: row.get(0)?,
                    key: row
                        .get::<_, Vec<u8>>(1)?
                        .try_into()
                        .map_err(|_| corrupt(1))?,
                    status: KeyStatus::parse(&row.get::<_, String>(2)?)
                        .ok_or_else(|| corrupt(2))?,
                    received_at: row.get(3)?,
                    acked_at: row.get(4)?,
                    report: BugReport {
                        title: row.get(5)?,
                        text: row.get(6)?,
                        version: row.get(7)?,
                        os: row.get(8)?,
                    },
                })
            })?
            .collect()
    }

    pub fn quarantined_count(&self) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM reports JOIN report_keys ON report_keys.key = reports.key
             WHERE report_keys.status = 'quarantine'",
            [],
            |row| row.get(0),
        )
    }

    pub fn ack_report(&self, id: u64, now: u64) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE reports SET acked_at = coalesce(acked_at, ?2)
             WHERE id = ?1 AND key IN (SELECT key FROM report_keys WHERE status = 'allowed')",
            params![id, now],
        )? > 0)
    }

    pub fn set_key_status(&self, key: &[u8; 32], status: KeyStatus, now: u64) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO report_keys (key, status, first_seen, last_seen) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT (key) DO UPDATE SET status = ?2",
            params![key.as_slice(), status.name(), now],
        )?;
        if status == KeyStatus::Blocked {
            tx.execute("DELETE FROM reports WHERE key = ?1", [key.as_slice()])?;
        }
        tx.commit()
    }

    pub fn store_signals(&mut self, new: &NewSignals<'_>) -> Result<SignalWrite> {
        let tx = self.connection.transaction()?;
        let status = tx
            .query_row(
                "SELECT status FROM report_keys WHERE key = ?1",
                [new.key.as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|status| KeyStatus::parse(&status).ok_or_else(|| corrupt(0)))
            .transpose()?
            .unwrap_or(KeyStatus::Quarantine);
        if status == KeyStatus::Blocked {
            return Ok(SignalWrite::Dropped("dropped-blocked"));
        }
        let batches: u64 = tx
            .query_row(
                "SELECT batches FROM signal_senders WHERE key = ?1 AND day = ?2",
                params![new.key.as_slice(), new.day],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if batches >= new.max_batches_per_key {
            return Ok(SignalWrite::Dropped("dropped-repeat"));
        }
        let vetted = status == KeyStatus::Allowed;
        if !vetted {
            let unvetted: u64 = tx.query_row(
                "SELECT count(*) FROM signal_counts WHERE day = ?1 AND vetted = 0",
                [new.day],
                |row| row.get(0),
            )?;
            if unvetted + new.signals.counts.len() as u64 > new.max_unvetted_rows {
                return Ok(SignalWrite::Dropped("dropped-unvetted-full"));
            }
        }
        tx.execute(
            "INSERT INTO signal_senders (key, day, batches) VALUES (?1, ?2, 1)
             ON CONFLICT (key, day) DO UPDATE SET batches = batches + 1",
            params![new.key.as_slice(), new.day],
        )?;
        let signals = new.signals;
        let commit = signals.commit.as_ref().map_or("", |commit| commit.as_str());
        for row in &signals.counts {
            tx.execute(
                "INSERT INTO signal_counts (day, version, commit_id, os, arch, component, kind, vetted, count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (day, version, commit_id, os, arch, component, kind, vetted) DO UPDATE SET count = count + ?9",
                params![new.day, signals.version.as_str(), commit, signals.os.as_str(), signals.arch.as_str(), row.component.as_str(), row.kind.key(), vetted, row.count],
            )?;
        }
        tx.commit()?;
        Ok(SignalWrite::Counted { vetted })
    }

    pub fn signal_rows(&self, since_day: u64) -> Result<Vec<SignalRow>> {
        let mut statement = self.connection.prepare(
            "SELECT day, version, commit_id, os, arch, component, kind, vetted, count FROM signal_counts
             WHERE day >= ?1 ORDER BY day DESC, vetted DESC, count DESC, component, kind",
        )?;
        statement
            .query_map([since_day], |row| {
                Ok(SignalRow {
                    day: row.get(0)?,
                    version: row.get(1)?,
                    commit: row.get(2)?,
                    os: row.get(3)?,
                    arch: row.get(4)?,
                    component: row.get(5)?,
                    kind: row.get(6)?,
                    vetted: row.get(7)?,
                    count: row.get(8)?,
                })
            })?
            .collect()
    }

    pub fn add_admin_token(
        &self,
        token_hash: &[u8],
        name: &str,
        scope: AdminScope,
        now: u64,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO admin_tokens (token_hash, name, created_at, scope) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (name) DO NOTHING",
            params![token_hash, name, now, scope.name()],
        )? > 0)
    }

    pub fn admin_token_scope(&self, token_hash: &[u8], now: u64) -> Result<Option<AdminScope>> {
        self.connection
            .query_row(
                "UPDATE admin_tokens SET last_used = ?2 WHERE token_hash = ?1 RETURNING scope",
                params![token_hash, now],
                |row| AdminScope::parse(&row.get::<_, String>(0)?).ok_or_else(|| corrupt(0)),
            )
            .optional()
    }

    pub fn admin_tokens(&self) -> Result<Vec<AdminToken>> {
        let mut statement = self.connection.prepare(
            "SELECT name, scope, created_at, last_used FROM admin_tokens ORDER BY created_at, name",
        )?;
        statement
            .query_map([], |row| {
                Ok(AdminToken {
                    name: row.get(0)?,
                    scope: AdminScope::parse(&row.get::<_, String>(1)?)
                        .ok_or_else(|| corrupt(1))?,
                    created_at: row.get(2)?,
                    last_used: row.get(3)?,
                })
            })?
            .collect()
    }

    pub fn counts(&self) -> Result<(u64, u64, u64)> {
        self.connection.query_row(
            "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM machines
                 WHERE NOT EXISTS (SELECT 1 FROM connects WHERE connects.machine_id = machines.id)),
                    (SELECT count(*) FROM devices)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
    }

    pub fn allowed_unacked_count(&self) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM reports JOIN report_keys ON report_keys.key = reports.key
             WHERE report_keys.status = 'allowed' AND reports.acked_at IS NULL",
            [],
            |row| row.get(0),
        )
    }

    pub fn admin_accounts(&self) -> Result<Vec<AdminAccountRow>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS},
                    (SELECT count(*) FROM machines WHERE machines.account_id = accounts.id),
                    (SELECT count(*) FROM devices WHERE devices.account_id = accounts.id),
                    {LAST_SEEN}
             FROM accounts ORDER BY created_at, id"
        ))?;
        statement
            .query_map([], |row| {
                Ok(AdminAccountRow {
                    account: account_row(row)?,
                    machines: row.get(ACCOUNT_COLUMN_COUNT)?,
                    devices: row.get(ACCOUNT_COLUMN_COUNT + 1)?,
                    last_seen: seen_at(row.get(ACCOUNT_COLUMN_COUNT + 2)?),
                })
            })?
            .collect()
    }

    pub fn account_page(
        &self,
        after: Option<&str>,
        limit: u64,
        active_since: u64,
    ) -> Result<Option<Vec<AccountPageRow>>> {
        if let Some(after) = after
            && self.account(after)?.is_none()
        {
            return Ok(None);
        }
        let mut statement = self.connection.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS},
                    (SELECT count(*) FROM machines WHERE machines.account_id = accounts.id),
                    (SELECT count(*) FROM machines WHERE machines.account_id = accounts.id AND machines.last_seen > ?3),
                    {LAST_SEEN}
             FROM accounts
             WHERE ?1 IS NULL OR (accounts.created_at, accounts.id) > (SELECT created_at, id FROM accounts WHERE id = ?1)
             ORDER BY accounts.created_at, accounts.id
             LIMIT ?2"
        ))?;
        statement
            .query_map(params![after, limit, active_since], |row| {
                Ok(AccountPageRow {
                    account: account_row(row)?,
                    machines: row.get(ACCOUNT_COLUMN_COUNT)?,
                    active_machines: row.get(ACCOUNT_COLUMN_COUNT + 1)?,
                    last_seen: seen_at(row.get(ACCOUNT_COLUMN_COUNT + 2)?),
                })
            })?
            .collect::<Result<_>>()
            .map(Some)
    }

    pub fn account_detail(&self, account: &str) -> Result<Option<AccountDetailRow>> {
        let found = self
            .connection
            .query_row(
                &format!(
                    "SELECT {ACCOUNT_COLUMNS},
                            (SELECT count(*) FROM passkeys WHERE passkeys.account_id = accounts.id),
                            {LAST_SEEN}
                     FROM accounts WHERE accounts.id = ?1"
                ),
                [account],
                |row| {
                    Ok((
                        account_row(row)?,
                        row.get(ACCOUNT_COLUMN_COUNT)?,
                        seen_at(row.get(ACCOUNT_COLUMN_COUNT + 1)?),
                    ))
                },
            )
            .optional()?;
        let Some((account, passkeys, last_seen)) = found else {
            return Ok(None);
        };
        let machines = self
            .connection
            .prepare(
                "SELECT machines.id, machines.name, machines.joined_at, machines.last_seen, boards.stored_at
                 FROM machines LEFT JOIN boards ON boards.machine_id = machines.id
                 WHERE machines.account_id = ?1
                 ORDER BY machines.joined_at, machines.id",
            )?
            .query_map([&account.id], |row| {
                Ok(AccountMachineRow {
                    id: id_at(row, 0)?,
                    name: row.get(1)?,
                    joined_at: row.get(2)?,
                    last_seen: row.get(3)?,
                    last_board_at: row.get(4)?,
                })
            })?
            .collect::<Result<_>>()?;
        Ok(Some(AccountDetailRow {
            account,
            passkeys,
            last_seen,
            machines,
        }))
    }

    pub fn admitted_count(&self, machine: &Id) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM admissions WHERE machine_id = ?1",
            [machine.as_str()],
            |row| row.get(0),
        )
    }

    pub fn revoke_admin_token(&self, name: &str) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM admin_tokens WHERE name = ?1", [name])?
            > 0)
    }

    pub fn sweep(&self, now: u64) -> Result<Sweep> {
        self.connection.execute(
            "DELETE FROM machines WHERE id IN (
                 SELECT machine_id FROM connects WHERE expires_at <= ?1 AND machine_id IS NOT NULL)",
            [now],
        )?;
        self.connection
            .execute("DELETE FROM connects WHERE expires_at <= ?1", [now])?;
        self.connection
            .execute("DELETE FROM join_codes WHERE expires_at <= ?1", [now])?;
        self.connection
            .execute("DELETE FROM invites WHERE expires_at <= ?1", [now])?;
        self.connection
            .execute("DELETE FROM ceremonies WHERE expires_at <= ?1", [now])?;
        let sessions = self
            .connection
            .execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])?;
        let devices = self.connection.execute(
            "DELETE FROM devices WHERE account_id IS NULL
                AND NOT EXISTS (SELECT 1 FROM admissions WHERE admissions.device_id = devices.id)
                AND NOT EXISTS (SELECT 1 FROM pairings WHERE pairings.device_id = devices.id
                    AND pairings.state IN ('open', 'committed', 'replied', 'revealed')
                    AND pairings.expires_at > ?1)",
            [now],
        )?;
        self.connection.execute(
            "DELETE FROM pairings WHERE expires_at + ?2 <= ?1",
            params![now, PAIRING_KEEP_SECONDS],
        )?;
        self.connection
            .execute("DELETE FROM queue WHERE expires_at <= ?1", [now])?;
        self.connection.execute(
            "DELETE FROM commands WHERE queued_at + ?2 <= ?1",
            params![now, COMMAND_KEEP_SECONDS],
        )?;
        self.connection
            .execute("DELETE FROM results WHERE expires_at <= ?1", [now])?;
        self.connection
            .execute("DELETE FROM notices WHERE expires_at <= ?1", [now])?;
        self.connection.execute(
            "DELETE FROM events WHERE at + ?2 <= ?1",
            params![now, EVENT_KEEP_SECONDS],
        )?;
        self.connection.execute(
            "DELETE FROM report_challenges WHERE expires_at <= ?1",
            [now],
        )?;
        self.connection.execute(
            "DELETE FROM reports WHERE received_at + ?2 <= ?1
                AND (acked_at IS NOT NULL OR key IN (SELECT key FROM report_keys WHERE status = 'quarantine'))",
            params![now, REPORT_KEEP_SECONDS],
        )?;
        self.connection.execute(
            "DELETE FROM report_keys WHERE status = 'quarantine'
                AND NOT EXISTS (SELECT 1 FROM reports WHERE reports.key = report_keys.key)",
            [],
        )?;
        let today = now / 86_400;
        self.connection
            .execute("DELETE FROM signal_senders WHERE day < ?1", [today])?;
        self.connection.execute(
            "DELETE FROM signal_counts WHERE day + ?2 <= ?1",
            params![today, SIGNAL_KEEP_DAYS],
        )?;
        Ok(Sweep { sessions, devices })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    Quarantine,
    Allowed,
    Blocked,
}

impl KeyStatus {
    pub fn name(self) -> &'static str {
        match self {
            KeyStatus::Quarantine => "quarantine",
            KeyStatus::Allowed => "allowed",
            KeyStatus::Blocked => "blocked",
        }
    }

    fn parse(name: &str) -> Option<KeyStatus> {
        [
            KeyStatus::Quarantine,
            KeyStatus::Allowed,
            KeyStatus::Blocked,
        ]
        .into_iter()
        .find(|status| status.name() == name)
    }
}

pub struct NewReport<'a> {
    pub key: &'a [u8; 32],
    pub report: &'a BugReport,
    pub now: u64,
    pub max_quarantined: u64,
    pub max_quarantined_per_key: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReportWrite {
    Stored(u64, KeyStatus),
    Dropped(u64, KeyStatus),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredReport {
    pub id: u64,
    pub key: [u8; 32],
    pub status: KeyStatus,
    pub received_at: u64,
    pub acked_at: Option<u64>,
    pub report: BugReport,
}

pub struct NewSignals<'a> {
    pub key: &'a [u8; 32],
    pub signals: &'a ErrorSignals,
    pub day: u64,
    pub max_batches_per_key: u64,
    pub max_unvetted_rows: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SignalWrite {
    Counted { vetted: bool },
    Dropped(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalRow {
    pub day: u64,
    pub version: String,
    pub commit: String,
    pub os: String,
    pub arch: String,
    pub component: String,
    pub kind: String,
    pub vetted: bool,
    pub count: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    pub sessions: usize,
    pub devices: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardHead {
    pub machine: Id,
    pub board: Option<(String, u64)>,
}

pub struct MachineRow {
    pub id: Id,
    pub name: String,
    pub joined_at: u64,
    pub last_seen: Option<u64>,
    pub owner: Option<String>,
}

pub struct AccountRow {
    pub id: String,
    pub handle: String,
    pub user_handle: [u8; 16],
    pub created_at: u64,
    pub owner: bool,
    pub email: Option<String>,
    pub plan: PlanStatus,
    pub plan_until: Option<u64>,
    pub deactivated_at: Option<u64>,
    pub github_id: Option<u64>,
    pub github_login: Option<String>,
    pub created_via: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MadeOwner {
    Made { replaced: Vec<String> },
    AlreadyOwner,
    OtherOwner(Vec<String>),
    Deactivated,
    NoAccount,
}

pub struct AccountPageRow {
    pub account: AccountRow,
    pub machines: u64,
    pub active_machines: u64,
    pub last_seen: Option<u64>,
}

pub struct AccountDetailRow {
    pub account: AccountRow,
    pub passkeys: u64,
    pub last_seen: Option<u64>,
    pub machines: Vec<AccountMachineRow>,
}

pub struct AccountMachineRow {
    pub id: Id,
    pub name: String,
    pub joined_at: u64,
    pub last_seen: Option<u64>,
    pub last_board_at: Option<u64>,
}

pub struct AdminAccountRow {
    pub account: AccountRow,
    pub machines: u64,
    pub devices: u64,
    pub last_seen: Option<u64>,
}

const ACCOUNT_COLUMNS: &str = "accounts.id, accounts.handle, accounts.user_handle, accounts.created_at, accounts.role, accounts.email, accounts.plan, accounts.plan_until, accounts.deactivated_at, accounts.github_id, accounts.github_login, accounts.created_via";
const ACCOUNT_COLUMN_COUNT: usize = 12;
const LAST_SEEN: &str = "max(coalesce((SELECT max(last_seen) FROM machines WHERE machines.account_id = accounts.id), 0),
        coalesce((SELECT max(last_seen) FROM devices WHERE devices.account_id = accounts.id), 0))";

fn seen_at(at: u64) -> Option<u64> {
    (at > 0).then_some(at)
}

fn account_row(row: &Row<'_>) -> Result<AccountRow> {
    Ok(AccountRow {
        id: row.get(0)?,
        handle: row.get(1)?,
        user_handle: row
            .get::<_, Vec<u8>>(2)?
            .try_into()
            .map_err(|_| corrupt(2))?,
        created_at: row.get(3)?,
        owner: row.get::<_, String>(4)? == "owner",
        email: row.get(5)?,
        plan: PlanStatus::parse(&row.get::<_, String>(6)?).ok_or_else(|| corrupt(6))?,
        plan_until: row.get(7)?,
        deactivated_at: row.get(8)?,
        github_id: row.get(9)?,
        github_login: row.get(10)?,
        created_via: row.get(11)?,
    })
}

fn role_of(kind: Ticket) -> &'static str {
    if kind == Ticket::Owner {
        "owner"
    } else {
        "member"
    }
}

fn handle_held(connection: &Connection, handle: &str) -> Result<bool> {
    connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM accounts WHERE handle = ?1)",
        [handle],
        |row| row.get(0),
    )
}

fn spend_ticket(connection: &Connection, hash: &[u8], connect: bool, now: u64) -> Result<Option<Ticket>> {
    let kind = if connect {
        let claimed = connection.execute(
            "UPDATE connects SET signup_at = ?2
             WHERE user_code_hash = ?1 AND state = 'pending' AND expires_at > ?2 AND signup_at IS NULL",
            params![hash, now],
        )?;
        if claimed == 0 {
            return Ok(None);
        }
        Ticket::Connect
    } else {
        let spent: Option<String> = connection
            .query_row(
                "DELETE FROM invites WHERE code_hash = ?1 AND expires_at > ?2 RETURNING kind",
                params![hash, now],
                |row| row.get(0),
            )
            .optional()?;
        let Some(kind) = spent else {
            return Ok(None);
        };
        Ticket::parse(&kind).ok_or_else(|| corrupt(0))?
    };
    if kind == Ticket::Owner {
        let accounts: u64 =
            connection.query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))?;
        if accounts > 0 {
            return Ok(None);
        }
    }
    Ok(Some(kind))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ticket {
    Invite,
    Owner,
    Connect,
}

impl Ticket {
    pub fn name(self) -> &'static str {
        match self {
            Ticket::Invite => "invite",
            Ticket::Owner => "owner",
            Ticket::Connect => "connect",
        }
    }

    pub fn created_via(self) -> &'static str {
        match self {
            Ticket::Invite => "invite",
            Ticket::Owner => "setup",
            Ticket::Connect => "connect",
        }
    }

    fn parse(name: &str) -> Option<Ticket> {
        [Ticket::Invite, Ticket::Owner]
            .into_iter()
            .find(|ticket| ticket.name() == name)
    }
}

pub struct AdminToken {
    pub name: String,
    pub scope: AdminScope,
    pub created_at: u64,
    pub last_used: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminScope {
    Reports,
    Billing,
}

impl AdminScope {
    pub fn name(self) -> &'static str {
        match self {
            AdminScope::Reports => "reports",
            AdminScope::Billing => "billing",
        }
    }

    pub fn parse(name: &str) -> Option<AdminScope> {
        [AdminScope::Reports, AdminScope::Billing]
            .into_iter()
            .find(|scope| scope.name() == name)
    }
}

#[derive(Clone, Debug)]
pub struct Connect {
    pub device_code_hash: Vec<u8>,
    pub name: String,
    pub keys: PublicKeys,
    pub state: ConnectState,
    pub account: Option<String>,
    pub machine: Option<Id>,
    pub pairing: Option<String>,
    pub interval: u64,
    pub polled_at: Option<u64>,
    pub expires_at: u64,
}

pub struct NewConnect<'a> {
    pub device_code_hash: &'a [u8],
    pub user_code_hash: &'a [u8],
    pub name: &'a str,
    pub keys: &'a PublicKeys,
    pub interval: u64,
    pub now: u64,
    pub expires_at: u64,
}

pub struct ConnectPairing<'a> {
    pub id: &'a str,
    pub code_hash: &'a [u8],
    pub commitment: [u8; 32],
    pub expires_at: u64,
}

pub struct ConnectApproval<'a> {
    pub user_code_hash: &'a [u8],
    pub account: &'a str,
    pub machine: &'a Id,
    pub placeholder_token_hash: &'a [u8],
    pub pairing: Option<&'a ConnectPairing<'a>>,
    pub now: u64,
}

pub struct DeviceRow {
    pub id: Id,
    pub name: Option<String>,
    pub created_at: u64,
    pub last_seen: Option<u64>,
    pub machines: Vec<Id>,
}

pub struct StoredPasskey {
    pub credential_id: Vec<u8>,
    pub name: Option<String>,
    pub passkey: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub counter: u32,
    pub prf_salt: [u8; 32],
    pub unlocks: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    SignUp,
    ConnectSignUp,
    AddPasskey,
    LogIn,
    GitHub,
    GitHubConnect,
    GitHubLink,
}

impl Purpose {
    pub fn name(self) -> &'static str {
        match self {
            Purpose::SignUp => "sign-up",
            Purpose::ConnectSignUp => "connect-sign-up",
            Purpose::AddPasskey => "add-passkey",
            Purpose::LogIn => "log-in",
            Purpose::GitHub => "github",
            Purpose::GitHubConnect => "github-connect",
            Purpose::GitHubLink => "github-link",
        }
    }

    fn parse(name: &str) -> Option<Purpose> {
        [
            Purpose::SignUp,
            Purpose::ConnectSignUp,
            Purpose::AddPasskey,
            Purpose::LogIn,
            Purpose::GitHub,
            Purpose::GitHubConnect,
            Purpose::GitHubLink,
        ]
            .into_iter()
            .find(|purpose| purpose.name() == name)
    }
}

pub struct Ceremony {
    pub id_hash: Vec<u8>,
    pub purpose: Purpose,
    pub account: Option<String>,
    pub handle: Option<String>,
    pub user_handle: Option<[u8; 16]>,
    pub invite_hash: Option<Vec<u8>>,
    pub state: String,
    pub expires_at: u64,
    pub prf_salt: Option<[u8; 32]>,
}

pub struct SignUp<'a> {
    pub invite_hash: &'a [u8],
    pub connect: bool,
    pub account: &'a str,
    pub handle: &'a str,
    pub user_handle: [u8; 16],
    pub credential_id: &'a [u8],
    pub name: Option<&'a str>,
    pub passkey: &'a str,
    pub prf_salt: [u8; 32],
    pub session_hash: &'a [u8],
    pub session_expires_at: u64,
    pub now: u64,
}

pub enum SignUpOutcome {
    Created(Ticket),
    InviteSpent,
    HandleTaken,
    CredentialTaken,
    GitHubTaken,
}

pub struct GitHubSignUp<'a> {
    pub invite_hash: &'a [u8],
    pub connect: bool,
    pub account: &'a str,
    pub handle: &'a str,
    pub user_handle: [u8; 16],
    pub github_id: u64,
    pub github_login: &'a str,
    pub session_hash: &'a [u8],
    pub session_expires_at: u64,
    pub now: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GitHubUnlink {
    Removed,
    NotLinked,
    LastSignIn,
}

pub enum Removal {
    Removed,
    Last,
    LastUnlocking,
    Missing,
}

#[derive(Debug, PartialEq, Eq)]
pub enum KeySet {
    Set,
    KeyExists,
    UnknownPasskey,
}

pub struct AccountBoard {
    pub machine: Id,
    pub name: String,
    pub last_seen: Option<u64>,
    pub keys: PublicKeys,
    pub board: Option<(u64, u64, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ListWrite {
    Stored,
    Stale,
    UnknownMachine,
}

fn put_wrapper(
    connection: &Connection,
    account: &str,
    credential: &[u8],
    version: u32,
    wrapped: &[u8],
    now: u64,
) -> Result<()> {
    connection.execute(
        "INSERT INTO key_wrappers (credential_id, account_id, key_version, wrapped, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (credential_id) DO UPDATE SET key_version = ?3, wrapped = ?4, created_at = ?5",
        params![credential, account, version, wrapped, now],
    )?;
    Ok(())
}

fn drop_undelivered(connection: &Connection, device: &Id, machine: Option<&Id>) -> Result<()> {
    let machine = machine.map(Id::as_str);
    connection.execute(
        "DELETE FROM queue WHERE (machine_id, seq) IN (
             SELECT machine_id, seq FROM commands
             WHERE device_id = ?1 AND delivered_at IS NULL AND (?2 IS NULL OR machine_id = ?2))",
        params![device.as_str(), machine],
    )?;
    connection.execute(
        "DELETE FROM commands
         WHERE device_id = ?1 AND delivered_at IS NULL AND (?2 IS NULL OR machine_id = ?2)",
        params![device.as_str(), machine],
    )?;
    Ok(())
}

fn connect_where(connection: &Connection, filter: &str, hash: &[u8]) -> Result<Option<Connect>> {
    connection
        .query_row(
            &format!(
                "SELECT device_code_hash, name, signing_key, encryption_key, state, account_id,
                        machine_id, pairing_id, poll_interval, polled_at, expires_at
                 FROM connects WHERE {filter} = ?1"
            ),
            [hash],
            |row| {
                Ok(Connect {
                    device_code_hash: row.get(0)?,
                    name: row.get(1)?,
                    keys: keys_at(row, 2)?,
                    state: ConnectState::parse(&row.get::<_, String>(4)?)
                        .ok_or_else(|| corrupt(4))?,
                    account: row.get(5)?,
                    machine: optional_id_at(row, 6)?,
                    pairing: row.get(7)?,
                    interval: row.get(8)?,
                    polled_at: row.get(9)?,
                    expires_at: row.get(10)?,
                })
            },
        )
        .optional()
}

fn corrupt(column: usize) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(
        column,
        "unexpected value".into(),
        rusqlite::types::Type::Text,
    )
}

fn id_at(row: &Row<'_>, column: usize) -> Result<Id> {
    Id::parse(&row.get::<_, String>(column)?).map_err(|_| corrupt(column))
}

fn optional_id_at(row: &Row<'_>, column: usize) -> Result<Option<Id>> {
    row.get::<_, Option<String>>(column)?
        .map(|text| Id::parse(&text).map_err(|_| corrupt(column)))
        .transpose()
}

fn bytes32_at(row: &Row<'_>, column: usize) -> Result<Option<[u8; 32]>> {
    row.get::<_, Option<Vec<u8>>>(column)?
        .map(|bytes| bytes.try_into().map_err(|_| corrupt(column)))
        .transpose()
}

fn keys_at(row: &Row<'_>, column: usize) -> Result<PublicKeys> {
    optional_keys_at(row, column)?.ok_or_else(|| corrupt(column))
}

fn optional_keys_at(row: &Row<'_>, column: usize) -> Result<Option<PublicKeys>> {
    match (bytes32_at(row, column)?, bytes32_at(row, column + 1)?) {
        (Some(signing), Some(encryption)) => Ok(Some(PublicKeys {
            signing,
            encryption,
        })),
        _ => Ok(None),
    }
}
