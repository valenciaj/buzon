-- Cómo se manda la contraseña: `auto`, `plain` (AUTHENTICATE PLAIN / AUTH PLAIN) o `login`
-- (LOGIN en IMAP, AUTH LOGIN en SMTP). No se usa con OAuth2.
ALTER TABLE accounts ADD COLUMN auth_mech TEXT NOT NULL DEFAULT 'auto';
