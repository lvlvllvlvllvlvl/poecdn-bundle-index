#!/usr/bin/env bash
set -euo pipefail

GAME=${1:-}
SERVER=${2:-}
if [[ -z "${GAME}" || ("${GAME}" != "poe1" && "${GAME}" != "poe2") ]]; then
  echo "Usage: $0 poe1|poe2 <server url>" >&2
  exit 2
fi

DIR="output/${GAME}"
mkdir -p "$DIR"

# Determine poe number and base URLs per game
if [[ "$GAME" == "poe1" ]]; then
  POE_NUM=1
else
  POE_NUM=2
fi
BASE_URL="https://lvlvllvlvllvlvl.github.io/poecdn-bundle-index/${GAME}"

# Query server for current CDN version URL and fetch known URLs list
SERVER_VERSION=$(curl -fsSL "${SERVER}/version?poe=${POE_NUM}") # e.g. https://patch.poecdn.com/3.26.0.12/
CURR_VERSION=$(cat "$DIR/urls.json")
PREV_VERSION=$(curl -fsSL "${BASE_URL}/urls.json")



if [[ "$CURR_VERSION" == *"$SERVER_VERSION"* ]]; then
  echo "Still on $SERVER_VERSION, no changes"
  touch "$DIR/update-not-required.sql"
elif [[ "$PREV_VERSION" == *"$SERVER_VERSION"* ]]; then
  echo "Known version detected for ${GAME}: ${SERVER_VERSION}"
  curl -fsSL "${BASE_URL}/bundle_index.sqlite" -o "$DIR/current_index.sqlite"
  cp "$DIR/current_index.sqlite" "$DIR/previous_bundle_index.sqlite"
  # Generate differential update SQL next to the current database
  # shellcheck disable=SC1009
  if cargo run --release -- diff-update \
    --previous "$DIR/previous_bundle_index.sqlite" \
    --current "$DIR/bundle_index.sqlite"
  then
      for UPDATE_FILE in "$DIR"/update-*.sql
      do
        echo "Applying update: ${UPDATE_FILE}"
        sqlite3 "$DIR/bundle_index.sqlite" ".read ${UPDATE_FILE}"
      done

      if sqlite3 "$DIR/bundle_index.sqlite" ".read scripts/validate.sql"; then
        echo "validation success"
      else
        echo "validation failed; removing generated SQL to perform full rebuild" >&2
        rm -f "$DIR"/update-*.sql
      fi
  else
    echo "diff failed, perform full rebuild"
  fi
else
  echo "No known version match for ${GAME} (server: ${SERVER_VERSION}). Skipping diff-update."
fi
