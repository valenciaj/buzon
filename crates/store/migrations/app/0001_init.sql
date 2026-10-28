-- Base de datos de la aplicación: usuarios, sesiones, invitaciones y ajustes globales.
-- Todas las claves primarias son UUIDv7 guardados como BLOB de 16 bytes.

CREATE TABLE users (
    id             BLOB PRIMARY KEY NOT NULL,
    username       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    email          TEXT,
    password_hash  TEXT NOT NULL,
    role           TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    totp_secret    BLOB,
    totp_enabled   INTEGER NOT NULL DEFAULT 0,
    failed_logins  INTEGER NOT NULL DEFAULT 0,
    locked_until   INTEGER,
    language       TEXT NOT NULL DEFAULT 'es',
    created_at     INTEGER NOT NULL
);

CREATE TABLE sessions (
    id          BLOB PRIMARY KEY NOT NULL,
    token_hash  TEXT NOT NULL UNIQUE,
    user_id     BLOB NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    user_agent  TEXT,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE invitations (
    id          BLOB PRIMARY KEY NOT NULL,
    token_hash  TEXT NOT NULL UNIQUE,
    note        TEXT,
    created_by  BLOB NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    used_at     INTEGER,
    used_by     BLOB REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE settings (
    key    TEXT PRIMARY KEY NOT NULL,
    value  TEXT NOT NULL
);

CREATE TABLE audit_log (
    id          BLOB PRIMARY KEY NOT NULL,
    user_id     BLOB,
    action      TEXT NOT NULL,
    detail      TEXT,
    created_at  INTEGER NOT NULL
);
