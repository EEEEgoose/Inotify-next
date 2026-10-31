CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    active        INTEGER NOT NULL DEFAULT 1,
    send_token    TEXT    NOT NULL UNIQUE,
    github_id     INTEGER UNIQUE,
    github_login  TEXT,
    avatar_url    TEXT,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE channels (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    -- Per-channel key: sending with it targets only this channel; doubles as the Bark device_key.
    key        TEXT    NOT NULL UNIQUE,
    config     TEXT    NOT NULL,
    active     INTEGER NOT NULL DEFAULT 1,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_channels_user ON channels(user_id);

CREATE TABLE send_stats (
    date    TEXT    NOT NULL,
    kind    TEXT    NOT NULL,
    success INTEGER NOT NULL DEFAULT 0,
    failed  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, kind)
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
