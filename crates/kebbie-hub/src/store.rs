use std::path::Path;

use kebbie_hub_api::{ConnectState, PairingFailure, PairingState};
use kebbie_hub_protocol::{AccountKey, Header, Id, MachineList, PublicKeys};
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
    last_seen INTEGER,
    account_id TEXT REFERENCES accounts(id) ON DELETE SET NULL
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
    created_at INTEGER NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    deactivated_at INTEGER,
    github_login TEXT,
    created_via TEXT
);
CREATE TABLE IF NOT EXISTS invites (
    code_hash BLOB PRIMARY KEY,
    expires_at INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'invite'
);
CREATE TABLE IF NOT EXISTS passkeys (
    credential_id BLOB PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT,
    passkey TEXT NOT NULL,
    counter INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    prf_salt BLOB
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash BLOB PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    keyless INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS ceremonies (
    id_hash BLOB PRIMARY KEY,
    purpose TEXT NOT NULL,
    account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    handle TEXT,
    user_handle BLOB,
    invite_hash BLOB,
    state TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    prf_salt BLOB
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
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    command_id TEXT NOT NULL,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    digest BLOB NOT NULL,
    seq INTEGER NOT NULL,
    queued_at INTEGER NOT NULL,
    delivered_at INTEGER,
    PRIMARY KEY (account_id, command_id),
    UNIQUE (machine_id, command_id)
);
CREATE INDEX IF NOT EXISTS commands_by_seq ON commands (machine_id, seq);
CREATE TABLE IF NOT EXISTS results (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    command_id TEXT NOT NULL,
    digest BLOB NOT NULL,
    envelope TEXT NOT NULL,
    stored_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    UNIQUE (account_id, command_id)
);
CREATE INDEX IF NOT EXISTS results_by_account ON results (account_id, seq);
CREATE TABLE IF NOT EXISTS pairings (
    id TEXT PRIMARY KEY,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    state TEXT NOT NULL,
    reason TEXT,
    commitment BLOB NOT NULL,
    machine_signing BLOB,
    machine_encryption BLOB,
    machine_nonce BLOB,
    account_signing BLOB,
    account_encryption BLOB,
    account_key_version INTEGER,
    account_nonce BLOB,
    opened_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    machine_id TEXT,
    detail TEXT
);
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
    expires_at INTEGER NOT NULL,
    signup_at INTEGER,
    passkey_account TEXT,
    passkey_until INTEGER
);
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
CREATE TABLE IF NOT EXISTS blobs (
    id TEXT NOT NULL,
    machine_id TEXT NOT NULL REFERENCES machines(id) ON DELETE CASCADE,
    ciphertext BLOB NOT NULL,
    stored_at INTEGER NOT NULL,
    PRIMARY KEY (id, machine_id)
);
";

const SCHEMA_VERSION: i64 = 15;
// Schemas 13 and 14 open as 15: their device, pairing, command, result, device-list, notice and event tables are dropped and the rest is kept.
const COMPATIBLE_VERSIONS: [i64; 2] = [13, 14];
const DROPPED_FOR_ACCOUNT_COMMANDS: &str = "
DROP TABLE IF EXISTS push_subscriptions;
DROP TABLE IF EXISTS admissions;
DROP TABLE IF EXISTS pairings;
DROP TABLE IF EXISTS device_lists;
DROP TABLE IF EXISTS results;
DROP TABLE IF EXISTS commands;
DROP TABLE IF EXISTS notices;
DROP TABLE IF EXISTS devices;
DROP TABLE IF EXISTS events;
";
pub const COMMAND_KEEP_SECONDS: u64 = 7 * 86_400;
const PAIRING_KEEP_SECONDS: u64 = 3_600;
const EVENT_KEEP_SECONDS: u64 = 30 * 86_400;
// The hub cannot read which blobs a board references, so a blob lives this long after its machine last PUT it.
pub const BLOB_KEEP_SECONDS: u64 = 14 * 86_400;
pub const BLOB_QUOTA_BYTES: u64 = 32 * 1024 * 1024;

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Debug)]
pub struct Pairing {
    pub id: String,
    pub machine: Id,
    pub state: PairingState,
    pub reason: Option<PairingFailure>,
    pub commitment: [u8; 32],
    pub machine_keys: Option<PublicKeys>,
    pub machine_nonce: Option<[u8; 32]>,
    pub account: Id,
    pub account_keys: Option<PublicKeys>,
    pub account_key_version: Option<u32>,
    pub account_nonce: Option<[u8; 32]>,
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

#[derive(Debug, PartialEq, Eq)]
pub enum BlobWrite {
    Stored,
    Renewed,
    OverQuota,
}

pub enum BoardWrite {
    Stored,
    Stale,
    Replayed,
}

pub struct NewCommand<'a> {
    pub account: &'a str,
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
    pub account: &'a str,
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
    pub account: String,
    pub delivered: bool,
}

