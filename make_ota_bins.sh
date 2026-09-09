#!/bin/bash

# Defaults
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SW_VERSION=""
HW_REVISIONS="3 4"
TARGET="ota_bin"

if [ -f "${SCRIPT_DIR}/FWVER" ]; then
  SW_VERSION="$(tr -d '[:space:]' < "${SCRIPT_DIR}/FWVER")"
fi

while getopts "v:" opt; do
  case $opt in
    v) SW_VERSION="$OPTARG" ;;
    \?) echo "Usage: $0 [-v SW_VERSION]"; exit 1 ;;
  esac
done

if [ -z "$SW_VERSION" ]; then
  echo "Error: SW_VERSION is empty (set FWVER or pass -v)"
  exit 1
fi

export SW_VERSION

for rev in ${HW_REVISIONS}; do
  echo "Building for HW Revision $rev (SW_VERSION=${SW_VERSION})..."
  make ${TARGET} \
    RUSTY_HANGULCLOCK_TOKEN=${HOMIN_DEV_TOKEN} \
    RUSTY_HANGULCLOCK_HW_REVISION=$rev \
    RUSTY_HANGULCLOCK_SW_VERSION=${SW_VERSION}
done
