# Buzón

Webmail autoalojado: backend en Rust, frontend en React y SQLite (una base de datos para la aplicación y otra por usuario, con varias cuentas de correo cada uno).

- Plan completo: [docs/PLAN.md](docs/PLAN.md)
- Prototipo visual (fase 0): [prototype/](prototype/)
- Despliegue con systemd + nginx: [deploy/](deploy/)

## Estructura

| Carpeta | Qué contiene |
| --- | --- |
| `crates/store` | SQLite: `app.db` y una base de datos por usuario, ids UUIDv7/Base58, cifrado |
| `crates/sync` | Sincronización IMAP (un worker por cuenta, IDLE), análisis MIME y saneado del HTML |
| `crates/api` | API HTTP (`/api/v1`), OpenAPI y visor público en `/api/docs` |
| `crates/buzon` | Binario del servidor y su configuración |
| `web` | Frontend React + TypeScript (Vite, Tailwind), en español e inglés |
| `deploy` | Configuración de ejemplo, unidad systemd y sitio nginx |

## Desarrollo

Requisitos: Rust estable y Node 22.

```sh
# API en http://127.0.0.1:8080 (datos en ./data)
cargo run -p buzon -- --config deploy/config.dev.toml

# Frontend con recarga en http://localhost:5173 (hace de proxy de /api)
cd web && npm install && npm run dev
```

La primera vez, Buzón abre el asistente de configuración. La documentación de la API está en `/api/docs`.

Tests: `cargo test --workspace` y `cd web && npm test`.

Licencia: [MIT](LICENSE).
