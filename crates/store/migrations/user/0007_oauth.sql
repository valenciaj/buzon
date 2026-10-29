-- Cuentas que entran con OAuth2 (Gmail, Outlook): `secret` guarda el token de renovación.
ALTER TABLE accounts ADD COLUMN auth TEXT NOT NULL DEFAULT 'password';
