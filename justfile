set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments
set quiet

repos := "Y2Linux Y2Reborn"
modules := repos + " Y2Flash"
image := "y2build:" + replace_regex(sha256_file("container/Dockerfile"), '^(.{12}).*$', '$1')
state := justfile_directory() / ".build"
package := state / "out/y2-first-install"
key := env("Y2_PUBLIC_KEY", home_directory() / ".ssh/y2linux_ed25519.pub")
ssh_key := env("Y2_SSH_KEY", home_directory() / ".ssh/y2linux_ed25519")
stock := env("Y2_STOCK", justfile_directory() / "y2_v3.2.0_FM-20260813")
player := env("Y2_PLAYER", "root@10.42.0.1")
signing := home_directory() / ".local/share/y2build/signing"

# list the recipes
default:
    just --list --unsorted

# check out the submodules and their branches, add the upstream remotes, enable rerere
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    git submodule update --init --quiet
    for r in {{modules}}; do
        b=$(git config -f .gitmodules submodule.$r.branch || echo main)
        if [ "$(git -C $r rev-parse HEAD)" = "$(git -C $r rev-parse -q --verify origin/$b)" ]; then
            git -C $r checkout -q -B $b origin/$b && git -C $r branch -q -u origin/$b $b
            echo "$r: on $b"
        else
            echo "$r: detached, $(git -C $r rev-parse --short HEAD) is not the tip of origin/$b"
        fi
        case " {{repos}} " in *" $r "*) ;; *) continue ;; esac
        git -C $r remote get-url upstream >/dev/null 2>&1 || git -C $r remote add upstream https://github.com/SchulzCode/$r.git
        git -C $r config rerere.enabled true && git -C $r config rerere.autoupdate true
    done

# build the container image
container:
    docker build --build-arg UID=$(id -u) --build-arg GID=$(id -g) -t {{image}} container

# open a shell in the container
shell:
    just _run bash

# run all checks before a commit: Reborn's formatting, clippy and tests, then the Y2Linux platform gate
check:
    just _run y2b check

# build [steps] [--public] [--fast] [--explain] [--clean] [--force step]: the package by default (build/src/steps)
build *args:
    just _run y2b build "$@"

# push [reborn [--release] | platform | system] to the player: Reborn by default, system installs everything
push *args:
    just _run y2b push "$@"

# flash the package and keep the player's data, or erase it with `just flash erase` for the first install
flash *args:
    #!/usr/bin/env bash
    set -euo pipefail
    scatter=MT6582_preserve_data_scatter.txt
    [ "${1:-}" != erase ] || { scatter=MT6582_Android_scatter.txt; shift; }
    [ -f "{{package}}/$scatter" ] || { echo "flash: no package in {{package}}: just build" >&2; exit 1; }
    just _run sh -c 'cd /y2/Y2Build/Y2Flash && CARGO_HOME=target/cargo-home cargo build --release --locked --quiet'
    exec Y2Flash/target/release/y2flash --loaders Y2Flash/loaders --scatter "$scatter" flash "{{package}}" "$@"

# merge SchulzCode's main into both forks
upstream:
    for r in {{repos}}; do \
        git -C $r symbolic-ref -q HEAD >/dev/null || { echo "$r: detached HEAD, check out a branch" >&2; exit 1; }; \
        git -C $r fetch -q upstream --tags; \
        git -C $r merge --no-edit upstream/main || { echo "$r: resolve, commit, run again" >&2; exit 1; }; \
    done
    echo "merged: just check, push the forks, then commit the new submodule commits in Y2Build"

# run a command in the build container, with y2b (build/) compiled and on the PATH; fixed paths under
# /y2 and state in .build/ keep every cache valid, the security options let the kernel build's
# bubblewrap sandbox run, the host's network reaches the player
[private]
_run *cmd:
    #!/usr/bin/env bash
    set -euo pipefail
    for r in {{modules}}; do
        [ -e "$r/.git" ] || { echo "$r is not checked out: just setup" >&2; exit 2; }
    done
    docker image inspect {{image}} >/dev/null 2>&1 || just container
    s={{state}}
    mkdir -p "$s"/src/Y2Linux/out "$s"/src/Y2Linux/.cache "$s"/src/Y2Reborn "$s"/out "$s"/cache "$s"/state
    mkdir -p -m 700 "{{signing}}"
    opts=(--rm --init --network host -w /y2/Y2Build --tmpfs /tmp:rw,exec,nosuid,nodev
          --security-opt seccomp=unconfined --security-opt label=disable --security-opt systempaths=unconfined
          -v "$PWD:/y2/Y2Build" -v "$s/state:/y2/state" -v "{{signing}}:/signing"
          -v "$s/src/Y2Linux:/y2/Y2Linux" -v "$s/src/Y2Reborn:/y2/Y2Reborn"
          -v "$s/out:/y2/Y2Linux/out" -v "$s/cache:/y2/Y2Linux/.cache"
          -e RUSTC_WRAPPER=sccache -e SCCACHE_DIR=/y2/Y2Linux/.cache/sccache -e SCCACHE_CACHE_SIZE=20G
          -e Y2_PLAYER={{player}} -e PATH=/y2/Y2Linux/.cache/y2b/release:/home/builder/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin)
    [ -d /sys/kernel/security/apparmor ] && opts+=(--security-opt apparmor=unconfined)
    [ -t 0 ] && [ -t 1 ] && opts+=(-it) || opts+=(-i)
    [ -d "{{stock}}" ] && opts+=(-v "{{stock}}:/stock:ro")
    [ -f "{{key}}" ] && opts+=(-v "{{key}}:/owner.pub:ro")
    [ -f "{{ssh_key}}" ] && opts+=(-v "{{ssh_key}}:/ssh/key:ro")
    [ -d "$HOME/.local/share/y2linux" ] && opts+=(-v "$HOME/.local/share/y2linux:/keys:ro")
    exec docker run "${opts[@]}" {{image}} bash -c '
        CARGO_HOME=/y2/Y2Linux/.cache/cargo-home CARGO_TARGET_DIR=/y2/Y2Linux/.cache/y2b \
            cargo build --quiet --release --manifest-path /y2/Y2Build/build/Cargo.toml && exec "$@"' run "$@"
