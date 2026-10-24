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
PRAGMA user_version = 6;
