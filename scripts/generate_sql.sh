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
UPDATE_FILE="${GAME_DIR}"/update-*.sql

rebuild_from_scratch() {
  echo "PRAGMA defer_foreign_keys = on;" > "${OUTFILE}"
  cat sql/{drop_tables,create_tables,create_indexes}.sql >> "${OUTFILE}"
  cat "${GAME_DIR}"/{bundles,dirs,files,version}.sql >> "${OUTFILE}"
}

if [[ "${FORCE_REBUILD}" == "true" ]]; then
  echo "Forced rebuild for ${GAME} via workflow_dispatch input"
  rebuild_from_scratch
elif [[ -f "$UPDATE_FILE" ]]; then
  echo "Using update script for ${GAME}: ${UPDATE_FILE}"
  cp "${UPDATE_FILE}" "${OUTFILE}"
else
  echo "Rebuilding ${GAME} database from scratch"
  rebuild_from_scratch
fi
