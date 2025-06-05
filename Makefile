export RUSTY_HANGULCLOCK_NO=0
export RUSTY_HANGULCLOCK_SW_VERSION=0
export RUSTY_HANGULCLOCK_TOKEN=

.PHONY: flash_dotstar flash build ota_bin erase_nvs

flash_dotstar:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features dotstar,tr_to_left --release -T part.csv -M

flash:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features neopixel,tr_to_left --release -T part.csv -M

build:
	source ~/export-esp.sh
	cargo build --no-default-features --features neopixel,tr_to_left --release

ota_bin: build
	mkdir -p release
	cargo espflash save-image --chip esp32c3 --release -T part.csv release/rusty-hangulclock_$(RUSTY_HANGULCLOCK_SW_VERSION)_$(shell date +%Y%m%d_%H%M%S).bin

erase_nvs:
	source ~/export-esp.sh
	cargo espflash erase-parts --partition-table part.csv nvs user_nvs

clean:
	rm -rf release
	cargo clean
