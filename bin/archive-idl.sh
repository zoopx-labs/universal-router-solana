#!/usr/bin/env bash
set -euo pipefail

PROG=zoopx_router
OUT_DIR="idl"
IDL="target/idl/${PROG}.json"
PID=$(jq -r '.metadata.address // .address // empty' "${IDL}")
DATE=$(date +%Y%m%d)
DEST="${OUT_DIR}/${PROG}.${PID:-UNKNOWN}.${DATE}.json"

mkdir -p "${OUT_DIR}"
cp "${IDL}" "${DEST}"
SHA=$(sha256sum "${DEST}" | awk '{print $1}')
echo "Archived ${DEST}"
echo "sha256=${SHA}"
