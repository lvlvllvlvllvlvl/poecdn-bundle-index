#!/usr/bin/env bash
set -euo pipefail

cargo run --release -- "$1"
scripts/check_update.sh "$1" https://ggpk.exposed
scripts/generate_sql.sh "$1" true
npx wrangler d1 execute "$1"-files --remote --file="$1".sql
