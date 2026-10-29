#!/usr/bin/env bash
# Compila Buzón y prepara un paquete para instalar en el servidor:
#   dist/buzon-<versión>-<arquitectura>.tar.gz
# Requisitos: Rust estable y Node 22. Para el PDF del manual hace falta Chromium
# de Playwright (npx playwright install chromium); sin él, se omite el PDF.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
name="buzon-$version-$arch"
out="dist/$name"

echo "==> Servidor (Rust)"
cargo build --release --locked -p buzon

echo "==> Web (React)"
(cd web && npm ci && npm run build)

echo "==> Manual"
(cd docs/manual && npm ci && npm run build && { npm run pdf || echo "Aviso: no se pudo generar el PDF del manual."; })

rm -rf "$out"
mkdir -p "$out"
cp target/release/buzon "$out/"
cp -r web/dist "$out/web"
cp -r deploy "$out/deploy"
cp README.md LICENSE "$out/"
tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "==> Listo: dist/$name.tar.gz"
