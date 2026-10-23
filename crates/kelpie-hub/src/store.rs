use std::path::Path;

use kelpie_hub_api::{PairingFailure, PairingState};
use kelpie_hub_protocol::{BugReport, Header, Id, PublicKeys};
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

const SCHEMA_VERSION: i64 = 5;
const UPGRADES: &[(i64, &str)] = &[(5, REPORTS_SCHEMA)];
pub const REPORT_KEEP_SECONDS: u64 = 30 * 86_400;
pub const COMMAND_KEEP_SECONDS: u64 = 7 * 86_400;
const PAIRING_KEEP_SECONDS: u64 = 3_600;
const EVENT_KEEP_SECONDS: u64 = 30 * 86_400;

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

pub struct StoredBoard {
    pub machine: Id,
    pub name: String,
    pub last_seen: Option<u64>,
    pub rev: u64,
    pub stored_at: u64,
    pub envelope: String,
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
                    "{} was written by another kelpie-hub version (schema {version}); start with an empty data directory",
                    path.display()
                )),
            ));
        }
        let mut connection = connection;
        let tx = connection.transaction()?;
        if fresh || version == SCHEMA_VERSION {
            tx.execute_batch(SCHEMA)?;
            tx.execute_batch(REPORTS_SCHEMA)?;
        } else {
            for (_, upgrade) in UPGRADES.iter().filter(|(to, _)| *to > version) {
                tx.execute_batch(upgrade)?;
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
                "SELECT id FROM machines WHERE token_hash = ?1",
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
                "SELECT id FROM devices WHERE token_hash = ?1",
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
            "UPDATE devices SET last_seen = ?2 WHERE id = ?1",
            params![device.as_str(), now],
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
                    device_signing, device_encryption, device_nonce, expires_at
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
                })
            })?
            .collect()
    }

    pub fn save_pairing(&self, pairing: &Pairing) -> Result<()> {
        self.connection.execute(
            "UPDATE pairings SET state = ?2, reason = ?3, device_id = ?4, commitment = ?5,
                    machine_signing = ?6, machine_encryption = ?7, machine_nonce = ?8,
                    device_signing = ?9, device_encryption = ?10, device_nonce = ?11
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

    pub fn boards_for_device(&self, device: &Id) -> Result<Vec<StoredBoard>> {
        let mut statement = self.connection.prepare(
            "SELECT boards.machine_id, boards.rev, boards.stored_at, boards.envelope, machines.name, machines.last_seen
             FROM admissions JOIN boards ON boards.machine_id = admissions.machine_id
             JOIN machines ON machines.id = admissions.machine_id
             WHERE admissions.device_id = ?1 ORDER BY boards.machine_id",
        )?;
        statement
            .query_map([device.as_str()], |row| {
                Ok(StoredBoard {
                    machine: id_at(row, 0)?,
                    name: row.get(4)?,
                    last_seen: row.get(5)?,
                    rev: row.get(1)?,
                    stored_at: row.get(2)?,
                    envelope: row.get(3)?,
                })
            })?
            .collect()
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

    pub fn device_exists(&self, device: &Id) -> Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1)",
            [device.as_str()],
            |row| row.get(0),
        )
    }

    pub fn heads_for_device(&self, device: &Id) -> Result<Vec<BoardHead>> {
        let mut statement = self.connection.prepare(
            "SELECT admissions.machine_id, boards.envelope_id, boards.rev
             FROM admissions LEFT JOIN boards ON boards.machine_id = admissions.machine_id
             WHERE admissions.device_id = ?1 ORDER BY admissions.machine_id",
        )?;
        statement
            .query_map([device.as_str()], |row| {
                let envelope: Option<String> = row.get(1)?;
                let rev: Option<u64> = row.get(2)?;
                Ok(BoardHead {
                    machine: id_at(row, 0)?,
                    board: envelope.zip(rev),
                })
            })?
            .collect()
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
            "SELECT id, name, joined_at, last_seen FROM machines ORDER BY joined_at, id",
        )?;
        statement
            .query_map([], |row| {
                Ok(MachineRow {
                    id: id_at(row, 0)?,
                    name: row.get(1)?,
                    joined_at: row.get(2)?,
                    last_seen: row.get(3)?,
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

    pub fn invite_valid(&self, code_hash: &[u8], now: u64) -> Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM invites WHERE code_hash = ?1 AND expires_at > ?2)",
            params![code_hash, now],
            |row| row.get(0),
        )
    }

    pub fn handle_taken(&self, handle: &str) -> Result<bool> {
        self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE handle = ?1)",
            [handle],
            |row| row.get(0),
        )
    }

    pub fn account_by_handle(&self, handle: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT id FROM accounts WHERE handle = ?1",
                [handle],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn account(&self, account: &str) -> Result<Option<AccountRow>> {
        self.connection
            .query_row(
                "SELECT id, handle, user_handle, created_at FROM accounts WHERE id = ?1",
                [account],
                |row| {
                    Ok(AccountRow {
                        id: row.get(0)?,
                        handle: row.get(1)?,
                        user_handle: row
                            .get::<_, Vec<u8>>(2)?
                            .try_into()
                            .map_err(|_| corrupt(2))?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()
    }

    pub fn save_ceremony(&self, ceremony: &Ceremony) -> Result<()> {
        self.connection.execute(
            "INSERT INTO ceremonies (id_hash, purpose, account_id, handle, user_handle, invite_hash, state, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                ceremony.id_hash,
                ceremony.purpose.name(),
                ceremony.account,
                ceremony.handle,
                ceremony.user_handle.map(|bytes| bytes.to_vec()),
                ceremony.invite_hash,
                ceremony.state,
                ceremony.expires_at,
            ],
        )?;
        Ok(())
    }

    pub fn take_ceremony(
        &self,
        id_hash: &[u8],
        purpose: Purpose,
        now: u64,
    ) -> Result<Option<Ceremony>> {
        let ceremony = self
            .connection
            .query_row(
                "DELETE FROM ceremonies WHERE id_hash = ?1
                 RETURNING id_hash, purpose, account_id, handle, user_handle, invite_hash, state, expires_at",
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
                    })
                },
            )
            .optional()?;
        Ok(ceremony.filter(|ceremony| ceremony.purpose == purpose && ceremony.expires_at > now))
    }

    pub fn sign_up(&mut self, signup: &SignUp<'_>) -> Result<SignUpOutcome> {
        let tx = self.connection.transaction()?;
        let spent = tx.execute(
            "DELETE FROM invites WHERE code_hash = ?1 AND expires_at > ?2",
            params![signup.invite_hash, signup.now],
        )?;
        if spent == 0 {
            return Ok(SignUpOutcome::InviteSpent);
        }
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE handle = ?1)",
            [signup.handle],
            |row| row.get(0),
        )?;
        if taken {
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
            "INSERT INTO accounts (id, handle, user_handle, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                signup.account,
                signup.handle,
                signup.user_handle.to_vec(),
                signup.now
            ],
        )?;
        tx.execute(
            "INSERT INTO passkeys (credential_id, account_id, name, passkey, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                signup.credential_id,
                signup.account,
                signup.name,
                signup.passkey,
                signup.now
            ],
        )?;
        tx.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![signup.session_hash, signup.account, signup.now, signup.session_expires_at],
        )?;
        tx.commit()?;
        Ok(SignUpOutcome::Created)
    }

    pub fn passkeys(&self, account: &str) -> Result<Vec<StoredPasskey>> {
        let mut statement = self.connection.prepare(
            "SELECT credential_id, name, passkey, created_at, last_used_at, counter
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
        now: u64,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO passkeys (credential_id, account_id, name, passkey, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (credential_id) DO NOTHING",
            params![credential_id, account, name, passkey, now],
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
        self.connection
            .query_row(
                "SELECT account_id FROM sessions WHERE token_hash = ?1 AND expires_at > ?2",
                params![token_hash, now],
                |row| row.get(0),
            )
            .optional()
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

    pub fn add_admin_token(&self, token_hash: &[u8], name: &str, now: u64) -> Result<bool> {
        Ok(self.connection.execute(
            "INSERT INTO admin_tokens (token_hash, name, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (name) DO NOTHING",
            params![token_hash, name, now],
        )? > 0)
    }

    pub fn admin_token_valid(&self, token_hash: &[u8], now: u64) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE admin_tokens SET last_used = ?2 WHERE token_hash = ?1",
            params![token_hash, now],
        )? > 0)
    }

    pub fn admin_tokens(&self) -> Result<Vec<(String, u64, Option<u64>)>> {
        let mut statement = self.connection.prepare(
            "SELECT name, created_at, last_used FROM admin_tokens ORDER BY created_at, name",
        )?;
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect()
    }

    pub fn revoke_admin_token(&self, name: &str) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM admin_tokens WHERE name = ?1", [name])?
            > 0)
    }

    pub fn sweep(&self, now: u64) -> Result<Sweep> {
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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    pub sessions: usize,
    pub devices: usize,
}

