#!/usr/bin/env bash
# Roda o editor localmente com os logs de wgpu/eframe ligados.
# Uso: ./run.sh [args do cargo...]
set -euo pipefail
cd "$(dirname "$0")"

export RUST_LOG="${RUST_LOG:-wgpu=debug,eframe=debug}"
exec cargo run --release -p editor_app "$@"