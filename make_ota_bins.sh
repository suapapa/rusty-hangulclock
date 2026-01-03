#!/bin/bash

# Defaults
# NO=26
# HW_REVISION=4
SW_VERSION=""
TARGET="ota_bin"

while getopts "n:r:v:a:o:" opt; do
  case $opt in
    v) SW_VERSION="$OPTARG" ;;
    \?) echo "Usage: $0 [-v SW_VERSION]"; exit 1 ;;
  esac
done

if [ -z "$SW_VERSION" ]; then
  echo "Error: -v option is required"
  exit 1
fi

export SW_VERSION

for rev in 3 4; do
  echo "Building for HW Revision $rev..."
  make ${TARGET} \
    RUSTY_HANGULCLOCK_TOKEN=${HOMIN_DEV_TOKEN} \
    RUSTY_HANGULCLOCK_HW_REVISION=$rev \
    RUSTY_HANGULCLOCK_SW_VERSION=${SW_VERSION}
done
