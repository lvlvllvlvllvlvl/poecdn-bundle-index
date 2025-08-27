#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/check_update.sh poe1|poe2 [output_target]
# If output_target is provided, outputs will be appended to that file.
GAME=${1:-}
OUTPUT=${2}
if [[ -z "${GAME}" || ("${GAME}" != "poe1" && "${GAME}" != "poe2") ]]; then
  echo "Usage: $0 poe1|poe2 [output_target]" >&2
  exit 2
fi

DIR="output/${GAME}"

if [[ -f "${DIR}/use_update_script" ]]; then
  update_file=$(ls "${DIR}"/update-*.sql 2>/dev/null || true)
  if [[ -n "${OUTPUT}" ]]; then
    echo "USE_UPDATE=true" >> "$OUTPUT"
    echo "UPDATE_FILE=${update_file}" >> "$OUTPUT"
  fi
  echo "Found update script for ${GAME}: ${update_file}"
else
  if [[ -n "${OUTPUT}" ]]; then
    echo "USE_UPDATE=false" >> "$OUTPUT"
  fi
  echo "No update script found for ${GAME}, will rebuild database"
fi
