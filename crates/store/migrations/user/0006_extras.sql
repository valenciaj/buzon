-- Fase 7: envío programado (y «deshacer envío») y seguimiento de respuestas.

-- Un envío programado es un borrador con fecha de salida. Si falla, vuelve a
-- Borradores con el error.
ALTER TABLE drafts ADD COLUMN send_at INTEGER;
ALTER TABLE drafts ADD COLUMN send_error TEXT;
-- Días tras el envío para avisar si nadie ha respondido.
ALTER TABLE drafts ADD COLUMN follow_up_days INTEGER;
CREATE INDEX drafts_by_send_at ON drafts (send_at) WHERE send_at IS NOT NULL;

-- Correos enviados a la espera de respuesta. El hilo se busca por Message-ID,
-- porque la copia en Enviados llega después con la sincronización.
CREATE TABLE followups (
    id            BLOB PRIMARY KEY NOT NULL,
    account_id    BLOB NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    message_id    TEXT NOT NULL,
    subject       TEXT NOT NULL DEFAULT '',
    to_json       TEXT NOT NULL DEFAULT '[]',
    sent_at       INTEGER NOT NULL,
    due_at        INTEGER NOT NULL,
    dismissed_at  INTEGER
);
CREATE INDEX followups_by_due ON followups (due_at) WHERE dismissed_at IS NULL;
