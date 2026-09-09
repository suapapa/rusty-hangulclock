#!/bin/bash

VERSION=""
FW_NAME=""
HW_REVISION=""

while getopts "v:f:r:" opt; do
  case $opt in
    v) VERSION="$OPTARG" ;;
    f) FW_NAME="$OPTARG" ;;
    r) HW_REVISION="$OPTARG" ;;
    *) echo "Usage: $0 -v <version> -f <filename> -r <revision>"
       exit 1 ;;
  esac
done

if [ -z "$VERSION" ] || [ -z "$FW_NAME" ] || [ -z "$HW_REVISION" ]; then
  echo "Error: Missing required arguments."
  echo "Usage: $0 -v <version> -f <filename> -r <revision>"
  exit 1
fi

if [ "$HW_REVISION" != "3" ] && [ "$HW_REVISION" != "4" ]; then
  echo "Error: -r must be 3 or 4."
  exit 1
fi

FW_NAME=$(basename "${FW_NAME}")
DOWNLOAD_URL_BASE="${DOWNLOAD_URL_BASE:-https://asset.homin.dev/rusty-hangulclock_fw/}"
# Ensure trailing slash
case "${DOWNLOAD_URL_BASE}" in
  */) ;;
  *) DOWNLOAD_URL_BASE="${DOWNLOAD_URL_BASE}/" ;;
esac
DOWNLOAD_URL="${DOWNLOAD_URL_BASE}${FW_NAME}"
RELEASE_NOTES="SW version $VERSION for HW revision $HW_REVISION"

API_URL="${OTA_API_URL:-https://hangulclock.homin.dev/v1/update}"
TOKEN="${HOMIN_DEV_TOKEN:-}"

if [ -z "$TOKEN" ]; then
  echo "Error: HOMIN_DEV_TOKEN is not set"
  exit 1
fi

curl -fsS -X POST "$API_URL" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d "{
    \"version\": $VERSION,
    \"download_url\": \"$DOWNLOAD_URL\",
    \"release_notes\": \"$RELEASE_NOTES\",
    \"hw_revision\": $HW_REVISION
  }"
echo
