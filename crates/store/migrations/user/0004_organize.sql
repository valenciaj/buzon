-- Fase 4: mover, buscar, etiquetas, filtros automáticos y contactos.

-- Mensaje movido desde Buzón que el servidor aún no ha confirmado en su nueva carpeta.
-- Mientras tanto su `uid` es negativo; al llegar la copia real se borra.
ALTER TABLE messages ADD COLUMN moved_at INTEGER;

-- Búsqueda de texto completo. `rowid` es el de `messages`.
CREATE VIRTUAL TABLE messages_fts USING fts5 (
    subject, people, body,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO messages_fts (rowid, subject, people, body)
SELECT m.rowid, m.subject,
       COALESCE(m.from_name, '') || ' ' || COALESCE(m.from_addr, '') || ' ' || m.to_json || ' ' || m.cc_json,
       COALESCE(bd.text, '')
FROM messages m LEFT JOIN message_bodies bd ON bd.message_id = m.id;

-- El cuerpo se guarda justo después del mensaje, en la misma transacción.
CREATE TRIGGER messages_fts_insert AFTER INSERT ON message_bodies BEGIN
    INSERT INTO messages_fts (rowid, subject, people, body)
    SELECT m.rowid, m.subject,
           COALESCE(m.from_name, '') || ' ' || COALESCE(m.from_addr, '') || ' ' || m.to_json || ' ' || m.cc_json,
           COALESCE(new.text, '')
    FROM messages m WHERE m.id = new.message_id;
END;

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.rowid;
END;

-- Etiquetas propias de Buzón (no se guardan en el servidor IMAP).
CREATE TABLE labels (
    id          BLOB PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color       TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE thread_labels (
    thread_id  BLOB NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    label_id   BLOB NOT NULL REFERENCES labels (id) ON DELETE CASCADE,
    PRIMARY KEY (thread_id, label_id)
);
CREATE INDEX thread_labels_by_label ON thread_labels (label_id);

-- Filtros automáticos para el correo nuevo que llega a la bandeja de entrada.
-- Las condiciones vacías no cuentan; las que hay deben cumplirse todas.
CREATE TABLE rules (
    id            BLOB PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    position      INTEGER NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    account_id    BLOB REFERENCES accounts (id) ON DELETE CASCADE,
    from_has      TEXT NOT NULL DEFAULT '',
    to_has        TEXT NOT NULL DEFAULT '',
    subject_has   TEXT NOT NULL DEFAULT '',
    body_has      TEXT NOT NULL DEFAULT '',
    label_id      BLOB REFERENCES labels (id) ON DELETE SET NULL,
    mark_read     INTEGER NOT NULL DEFAULT 0,
    star          INTEGER NOT NULL DEFAULT 0,
    -- Tipo de carpeta destino: archive, trash o junk. Vacío: se queda en la entrada.
    move_to       TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL
);

-- Libreta de contactos.
CREATE TABLE contacts (
    id          BLOB PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL DEFAULT '',
    email       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    phone       TEXT NOT NULL DEFAULT '',
    notes       TEXT NOT NULL DEFAULT '',
    -- manual, sent (alguien a quien escribiste) o vcard.
    source      TEXT NOT NULL DEFAULT 'manual',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
