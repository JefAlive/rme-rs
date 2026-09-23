#!/usr/bin/env bash
# Cross-compila o editor para Windows (x86_64-pc-windows-msvc) com cargo-xwin.
#
# Reproduz exatamente o fluxo que já funciona:
#   rustup target add x86_64-pc-windows-msvc
#   cargo install cargo-xwin
#   cargo xwin build --release -p editor_app --target x86_64-pc-windows-msvc
#
# Com o rust-toolchain.toml deste repo, o `rustup target add` nem precisa ser
# executado: o rustup instala o target automaticamente ao entrar no projeto.
#
# Diferença única: o cargo-xwin é instalado dentro do repo (em .tools/), e não
# no cargo global. Se preferir install global, rode `cargo install cargo-xwin`
# uma vez e este script passa direto.
#
# Uso: ./build_windows.sh [args do cargo xwin...]
set -euo pipefail
cd "$(dirname "$0")"

TARGET="x86_64-pc-windows-msvc"
PKG="editor_app"
BIN="rme-rs"
TOOLS_DIR="$(pwd)/.tools"
DIST_DIR="$(pwd)/dist"

# 1. garante o target MSVC (no-op se o rust-toolchain.toml já o instalou)
rustup target add "$TARGET"

# 2. cargo-xwin — local ao projeto, sem sujar o cargo global
export PATH="$TOOLS_DIR/bin:$PATH"
if ! command -v cargo-xwin >/dev/null 2>&1; then
    echo "Instalando cargo-xwin em $TOOLS_DIR (local ao projeto)..."
    cargo install cargo-xwin --root "$TOOLS_DIR" --locked
fi

# 3. build
echo "Cross-compilando $PKG para $TARGET..."
cargo xwin build --release -p "$PKG" --target "$TARGET" "$@"

# 4. copia o .exe final pra dist/
mkdir -p "$DIST_DIR"
cp "target/$TARGET/release/$BIN.exe" "$DIST_DIR/"
echo "OK: $DIST_DIR/$BIN.exe"