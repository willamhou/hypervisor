#!/bin/bash
set -euo pipefail

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export PATH="$CARGO_HOME/bin:$PATH"

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_FILE="$REPO_DIR/bl33full.log"

cd "$REPO_DIR"
timeout 120 make run-tfa-linux-ffa > "$LOG_FILE" 2>&1
echo "exitcode=$?" >> "$LOG_FILE"
