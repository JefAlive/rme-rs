#!/usr/bin/env bash
# Dump de todo o código-fonte do projeto num único .txt, para revisão externa.
set -euo pipefail
cd "$(dirname "$0")"

OUT="${1:-/tmp/rme-rs-dump.txt}"

find . -type f \( \
    -name "*.rs" -o -name "*.toml" -o -name "*.wgsl" -o -name "*.sh" -o -name "*.md" \
  \) \
  -not -path "*/target/*" \
  -not -path "*/.git/*" \
  -not -path "*/dist/*" \
  -not -path "*/.tools/*" \
  -not -name "Cargo.lock" \
  | sort \
  | while read -r f; do
      echo "// $f"
      cat "$f"
      printf '\n\n'
    done > "$OUT"

echo "OK: $OUT ($(wc -l < "$OUT") linhas, $(du -h "$OUT" | cut -f1))"