-- Pedir confirmación de lectura (cabecera Disposition-Notification-To, RFC 8098).
ALTER TABLE drafts ADD COLUMN read_receipt INTEGER NOT NULL DEFAULT 0;
