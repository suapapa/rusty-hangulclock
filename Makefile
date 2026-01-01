export RUSTY_HANGULCLOCK_NO=
export RUSTY_HANGULCLOCK_SW_VERSION=
export RUSTY_HANGULCLOCK_HW_REVISION=4
export RUSTY_HANGULCLOCK_TOKEN=

.PHONY: flash_dotstar flash build ota_bin erase_nvs clippy fmt audit deny check-all

flash_dotstar:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features dotstar,tr_to_left --release --partition-table part.csv -M

flash:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features neopixel,tr_to_left --release --partition-table part.csv -M

# Force flash to specific OTA partition
flash_ota0:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features neopixel,tr_to_left --release --partition-table part.csv --partition-table-offset 0xd000 --target-app-partition ota_0 -M

flash_ota1:
	source ~/export-esp.sh
	cargo espflash flash --no-default-features --features neopixel,tr_to_left --release --partition-table part.csv --partition-table-offset 0xd000 --target-app-partition ota_1 -M

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

# Static analysis targets
clippy:
	cargo clippy --target riscv32imc-esp-espidf

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

audit:
	cargo audit

deny:
	cargo deny check

check-all: clippy fmt-check audit deny
	@echo "✅ All static analysis checks passed!"

# Development workflow
dev-check: fmt-check clippy
	@echo "✅ Development checks passed!"

# Pre-commit hook (run this before committing)
pre-commit: check-all
	@echo "✅ Pre-commit checks passed!"
