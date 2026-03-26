#!/bin/bash

# Defaults
NO=26
HW_REVISION=4
SW_VERSION=33
TARGET="flash"
OWNER=""

while getopts "n:r:v:a:o:" opt; do
  case $opt in
    n) NO="$OPTARG" ;;
    r)
      case "$OPTARG" in
        3|4) HW_REVISION="$OPTARG" ;;
        *) echo "Error: Invalid -r value '$OPTARG'. Use 3 or 4."; exit 1 ;;
      esac
      ;;
    v) SW_VERSION="$OPTARG" ;;
    a)
      case "$OPTARG" in
        0) TARGET="flash_ota0" ;;
        1) TARGET="flash_ota1" ;;
        *) echo "Error: Invalid -a value '$OPTARG'. Use 0 or 1."; exit 1 ;;
      esac
      ;;
    o) OWNER="$OPTARG" ;;
    \?) echo "Usage: $0 [-n NO] [-r HW_REVISION] [-v SW_VERSION] [-a 0|1] [-o OWNER]"; exit 1 ;;
  esac
done

export NO
export HW_REVISION
export SW_VERSION
export OWNER

echo "Flashing with: NO=${NO}, HW_REVISION=${HW_REVISION}, SW_VERSION=${SW_VERSION}, TARGET=${TARGET}, OWNER='${OWNER}'"

# Execute make command
make ${TARGET} \
  RUSTY_HANGULCLOCK_TOKEN=${HOMIN_DEV_TOKEN} \
  RUSTY_HANGULCLOCK_HW_REVISION=${HW_REVISION} \
  RUSTY_HANGULCLOCK_SW_VERSION=${SW_VERSION} \
  RUSTY_HANGULCLOCK_NO=${NO} \
  RUSTY_HANGULCLOCK_OWNER=${OWNER}
