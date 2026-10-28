-- Fase 3: identidades (remitentes y firmas), borradores y adjuntos subidos.

CREATE TABLE identities (
    id          BLOB PRIMARY KEY NOT NULL,
    account_id  BLOB NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name        TEXT NOT NULL DEFAULT '',
    email       TEXT NOT NULL,
    signature   TEXT NOT NULL DEFAULT '',
    is_default  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE TABLE drafts (
    id           BLOB PRIMARY KEY NOT NULL,
    identity_id  BLOB REFERENCES identities (id) ON DELETE SET NULL,
    to_json      TEXT NOT NULL DEFAULT '[]',
    cc_json      TEXT NOT NULL DEFAULT '[]',
    bcc_json     TEXT NOT NULL DEFAULT '[]',
    subject      TEXT NOT NULL DEFAULT '',
    html         TEXT NOT NULL DEFAULT '',
    text         TEXT NOT NULL DEFAULT '',
    -- Mensaje al que responde o que reenvía.
    reply_to     BLOB,
    forward_of   BLOB,
    attachments  TEXT NOT NULL DEFAULT '[]',
    updated_at   INTEGER NOT NULL
);

-- Ficheros subidos para adjuntar; el contenido está en disco (users/<id>/uploads).
CREATE TABLE uploads (
    id            BLOB PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    content_type  TEXT NOT NULL,
    size          INTEGER NOT NULL,
    created_at    INTEGER NOT NULL
);
