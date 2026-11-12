#!/usr/bin/env bash
set -euo pipefail
READER_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$READER_ROOT"
# Local Rust installed during development; a normal global installation wins.
if ! command -v cargo >/dev/null 2>&1 && [[ -x "$READER_ROOT/../work/runtime/cargo/bin/cargo" ]]; then
  export CARGO_HOME="$READER_ROOT/../work/runtime/cargo"
  export RUSTUP_HOME="$READER_ROOT/../work/runtime/rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
if [[ ! -d frontend/node_modules ]]; then (cd frontend && npm ci); fi
(cd frontend && npm run build)
exec cargo run --example workspace_server -j 4 -- "${1:-$READER_ROOT/../work/reader-data}" "${2:-1122}"