pub struct BoardHead {
    pub machine: Id,
    pub board: Option<(String, u64)>,
}

pub struct MachineRow {
    pub id: Id,
    pub name: String,
    pub joined_at: u64,
    pub last_seen: Option<u64>,
}

pub struct AccountRow {
    pub id: String,
    pub handle: String,
    pub user_handle: [u8; 16],
    pub created_at: u64,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    SignUp,
    AddPasskey,
    LogIn,
}

impl Purpose {
    pub fn name(self) -> &'static str {
        match self {
            Purpose::SignUp => "sign-up",
            Purpose::AddPasskey => "add-passkey",
            Purpose::LogIn => "log-in",
        }
    }

    fn parse(name: &str) -> Option<Purpose> {
        [Purpose::SignUp, Purpose::AddPasskey, Purpose::LogIn]
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
}

pub struct SignUp<'a> {
    pub invite_hash: &'a [u8],
    pub account: &'a str,
    pub handle: &'a str,
    pub user_handle: [u8; 16],
    pub credential_id: &'a [u8],
    pub name: Option<&'a str>,
    pub passkey: &'a str,
    pub session_hash: &'a [u8],
    pub session_expires_at: u64,
    pub now: u64,
}

pub enum SignUpOutcome {
    Created,
    InviteSpent,
    HandleTaken,
    CredentialTaken,
}

pub enum Removal {
    Removed,
    Last,
    Missing,
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