pub struct ResultHead {
    pub seq: u64,
    pub machine: Id,
    pub command: Id,
}

pub struct StoredResult {
    pub seq: u64,
    pub machine: Id,
    pub command: Id,
    pub stored_at: u64,
    pub envelope: String,
}

impl Store {
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

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
        if !fresh && version != SCHEMA_VERSION && !COMPATIBLE_VERSIONS.contains(&version) {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISMATCH),
                Some(format!(
                    "{} holds hub schema {version}, and this kebbie-hub opens schemas 13 to {SCHEMA_VERSION} only; start with an empty data directory",
                    path.display()
                )),
            ));
        }
        let mut connection = connection;
        let tx = connection.transaction()?;
        if COMPATIBLE_VERSIONS.contains(&version) {
            tx.execute_batch(DROPPED_FOR_ACCOUNT_COMMANDS)?;
        }
        if fresh || COMPATIBLE_VERSIONS.contains(&version) {
            tx.execute_batch(SCHEMA)?;
        }
        let marked: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'keyless')",
            [],
            |row| row.get(0),
        )?;
        if !marked {
            tx.execute_batch("ALTER TABLE sessions ADD COLUMN keyless INTEGER NOT NULL DEFAULT 0")?;
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

    pub fn board_rev(&self, machine: &Id) -> Result<Option<u64>> {
        self.connection
            .query_row(
                "SELECT rev FROM boards WHERE machine_id = ?1",
                [machine.as_str()],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn store_blob(&mut self, machine: &Id, id: &Id, ciphertext: &[u8], now: u64) -> Result<BlobWrite> {
        let tx = self.connection.transaction()?;
        let renewed = tx.execute(
            "UPDATE blobs SET stored_at = ?3 WHERE id = ?1 AND machine_id = ?2",
            params![id.as_str(), machine.as_str(), now],
        )?;
        if renewed > 0 {
            tx.commit()?;
            return Ok(BlobWrite::Renewed);
        }
        let held: u64 = tx.query_row(
            "SELECT coalesce(sum(length(ciphertext)), 0) FROM blobs WHERE machine_id = ?1",
            [machine.as_str()],
            |row| row.get(0),
        )?;
        if held + ciphertext.len() as u64 > BLOB_QUOTA_BYTES {
            return Ok(BlobWrite::OverQuota);
        }
        tx.execute(
            "INSERT INTO blobs (id, machine_id, ciphertext, stored_at) VALUES (?1, ?2, ?3, ?4)",
            params![id.as_str(), machine.as_str(), ciphertext, now],
        )?;
        tx.commit()?;
        Ok(BlobWrite::Stored)
    }

    pub fn blob_for_account(&self, id: &Id, account: &str) -> Result<Option<Vec<u8>>> {
        self.connection
            .query_row(
                "SELECT blobs.ciphertext FROM blobs JOIN machines ON machines.id = blobs.machine_id
                 WHERE blobs.id = ?1 AND machines.account_id = ?2 LIMIT 1",
                params![id.as_str(), account],
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

    pub fn machines_of_account(&self, account: &str) -> Result<Vec<Id>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM machines WHERE account_id = ?1")?;
        statement
            .query_map([account], |row| id_at(row, 0))?
            .collect()
    }

    pub fn queued_count(&self, machine: &Id) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM queue WHERE machine_id = ?1",
            [machine.as_str()],
            |row| row.get(0),
        )
    }

    pub fn undelivered_count(&self, machine: &Id, account: &str) -> Result<u64> {
        self.connection.query_row(
            "SELECT count(*) FROM commands JOIN queue
                 ON queue.machine_id = commands.machine_id AND queue.seq = commands.seq
             WHERE commands.machine_id = ?1 AND commands.account_id = ?2
                 AND commands.delivered_at IS NULL",
            params![machine.as_str(), account],
            |row| row.get(0),
        )
    }

    pub fn resent_command(&self, account: &str, command: &Id) -> Result<Option<(Vec<u8>, u64)>> {
        self.connection
            .query_row(
                "SELECT digest, seq FROM commands WHERE account_id = ?1 AND command_id = ?2",
                params![account, command.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    pub fn queue_command(&mut self, new: &NewCommand<'_>) -> Result<CommandWrite> {
        let tx = self.connection.transaction()?;
        let held: Option<(String, Vec<u8>, u64)> = tx
            .query_row(
                "SELECT account_id, digest, seq FROM commands WHERE machine_id = ?1 AND command_id = ?2",
                params![new.machine.as_str(), new.command.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((account, digest, seq)) = held {
            return Ok(if account == new.account && digest == new.digest {
                CommandWrite::Resent(seq)
            } else {
                CommandWrite::IdTaken
            });
        }
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM commands WHERE account_id = ?1 AND command_id = ?2)",
            params![new.account, new.command.as_str()],
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
            "INSERT INTO commands (account_id, command_id, machine_id, kind, digest, seq, queued_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                new.account,
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
                "SELECT account_id, delivered_at IS NOT NULL FROM commands
                 WHERE machine_id = ?1 AND command_id = ?2",
                params![machine.as_str(), command.as_str()],
                |row| {
                    Ok(DeliveredCommand {
                        account: row.get(0)?,
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
                "SELECT digest, seq FROM results WHERE account_id = ?1 AND command_id = ?2",
                params![new.account, new.command.as_str()],
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
            "INSERT INTO results (account_id, machine_id, command_id, digest, envelope, stored_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING seq",
            params![
                new.account,
                new.machine.as_str(),
                new.command.as_str(),
                new.digest,
                new.envelope,
                new.now,
                new.expires_at
            ],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(ResultWrite::Stored(seq))
    }

    pub fn results_after(&self, account: &str, after: u64, now: u64) -> Result<Vec<StoredResult>> {
        let mut statement = self.connection.prepare(
            "SELECT seq, machine_id, command_id, stored_at, envelope FROM results
             WHERE account_id = ?1 AND seq > ?2 AND expires_at > ?3 ORDER BY seq",
        )?;
        statement
            .query_map(params![account, after, now], |row| {
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

    pub fn latest_result_seq(&self, account: &str) -> Result<u64> {
        self.connection.query_row(
            "SELECT coalesce(max(seq), 0) FROM results WHERE account_id = ?1",
            [account],
            |row| row.get(0),
        )
    }

    pub fn result_heads_after(&self, account: &str, after: u64, now: u64) -> Result<Vec<ResultHead>> {
        let mut statement = self.connection.prepare(
            "SELECT seq, machine_id, command_id FROM results
             WHERE account_id = ?1 AND seq > ?2 AND expires_at > ?3 ORDER BY seq",
        )?;
        statement
            .query_map(params![account, after, now], |row| {
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

    pub fn pairing(&self, id: &str) -> Result<Option<Pairing>> {
        Ok(self
            .pairings_where("id = ?1", rusqlite::types::Value::Text(id.to_string()))?
            .pop())
    }

    fn pairings_where(&self, filter: &str, value: rusqlite::types::Value) -> Result<Vec<Pairing>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT id, machine_id, state, reason, commitment,
                    machine_signing, machine_encryption, machine_nonce,
                    account_signing, account_encryption, account_nonce, expires_at,
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
                    commitment: bytes32_at(row, 4)?.ok_or_else(|| corrupt(4))?,
                    machine_keys: optional_keys_at(row, 5)?,
                    machine_nonce: bytes32_at(row, 7)?,
                    account_keys: optional_keys_at(row, 8)?,
                    account_nonce: bytes32_at(row, 10)?,
                    expires_at: row.get(11)?,
                    account: id_at(row, 12)?,
                    account_key_version: row.get(13)?,
                })
            })?
            .collect()
    }

    pub fn save_pairing(&self, pairing: &Pairing) -> Result<()> {
        self.connection.execute(
            "UPDATE pairings SET state = ?2, reason = ?3,
                    machine_signing = ?4, machine_encryption = ?5, machine_nonce = ?6,
                    account_signing = ?7, account_encryption = ?8, account_nonce = ?9,
                    account_key_version = ?10
             WHERE id = ?1",
            params![
                pairing.id,
                pairing.state.name(),
                pairing.reason.map(PairingFailure::name),
                pairing.machine_keys.map(|keys| keys.signing),
                pairing.machine_keys.map(|keys| keys.encryption),
                pairing.machine_nonce,
                pairing.account_keys.map(|keys| keys.signing),
                pairing.account_keys.map(|keys| keys.encryption),
                pairing.account_nonce,
                pairing.account_key_version,
            ],
        )?;
        Ok(())
    }

    pub fn log(
        &self,
        now: u64,
        kind: &str,
        machine: Option<&Id>,
        detail: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO events (at, kind, machine_id, detail) VALUES (?1, ?2, ?3, ?4)",
            params![now, kind, machine.map(Id::as_str), detail],
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
        prf_salt: Option<&[u8; 32]>,
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
        put_wrapper(
            &tx,
            account,
            credential,
            key.version,
            wrapped,
            prf_salt,
            now,
        )?;
        tx.commit()?;
        Ok(KeySet::Set)
    }

    pub fn set_wrapper(
        &self,
        account: &str,
        credential: &[u8],
        version: u32,
        wrapped: &[u8],
        prf_salt: Option<&[u8; 32]>,
        now: u64,
    ) -> Result<bool> {
        let tx = self.connection.unchecked_transaction()?;
        let owned: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE credential_id = ?1 AND account_id = ?2)",
            params![credential, account],
            |row| row.get(0),
        )?;
        if owned {
            put_wrapper(&tx, account, credential, version, wrapped, prf_salt, now)?;
            tx.commit()?;
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

    pub fn grant_connect(&self, user_code_hash: &[u8], account: &str, until: u64, now: u64) -> Result<()> {
        self.connection.execute(
            "UPDATE connects SET passkey_account = ?2, passkey_until = ?3
             WHERE user_code_hash = ?1 AND state = 'pending' AND expires_at > ?4",
            params![user_code_hash, account, until, now],
        )?;
        Ok(())
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
            "UPDATE connects SET state = 'approved', account_id = ?3, machine_id = ?4, pairing_id = ?5,
                                 passkey_account = NULL, passkey_until = NULL
             WHERE user_code_hash = ?1 AND state = 'pending' AND expires_at > ?2
               AND passkey_account = ?3 AND passkey_until > ?2",
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
                "INSERT INTO pairings (id, machine_id, state, account_id, commitment, opened_at, expires_at)
                 VALUES (?1, ?2, 'committed', ?3, ?4, ?5, ?6)",
                params![
                    pairing.id,
                    approval.machine.as_str(),
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
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at, keyless) VALUES (?1, ?2, ?3, ?4, 0)",
            params![signup.session_hash, signup.account, signup.now, signup.session_expires_at],
        )?;
        tx.commit()?;
        Ok(SignUpOutcome::Created(kind))
    }

    pub fn sign_up_keyless(&mut self, signup: &KeylessSignUp<'_>) -> Result<SignUpOutcome> {
        let tx = self.connection.transaction()?;
        let Some(kind) = spend_ticket(&tx, signup.invite_hash, signup.connect, signup.now)? else {
            return Ok(SignUpOutcome::InviteSpent);
        };
        if handle_held(&tx, signup.handle)? {
            return Ok(SignUpOutcome::HandleTaken);
        }
        if let Some((github_id, _)) = signup.github {
            let linked: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM accounts WHERE github_id = ?1)",
                [github_id],
                |row| row.get(0),
            )?;
            if linked {
                return Ok(SignUpOutcome::GitHubTaken);
            }
        }
        let (github_id, github_login) = signup.github.unzip();
        tx.execute(
            "INSERT INTO accounts (id, handle, user_handle, created_at, role, github_id, github_login, created_via)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                signup.account,
                signup.handle,
                signup.user_handle.to_vec(),
                signup.now,
                role_of(kind),
                github_id,
                github_login,
                kind.created_via()
            ],
        )?;
        tx.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at, keyless) VALUES (?1, ?2, ?3, ?4, 1)",
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
        access: Access,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at, keyless) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![token_hash, account, now, expires_at, access == Access::AccountOnly],
        )?;
        Ok(())
    }

    pub fn session_account(&self, token_hash: &[u8], now: u64) -> Result<Option<String>> {
        Ok(self.session(token_hash, now)?.map(|session| session.account))
    }

    pub fn session(&self, token_hash: &[u8], now: u64) -> Result<Option<Session>> {
        self.connection
            .query_row(
                "SELECT sessions.account_id, sessions.created_at, sessions.keyless FROM sessions
                 JOIN accounts ON accounts.id = sessions.account_id
                 WHERE sessions.token_hash = ?1 AND sessions.expires_at > ?2
                     AND accounts.deactivated_at IS NULL",
                params![token_hash, now],
                |row| {
                    Ok(Session {
                        account: row.get(0)?,
                        signed_in_at: row.get(1)?,
                        access: if row.get(2)? { Access::AccountOnly } else { Access::Key },
                    })
                },
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

    // Only the passkey that becomes an account's first key holder lifts its session; an account with a key stays as it was.
    pub fn grant_key_access(&self, token_hash: &[u8], account: &str, now: u64) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE sessions SET keyless = 0, created_at = ?3 WHERE token_hash = ?1 AND account_id = ?2 AND expires_at > ?3
                 AND NOT EXISTS (SELECT 1 FROM account_keys WHERE account_id = ?2)",
            params![token_hash, account, now],
        )? > 0)
    }

    pub fn end_session(&self, token_hash: &[u8]) -> Result<()> {
        self.connection
            .execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    }

    pub fn counts(&self) -> Result<(u64, u64)> {
        self.connection.query_row(
            "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM machines
                 WHERE NOT EXISTS (SELECT 1 FROM connects WHERE connects.machine_id = machines.id))",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }

    pub fn admin_accounts(&self) -> Result<Vec<AdminAccountRow>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {ACCOUNT_COLUMNS},
                    (SELECT count(*) FROM machines WHERE machines.account_id = accounts.id),
                    {LAST_SEEN}
             FROM accounts ORDER BY created_at, id"
        ))?;
        statement
            .query_map([], |row| {
                Ok(AdminAccountRow {
                    account: account_row(row)?,
                    machines: row.get(ACCOUNT_COLUMN_COUNT)?,
                    last_seen: seen_at(row.get(ACCOUNT_COLUMN_COUNT + 1)?),
                })
            })?
            .collect()
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
        self.connection.execute(
            "DELETE FROM events WHERE at + ?2 <= ?1",
            params![now, EVENT_KEEP_SECONDS],
        )?;
        self.connection.execute(
            "DELETE FROM blobs WHERE stored_at + ?2 <= ?1",
            params![now, BLOB_KEEP_SECONDS],
        )?;
        Ok(Sweep { sessions })
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    pub sessions: usize,
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

pub struct AdminAccountRow {
    pub account: AccountRow,
    pub machines: u64,
    pub last_seen: Option<u64>,
}

const ACCOUNT_COLUMNS: &str = "accounts.id, accounts.handle, accounts.user_handle, accounts.created_at, accounts.role, accounts.deactivated_at, accounts.github_id, accounts.github_login, accounts.created_via";
const ACCOUNT_COLUMN_COUNT: usize = 9;
const LAST_SEEN: &str = "coalesce((SELECT max(last_seen) FROM machines WHERE machines.account_id = accounts.id), 0)";

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
        deactivated_at: row.get(5)?,
        github_id: row.get(6)?,
        github_login: row.get(7)?,
        created_via: row.get(8)?,
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
    pub passkey_account: Option<String>,
    pub passkey_until: Option<u64>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Key,
    AccountOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub account: String,
    pub signed_in_at: u64,
    pub access: Access,
}

