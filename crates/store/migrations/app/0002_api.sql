-- Fase 6: tokens personales de la API y webhooks.

-- Solo se guarda el hash del token. `prefix` son sus primeros caracteres, para reconocerlo.
-- scopes: permisos separados por espacios (mail:read, mail:write, mail:send, contacts, webhooks).
CREATE TABLE api_tokens (
    id            BLOB PRIMARY KEY NOT NULL,
    user_id       BLOB NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    prefix        TEXT NOT NULL,
    scopes        TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER,
    last_used_at  INTEGER,
    revoked_at    INTEGER
);
CREATE INDEX api_tokens_user ON api_tokens (user_id);

-- El secreto firma cada envío; va cifrado con la clave maestra.
-- events: eventos separados por espacios.
CREATE TABLE webhooks (
    id          BLOB PRIMARY KEY NOT NULL,
    user_id     BLOB NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    url         TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    events      TEXT NOT NULL,
    secret      BLOB NOT NULL,
    active      INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL
);
CREATE INDEX webhooks_user ON webhooks (user_id);

-- Cola persistente e historial de envíos.
-- status: pending (por enviar o reintentar), ok o failed (se agotaron los reintentos).
CREATE TABLE webhook_deliveries (
    id           BLOB PRIMARY KEY NOT NULL,
    webhook_id   BLOB NOT NULL REFERENCES webhooks (id) ON DELETE CASCADE,
    event        TEXT NOT NULL,
    payload      TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    attempts     INTEGER NOT NULL DEFAULT 0,
    next_at      INTEGER,
    response     INTEGER,
    error        TEXT,
    created_at   INTEGER NOT NULL,
    finished_at  INTEGER
);
CREATE INDEX webhook_deliveries_due ON webhook_deliveries (status, next_at);
CREATE INDEX webhook_deliveries_hook ON webhook_deliveries (webhook_id, created_at);
