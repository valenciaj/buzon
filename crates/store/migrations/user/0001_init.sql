-- Base de datos de cada usuario: sus preferencias y sus cuentas de correo.

CREATE TABLE prefs (
    key    TEXT PRIMARY KEY NOT NULL,
    value  TEXT NOT NULL
);

CREATE TABLE accounts (
    id           BLOB PRIMARY KEY NOT NULL,
    name         TEXT NOT NULL,
    email        TEXT NOT NULL,
    imap_host    TEXT NOT NULL,
    imap_port    INTEGER NOT NULL,
    smtp_host    TEXT NOT NULL,
    smtp_port    INTEGER NOT NULL,
    login        TEXT NOT NULL,
    secret       BLOB NOT NULL,
    created_at   INTEGER NOT NULL
);
