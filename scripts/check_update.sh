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
KNOWN_URLS=$(curl -fsSL "${BASE_URL}/urls.json")

# If the server version URL is already known, we can compute a differential update
if [[ "$KNOWN_URLS" == *"$SERVER_VERSION"* ]]; then
  echo "Known version detected for ${GAME}: ${SERVER_VERSION}"
  curl -fsSL "${BASE_URL}/bundle_index.sqlite" -o "$DIR/previous_bundle_index.sqlite"
  # Generate differential update SQL next to the current database
  cargo run --release -- diff-update \
    --previous "$DIR/previous_bundle_index.sqlite" \
    --current "$DIR/bundle_index.sqlite"
else
  echo "No known version match for ${GAME} (server: ${SERVER_VERSION}). Skipping diff-update."
fi
