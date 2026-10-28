-- Fase 2: caché local del correo de cada cuenta.

ALTER TABLE accounts ADD COLUMN imap_security TEXT NOT NULL DEFAULT 'tls';
ALTER TABLE accounts ADD COLUMN smtp_security TEXT NOT NULL DEFAULT 'tls';
ALTER TABLE accounts ADD COLUMN synced_at INTEGER;
ALTER TABLE accounts ADD COLUMN sync_error TEXT;

-- Carpetas IMAP. `role` viene de los atributos SPECIAL-USE (o del nombre).
CREATE TABLE mailboxes (
    id            BLOB PRIMARY KEY NOT NULL,
    account_id    BLOB NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    delimiter     TEXT,
    role          TEXT,
    uidvalidity   INTEGER,
    last_uid      INTEGER NOT NULL DEFAULT 0,
    -- UID más antiguo descargado; 0 cuando ya está todo el historial.
    oldest_uid    INTEGER,
    UNIQUE (account_id, name)
);

CREATE TABLE threads (
    id          BLOB PRIMARY KEY NOT NULL,
    account_id  BLOB NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    subject     TEXT NOT NULL,
    last_date   INTEGER NOT NULL
);
CREATE INDEX threads_by_date ON threads (last_date DESC, id DESC);

CREATE TABLE messages (
    id               BLOB PRIMARY KEY NOT NULL,
    account_id       BLOB NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    mailbox_id       BLOB NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    uid              INTEGER NOT NULL,
    thread_id        BLOB NOT NULL REFERENCES threads (id),
    message_id       TEXT,
    in_reply_to      TEXT,
    refs             TEXT NOT NULL DEFAULT '',
    subject          TEXT NOT NULL,
    from_name        TEXT,
    from_addr        TEXT,
    to_json          TEXT NOT NULL DEFAULT '[]',
    cc_json          TEXT NOT NULL DEFAULT '[]',
    date             INTEGER NOT NULL,
    snippet          TEXT NOT NULL DEFAULT '',
    size             INTEGER NOT NULL DEFAULT 0,
    seen             INTEGER NOT NULL DEFAULT 0,
    flagged          INTEGER NOT NULL DEFAULT 0,
    answered         INTEGER NOT NULL DEFAULT 0,
    has_attachments  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (mailbox_id, uid)
);
CREATE INDEX messages_by_thread ON messages (thread_id, date);
CREATE INDEX messages_by_msgid ON messages (account_id, message_id);
CREATE INDEX messages_by_reply ON messages (account_id, in_reply_to);

-- Cuerpo ya saneado y lista de adjuntos. El mensaje original está en disco.
CREATE TABLE message_bodies (
    message_id     BLOB PRIMARY KEY NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    text           TEXT,
    html           TEXT,
    remote_images  INTEGER NOT NULL DEFAULT 0,
    attachments    TEXT NOT NULL DEFAULT '[]'
);
