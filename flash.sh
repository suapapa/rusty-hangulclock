#!/bin/bash

export NO=32
export HW_REVISION=4
export SW_VERSION=27

# Default flash (to inactive partition)
make -f Makefile_homin flash RUSTY_HANGULCLOCK_HW_REVISION=${HW_REVISION} RUSTY_HANGULCLOCK_SW_VERSION=${SW_VERSION} RUSTY_HANGULCLOCK_NO=${NO}

# Uncomment one of the following to force flash to specific partition:
#make -f Makefile_homin flash_ota0 RUSTY_HANGULCLOCK_HW_REVISION=${HW_REVISION} RUSTY_HANGULCLOCK_SW_VERSION=${SW_VERSION} RUSTY_HANGULCLOCK_NO=${NO}
# make -f Makefile_homin flash_ota1 RUSTY_HANGULCLOCK_HW_REVISION=${HW_REVISION} RUSTY_HANGULCLOCK_SW_VERSION=${SW_VERSION} RUSTY_HANGULCLOCK_NO=${NO}
