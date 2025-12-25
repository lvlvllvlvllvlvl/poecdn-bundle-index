#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/generate_sql.sh poe1|poe2 <force_rebuild:true|false>
GAME=${1:-}
FORCE_REBUILD=${2:-false}

if [[ -z "${GAME}" || ("${GAME}" != "poe1" && "${GAME}" != "poe2") ]]; then
  echo "Usage: $0 poe1|poe2 <event_name> <force_rebuild:true|false> <use_update:true|false> <update_file-or-empty>" >&2
  exit 2
fi

OUTFILE="${GAME}.sql"
GAME_DIR="output/${GAME}"
UPDATE_FILE=("${GAME_DIR}"/update-*.sql)

ls -lah "$GAME_DIR"

rebuild_from_scratch() {
  {
    echo "PRAGMA defer_foreign_keys = on;"
    cat sql/{drop,create}_tables.sql \
      "${GAME_DIR}"/{bundles,dirs,files,version}.sql \
      sql/create_indexes.sql
  } > "${OUTFILE}"
  ls -lah "${OUTFILE}"
}

if [[ "${FORCE_REBUILD}" == "true" ]]; then
  echo "Forced rebuild for ${GAME} via workflow_dispatch input"
  rebuild_from_scratch
elif [[ -f "${GAME_DIR}"/update-not-required.sql ]]; then
  echo update not required
elif [[ -f "${UPDATE_FILE[0]}" ]]; then
  echo "Using update script for ${GAME}"
  cat "${UPDATE_FILE[@]}" > "${OUTFILE}"
  ls -lah "${OUTFILE}"
else
  echo "Update file ${UPDATE_FILE[*]} not found. Rebuilding ${GAME} database from scratch"
  rebuild_from_scratch
fi
