# Rusty HangulClock

![rusty_hangulclock](./_asset/rust-hangulclock.jpg)

A HangulClock written in Rust for the ESP32-C3 board.

- [Usage manual](https://hangulclock.homin.dev)
- [Pannel image mager - Web](https://hangulclock.homin.dev/panel-maker)

## Hardware

- [Schematic & PCB artwork](./sch/rusty-hangulclock/) - KiCad project files
- [3D case models](./case/)

## Build and Flash

### Install Toolchain (One-time setup)

```sh
cargo install espup cargo-espflash ldproxy
espup install
```

### Build and Flash

```sh
make flash
```

### Factory Reset

Reset all settings to defaults:

```sh
make erase_nvs
```

## Pre-commit Workflow

Before committing code, run:

```sh
make pre-commit
```

This will run all static analysis tools and ensure your code meets quality standards.