PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS teams (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    head_coach        TEXT NOT NULL DEFAULT '',
    assistant_coaches TEXT NOT NULL DEFAULT '',
    logo_path         TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS players (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id          INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    number           TEXT NOT NULL DEFAULT '',
    name             TEXT NOT NULL,
    default_position TEXT NOT NULL DEFAULT '',
    sort_order       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS lineups (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id    INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    name       TEXT NOT NULL DEFAULT '',
    opponent   TEXT NOT NULL DEFAULT '',
    game_date  TEXT NOT NULL DEFAULT '',
    location   TEXT NOT NULL DEFAULT '',
    home_away  TEXT NOT NULL DEFAULT '',
    use_dh     INTEGER NOT NULL DEFAULT 0,
    use_eh     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS lineup_spots (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    lineup_id     INTEGER NOT NULL REFERENCES lineups(id) ON DELETE CASCADE,
    batting_order INTEGER NOT NULL,
    slot_kind     TEXT NOT NULL DEFAULT 'BAT',
    player_id     INTEGER REFERENCES players(id) ON DELETE SET NULL,
    position      TEXT NOT NULL DEFAULT '',
    -- High-school two-way player: bats and also plays defense as the DH.
    is_dh         INTEGER NOT NULL DEFAULT 0
);
