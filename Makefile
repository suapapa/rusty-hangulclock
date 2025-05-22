RUSTY_HANGUL_CLOCK_SERIAL = dev

.PHONY: flash_dotstar flash_neopixel erase_nvs

flash_neopixel:
    # @echo "Neopixel is experimental"
	source ~/export-esp.sh
	cargo espflash flash --release -T part.csv -M

flash_dotstar:
    # @echo "Flashing for DotStar..."
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features dotstar,tr_to_left --release -T part.csv -M

erase_nvs:
    # @echo "Erasing NVS..."
	source ~/export-esp.sh
	cargo espflash erase-parts --partition-table part.csv nvs user_nvs