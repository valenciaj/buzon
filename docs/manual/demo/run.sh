#!/usr/bin/env bash
# Regenera las capturas del manual (public/img/es y public/img/en) con una instancia
# de Buzón de demostración y servidores de correo falsos. Necesita el binario
# (cargo build -p buzon), la web compilada (cd web && npm run build) y Chromium de Playwright.
set -euo pipefail
cd "$(dirname "$0")"
bin=${BUZON_BIN:-../../../target/debug/buzon}
pids=()
stop() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; pids=(); }
trap stop EXIT

for lang in es en; do
  rm -rf data deliver
  python3 demo_imap.py 3143 "$lang" . > imap.log 2>&1 & pids+=($!)
  python3 demo_smtp.py 3025 > smtp.log 2>&1 & pids+=($!)
  "$bin" --config config.toml > buzon.log 2>&1 & pids+=($!)
  for _ in $(seq 50); do curl -sf http://127.0.0.1:8089/api/v1/health >/dev/null && break; sleep 0.2; done
  node ../scripts/screenshots.mjs "$lang"
  stop
done
rm -rf data deliver ./*.log
