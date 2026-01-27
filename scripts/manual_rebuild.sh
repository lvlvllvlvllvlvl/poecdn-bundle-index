#!/usr/bin/env bash
set -euo pipefail

echo "usage: '$0 poe1 local' (substitute poe2 or remote as desired)"

cd "$(dirname "$0")"/..

cargo run --release -- "$1"
[[ "$2" == remote ]] && scripts/check_update.sh "$1" https://ggpk.exposed
scripts/generate_sql.sh "$1" true
npx wrangler d1 execute "$1"-files --"$2" --file="$1".sql
