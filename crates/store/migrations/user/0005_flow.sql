-- Fase 5: el flujo del modo Enfoque (inspirado en Hey).

-- Qué parece cada mensaje según sus cabeceras: principal, news (boletines) o receipts.
ALTER TABLE messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'principal';

-- Dominios de los píxeles espía quitados del HTML (JSON).
ALTER TABLE message_bodies ADD COLUMN trackers TEXT NOT NULL DEFAULT '[]';

-- Pilas y recordatorios de cada hilo. Viven en Buzón, no en el servidor IMAP.
ALTER TABLE threads ADD COLUMN reply_later INTEGER;
ALTER TABLE threads ADD COLUMN aside INTEGER;
-- Hasta cuándo está pospuesto. Pasada la fecha, el hilo reaparece arriba.
ALTER TABLE threads ADD COLUMN snooze_until INTEGER;
ALTER TABLE threads ADD COLUMN note TEXT NOT NULL DEFAULT '';

-- Remitentes que escriben a la bandeja de entrada y qué se ha decidido sobre ellos.
-- status: pending (por aprobar), approved o blocked.
-- bucket: dónde va su correo aprobado (principal, news o receipts).
CREATE TABLE senders (
    email       TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
    name        TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL,
    bucket      TEXT NOT NULL DEFAULT 'principal',
    created_at  INTEGER NOT NULL,
    decided_at  INTEGER
);
CREATE INDEX senders_by_status ON senders (status);
CREATE INDEX messages_by_from ON messages (from_addr);

-- Quien ya tenía correo en la bandeja de entrada queda aprobado, en la bandeja que toca.
INSERT OR IGNORE INTO senders (email, name, status, bucket, created_at)
SELECT lower(m.from_addr), COALESCE(MAX(m.from_name), ''), 'approved', 'principal', MIN(m.date)
FROM messages m JOIN mailboxes mb ON mb.id = m.mailbox_id
WHERE mb.role = 'inbox' AND m.from_addr IS NOT NULL AND m.from_addr <> ''
GROUP BY lower(m.from_addr);
