use std::path::Path;

use kelpie_hub_api::{PairingFailure, PairingState};
use kelpie_hub_protocol::{Header, Id, PublicKeys};
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
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE,
    signing_key BLOB,
    encryption_key BLOB,
    created_at INTEGER NOT NULL,
    last_seen INTEGER
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
    PRIMARY KEY (machine_id, seq)
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
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    machine_id TEXT,
    device_id TEXT,
    detail TEXT
);
";

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
    pub rev: u64,
    pub stored_at: u64,
    pub envelope: String,
}

pub enum BoardWrite {
    Stored,
    Stale,
    Replayed,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(SCHEMA)?;
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

    pub fn enqueue(&mut self, machine: &Id, envelope: &str, now: u64) -> Result<u64> {
        let tx = self.connection.transaction()?;
        let seq: u64 = tx.query_row(
            "UPDATE machines SET last_seq = last_seq + 1 WHERE id = ?1 RETURNING last_seq",
            [machine.as_str()],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO queue (machine_id, seq, envelope, queued_at) VALUES (?1, ?2, ?3, ?4)",
            params![machine.as_str(), seq, envelope, now],
        )?;
        tx.commit()?;
        Ok(seq)
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
        self.pairing_where("id = ?1", rusqlite::types::Value::Text(id.to_string()))
    }

    pub fn pairing_by_code(&self, code_hash: &[u8]) -> Result<Option<Pairing>> {
        self.pairing_where(
            "code_hash = ?1",
            rusqlite::types::Value::Blob(code_hash.to_vec()),
        )
    }

    fn pairing_where(
        &self,
        filter: &str,
        value: rusqlite::types::Value,
    ) -> Result<Option<Pairing>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT id, machine_id, state, reason, device_id, commitment,
                            machine_signing, machine_encryption, machine_nonce,
                            device_signing, device_encryption, device_nonce, expires_at
                     FROM pairings WHERE {filter}"
                ),
                [value],
                |row| {
                    Ok(Pairing {
                        id: row.get(0)?,
                        machine: id_at(row, 1)?,
                        state: PairingState::parse(&row.get::<_, String>(2)?)
                            .ok_or_else(|| corrupt(2))?,
                        reason: row
                            .get::<_, Option<String>>(3)?
                            .map(|name| PairingFailure::parse(&name).ok_or_else(|| corrupt(3)))
                            .transpose()?,
                        device: row
                            .get::<_, Option<String>>(4)?
                            .map(|text| Id::parse(&text).map_err(|_| corrupt(4)))
                            .transpose()?,
                        commitment: bytes32_at(row, 5)?,
                        machine_keys: optional_keys_at(row, 6)?,
                        machine_nonce: bytes32_at(row, 8)?,
                        device_keys: optional_keys_at(row, 9)?,
                        device_nonce: bytes32_at(row, 11)?,
                        expires_at: row.get(12)?,
                    })
                },
            )
            .optional()
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

    pub fn create_device(&self, device: &Id, token_hash: &[u8], now: u64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO devices (id, token_hash, created_at, last_seen) VALUES (?1, ?2, ?3, ?3)",
            params![device.as_str(), token_hash, now],
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
            "SELECT boards.machine_id, boards.rev, boards.stored_at, boards.envelope
             FROM admissions JOIN boards ON boards.machine_id = admissions.machine_id
             WHERE admissions.device_id = ?1 ORDER BY boards.machine_id",
        )?;
        statement
            .query_map([device.as_str()], |row| {
                Ok(StoredBoard {
                    machine: id_at(row, 0)?,
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

    pub fn sweep(&self, now: u64) -> Result<()> {
        self.connection
            .execute("DELETE FROM join_codes WHERE expires_at <= ?1", [now])?;
        self.connection.execute(
            "DELETE FROM pairings WHERE expires_at + ?2 <= ?1",
            params![now, PAIRING_KEEP_SECONDS],
        )?;
        self.connection.execute(
            "DELETE FROM events WHERE at + ?2 <= ?1",
            params![now, EVENT_KEEP_SECONDS],
        )?;
        Ok(())
    }
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
