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

#TODO call diff-update command
