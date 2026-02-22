#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[1/4] Limpiando artefactos de compilación de Rust (cargo clean)..."
cargo clean --manifest-path src-tauri/Cargo.toml

echo "[2/4] Instalando dependencias JS (npm ci)..."
npm ci

echo "[3/4] Compilando frontend (npm run build)..."
npm run build

echo "[4/4] Generando bundle firmado de Tauri (npx tauri build)..."
npx tauri build

echo "Build segura finalizada. Recuerda adjuntar latest.json en cada release."
