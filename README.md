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

For `just build` you need the unpacked Innioasis Y2 stock firmware v3.2.0 in `y2_v3.2.0_FM-20260813`
next to the justfile (or set `Y2_STOCK` to its folder) and an SSH public key at
`~/.ssh/y2linux_ed25519.pub` (or set `Y2_PUBLIC_KEY`).

For `just flash` the user must be in the `dialout` group.

## Start

```sh
git clone --recurse-submodules <this repo> Y2Build
cd Y2Build
just setup
just check
just build
```

The first run builds the container, which takes a few minutes.

## Commands

| Command | What it does |
| --- | --- |
| `just setup` | Check out the submodules on their branches and add the upstream remotes |
| `just check` | Run Reborn's `cargo fmt --check`, clippy and tests, then the Y2Linux platform gate |
| `just build` | Build the first-install package into `.build/out/y2-first-install`, from the working trees as they are (uncommitted changes included). Only what changed is built again |
| `just build --public` | The same without aptX and without your SSH key |
| `just build --fast` | The same with Reborn and the platform built without LTO and without the release checks: quicker, for trying out, not for release |
| `just build boot` | Build only `BOOTIMG` and `RECOVERY` |
| `just build --explain` | Show what would be built and why, without building |
| `just build --clean` | Build everything again (downloads and compiler caches are kept) |
| `just push` | Build Reborn quickly (no LTO), copy it to the player over SSH and restart it |
| `just push reborn --release` | The same with Reborn as the image builds it |
| `just push platform` | Copy the platform (every `y2-platform` command and daemon) to the player and restart it |
| `just push system` | Install the whole build (kernel, root filesystem, recovery) through the player's own updater, with its backup and automatic way back, then wait for the player to restart |
| `just flash` | Flash BOOTIMG and ANDROID, keeping the player's data |
| `just flash erase` | First install: flash BOOTIMG, ANDROID and USRDATA (**erases** the player's data) |
| `just shell` | Open a shell in the build container |
| `just upstream` | Merge SchulzCode's `main` into both forks |

Add `--dry-run` to `just flash` to see what would be written. `just push` uses `Y2_PLAYER`
(default `root@10.42.0.1`) and `Y2_SSH_KEY` (default `~/.ssh/y2linux_ed25519`). Build logs are in
`.build/out/logs/`.
