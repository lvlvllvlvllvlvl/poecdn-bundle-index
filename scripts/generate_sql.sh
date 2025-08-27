#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/generate_sql.sh poe1|poe2 <event_name> <force_rebuild:true|false> <use_update:true|false> <update_file-or-empty>
GAME=${1:-}
EVENT_NAME=${2:-}
FORCE_REBUILD=${3:-false}
USE_UPDATE=${4:-false}
UPDATE_FILE=${5:-}

if [[ -z "${GAME}" || ("${GAME}" != "poe1" && "${GAME}" != "poe2") ]]; then
  echo "Usage: $0 poe1|poe2 <event_name> <force_rebuild:true|false> <use_update:true|false> <update_file-or-empty>" >&2
  exit 2
fi

OUTFILE="${GAME}.sql"
GAME_DIR="output/${GAME}"

rebuild_from_scratch() {
  echo "PRAGMA defer_foreign_keys = off;" > "${OUTFILE}"
  cat sql/drop_tables.sql sql/create_tables.sql sql/create_indexes.sql >> "${OUTFILE}"
  cat "${GAME_DIR}"/*.sql >> "${OUTFILE}"
}

if [[ "${EVENT_NAME}" == "workflow_dispatch" && "${FORCE_REBUILD}" == "true" ]]; then
  echo "Forced rebuild for ${GAME} via workflow_dispatch input"
  rebuild_from_scratch
elif [[ "${USE_UPDATE}" == "true" && -n "${UPDATE_FILE}" ]]; then
  echo "Using update script for ${GAME}: ${UPDATE_FILE}"
  cp "${UPDATE_FILE}" "${OUTFILE}"
else
  echo "Rebuilding ${GAME} database from scratch"
  rebuild_from_scratch
fi
