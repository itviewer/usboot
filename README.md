## Universal USB Boot utilities for embedded platforms

Cross-platform USB boot tool for Amlogic/Rockchip SoCs — pure Rust, single binary, no libusb dependency.

## Installation

Download from [Releases](https://github.com/itviewer/usboot/releases) or `cargo install usboot`

On Windows, you should use [Zadig](https://zadig.akeo.ie/) or [libwdi](https://github.com/pbatard/libwdi) to manually install the WinUSB driver for a device

## boot-g12

A Rust implementation of [pyamlboot](https://github.com/superna9999/pyamlboot) boot-g12.py

```bash
Load U-Boot binary onto an Amlogic G12 SoC over USB in boot mode

Usage: boot-g12 [OPTIONS] <BINARY>

Arguments:
  <BINARY>  Binary to load

Options:
      --timeout <TIMEOUT>  Timeout in seconds for the device to enumerate [default: 5]
  -h, --help               Print help
  -V, --version            Print version
```

## boot-gx

A Rust implementation of [pyamlboot](https://github.com/superna9999/pyamlboot) boot.py

```bash
Load U-Boot binary onto an Amlogic GX/AXG SoC over USB in boot mode

Usage: boot-gx [OPTIONS] <BOARD>

Arguments:
  <BOARD>  Board type to boot on

Options:
      --board-files <BOARD_FILES>  Path to board-specific files directory [default: .]
      --image <IMAGE>              Image file to load
      --fdt <FDT>                  Device tree binary file to load
      --script <SCRIPT>            U-Boot script file to load
      --ramfs <RAMFS>              RamFS/initramfs file to load
      --timeout <TIMEOUT>          Timeout in seconds for the device to enumerate [default: 5]
  -h, --help                       Print help
  -V, --version                    Print version
```

