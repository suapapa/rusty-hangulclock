export RUSTY_HANGULCLOCK_NO=
export RUSTY_HANGULCLOCK_SW_VERSION=7
export RUSTY_HANGULCLOCK_HW_REVISION=4
export RUSTY_HANGULCLOCK_TOKEN=your_token_here

.PHONY: flash_dotstar flash build ota_bin erase_nvs

flash_dotstar:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features dotstar,tr_to_left --release --partition-table part.csv -M

flash:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features neopixel,tr_to_left --release --partition-table part.csv -M

build:
	source ~/export-esp.sh
	cargo build --no-default-features --features neopixel,tr_to_left --release

ota_bin: build
	mkdir -p release
	cargo espflash save-image --chip esp32c3 --release --partition-table part.csv release/rusty-hangulclock_rev${RUSTY_HANGULCLOCK_HW_REVISION}_$(RUSTY_HANGULCLOCK_SW_VERSION)_$(shell date +%Y%m%d_%H%M%S).bin

erase_nvs:
	source ~/export-esp.sh
	cargo espflash erase-parts --partition-table part.csv nvs user_nvs

monitor:
	cargo espflash monitor --chip esp32c3

clean:
	rm -rf release
	cargo clean
