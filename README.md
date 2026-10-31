# AI slop ahead

Large parts of this project were build using LLMs. 
If you are not fond of this kind of work, you might want to move on now.

# Y2Build

Builds [Y2Linux](https://github.com/mrtngamper/Y2Linux) and
[Y2Reborn](https://github.com/mrtngamper/Y2Reborn) in a Docker container and flashes the result
with [Y2Flash](https://github.com/mrtngamper/Y2Flash).

Y2Linux and Y2Reborn are forks of [Y2Linux](https://github.com/SchulzCode/Y2Linux) and
[Y2Reborn](https://github.com/SchulzCode/Y2Reborn) by [SchulzCode](https://github.com/SchulzCode).
Check out the original work!

## Requirements

- Linux (x86_64)
- just
- Docker (usable without sudo)
- git

For `just package` you need the unpacked Innioasis Y2 stock firmware v3.2.0 (`y2_v3.2.0_FM-20260813`)
and an SSH public key at `~/.ssh/y2linux_ed25519.pub` (or set `Y2_PUBLIC_KEY`)

For `just flash` the user must be in the `dialout` group.

## Start

```sh
git clone --recurse-submodules <this repo> Y2Build
cd Y2Build
just setup
just check
```

The first run builds the container, which takes a few minutes.

## Commands

| Command | What it does |
| --- | --- |
| `just check` | `cargo fmt --check`, clippy and `cargo test` in Reborn, then the Y2Linux platform gate |
| `just fmt` | `cargo fmt --all` in Reborn |
| `just image [--public]` | Build the system image into `Y2Linux/out/y2-build` |
| `just resume [stage]` | Rebuild from `kernel`, `buildroot` (default) or `artifacts` |
| `just package <stock folder> [--public]` | Make the first-install package in `Y2Linux/out/y2-first-install` |
| `just flash erase` | First install: flash BOOTIMG, ANDROID and USRDATA (**erases** the player's data) |
| `just flash` | Update: flash BOOTIMG and ANDROID, keep the player's data |
| `just shell` | Open a shell in the container |
| `just status` | Show each submodule's branch and changes |
| `just upstream` | Merge SchulzCode's `main` into both forks |
| `just pin` | Stage the submodules' current commits in Y2Build |

The image is built from committed code only: commit in the fork, then `just resume`.

Add `--dry-run` to `just flash` to see what would be written.