pub struct KeylessSignUp<'a> {
    pub invite_hash: &'a [u8],
    pub connect: bool,
    pub account: &'a str,
    pub handle: &'a str,
    pub user_handle: [u8; 16],
    pub github: Option<(u64, &'a str)>,
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
    prf_salt: Option<&[u8; 32]>,
    now: u64,
) -> Result<()> {
    connection.execute(
        "INSERT INTO key_wrappers (credential_id, account_id, key_version, wrapped, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (credential_id) DO UPDATE SET key_version = ?3, wrapped = ?4, created_at = ?5",
        params![credential, account, version, wrapped, now],
    )?;
    if let Some(salt) = prf_salt {
        connection.execute(
            "UPDATE passkeys SET prf_salt = ?3 WHERE credential_id = ?1 AND account_id = ?2",
            params![credential, account, salt.as_slice()],
        )?;
    }
    Ok(())
}

fn connect_where(connection: &Connection, filter: &str, hash: &[u8]) -> Result<Option<Connect>> {
    connection
        .query_row(
            &format!(
                "SELECT device_code_hash, name, signing_key, encryption_key, state, account_id,
                        machine_id, pairing_id, poll_interval, polled_at, expires_at,
                        passkey_account, passkey_until
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
                    passkey_account: row.get(11)?,
                    passkey_until: row.get(12)?,
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
