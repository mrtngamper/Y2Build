set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments
set quiet

repos := "Y2Linux Y2Reborn"
modules := repos + " Y2Flash"
image := "y2build:" + replace_regex(sha256_file("container/Dockerfile"), '^(.{12}).*$', '$1')
out := "out/y2-build"
package_out := "out/y2-first-install"
key := env("Y2_PUBLIC_KEY", home_directory() / ".ssh/y2linux_ed25519.pub")

# list the recipes
default:
    just --list --unsorted

# check out the submodules and their branches, add the upstream remotes, enable rerere
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Updating submodules to the pinned commits..."
    git submodule update --init --quiet
    for r in {{modules}}; do
        echo
        echo "$r"
        b=$(git config -f .gitmodules submodule.$r.branch || echo main)
        commit=$(git -C $r rev-parse --short HEAD)
        if [ "$(git -C $r rev-parse HEAD)" = "$(git -C $r rev-parse -q --verify origin/$b)" ]; then
            git -C $r checkout -q -B $b origin/$b
            git -C $r branch -q -u origin/$b $b
            echo "  branch:   $b at $commit"
        else
            echo "  branch:   none, $commit is not the tip of origin/$b (detached)"
        fi
        echo "  origin:   $(git -C $r remote get-url origin | sed -E 's#^git@([^:]+):#https://\1/#; s#\.git$##')"
        case " {{repos}} " in *" $r "*) ;; *) continue ;; esac
        git -C $r remote get-url upstream >/dev/null 2>&1 ||
            git -C $r remote add upstream https://github.com/SchulzCode/$r.git
        git -C $r config rerere.enabled true
        git -C $r config rerere.autoupdate true
        echo "  rerere:   on"
    done
    echo
    echo "Done."

# build the container image
container:
    docker build --build-arg UID=$(id -u) --build-arg GID=$(id -g) -t {{image}} container

# open a shell in the container
shell:
    just _in Y2Linux bash

# format the Reborn code
fmt:
    just _in Y2Reborn cargo fmt --all

# run all checks before a commit (check-reborn, then gate)
check: check-reborn gate

# check Reborn: formatting, clippy, tests
check-reborn:
    just _in Y2Reborn sh -c 'cargo fmt --all --check \
        && cargo clippy --workspace --all-targets --locked --offline -- -D warnings \
        && cargo test --workspace --locked --offline'

# run the Y2Linux platform gate (includes Reborn's contract test)
gate:
    just _in Y2Linux env LD_LIBRARY_PATH=/opt/zlib-ng/lib sh tools/development/platform-gate.sh

# build the system image from the committed code (--public: public variant)
image *args:
    just _in Y2Linux sh {{justfile_directory()}}/container/image.sh --output {{out}} "$@"

# rebuild from one stage: kernel, buildroot (default) or artifacts
resume stage="buildroot" *args:
    just _in Y2Linux sh {{justfile_directory()}}/container/image.sh --output {{out}} --resume "$@"

# make the first-install package from the stock firmware folder
package stock *args:
    STOCK="$(realpath "$1")" just _in Y2Linux python3 tools/production/package.py --build {{out}} \
        --output {{package_out}} --stock "$(realpath "$1")" --public-key {{key}} "${@:2}"

# flash the package and keep the player's data; `just flash erase` for the first install
flash *args:
    #!/usr/bin/env bash
    set -euo pipefail
    case "${1:-}" in
        erase) scatter=MT6582_Android_scatter.txt; shift ;;
        ""|-*) scatter=MT6582_preserve_data_scatter.txt ;;
        *) echo "flash: unknown mode '$1': just flash [erase] [y2flash options, e.g. --dry-run]" >&2; exit 2 ;;
    esac
    root={{justfile_directory()}}
    package=$root/Y2Linux/{{package_out}}
    [ -f "$package/$scatter" ] || { echo "flash: no package in $package: just package <stock folder>" >&2; exit 1; }
    echo "Building y2flash..."
    just _in Y2Flash sh -c 'CARGO_HOME="$PWD/target/cargo-home" cargo build --release --locked --quiet'
    echo "Flashing $package with $scatter"
    exec "$root/Y2Flash/target/release/y2flash" --loaders "$root/Y2Flash/loaders" --scatter "$scatter" \
        flash "$package" "$@"

# update the Alpine lock after the mirror dropped a package
refresh-alpine-lock:
    just _in Y2Linux python3 {{justfile_directory()}}/container/refresh-alpine-lock.py
    git -C Y2Linux diff --stat tools/build/inputs.lock.json

# show each submodule's branch and changes
status:
    git submodule status
    for r in {{modules}}; do echo "== $r"; git -C $r status -sb | head -20; done

# merge SchulzCode's main into both forks
upstream:
    for r in {{repos}}; do \
        git -C $r symbolic-ref -q HEAD >/dev/null || { echo "$r: detached HEAD, check out a branch" >&2; exit 1; }; \
        git -C $r fetch -q upstream --tags; \
        git -C $r merge --no-edit upstream/main || { echo "$r: resolve, commit, run again" >&2; exit 1; }; \
    done
    echo "merged: just check, push the forks, then just pin"

# stage the submodules' current commits in Y2Build (they must be pushed)
pin:
    for r in {{modules}}; do \
        [ -z "$(git -C $r status --porcelain)" ] || { echo "$r has uncommitted changes" >&2; exit 1; }; \
        git -C $r fetch -q origin; \
        git -C $r branch -r --contains HEAD | grep -q 'origin/' || \
            { echo "$r: HEAD is not pushed, a pin to it could not be checked out elsewhere" >&2; exit 1; }; \
    done
    git add {{modules}}
    git diff --cached --submodule=log {{modules}}; echo "staged: commit Y2Build to record the pins"

# run a command in the container: just _in <Y2Linux|Y2Reborn|Y2Flash> <command...>
[private]
_in dir *cmd:
    #!/usr/bin/env bash
    set -euo pipefail
    root={{justfile_directory()}}
    for r in {{modules}}; do
        [ -e "$root/$r/.git" ] || { echo "$root/$r is not checked out: just setup" >&2; exit 2; }
    done
    docker image inspect {{image}} >/dev/null 2>&1 || just container
    opts=(--rm --init --security-opt seccomp=unconfined --security-opt label=disable
          --security-opt systempaths=unconfined)
    [ -d /sys/kernel/security/apparmor ] && opts+=(--security-opt apparmor=unconfined)
    [ -t 0 ] && [ -t 1 ] && opts+=(-it) || opts+=(-i)
    opts+=(--tmpfs /tmp:rw,exec,nosuid,nodev)
    opts+=(-v "$root:$root" -w "$root/$1" -e STOCK="${STOCK:-}")
    [ -f "{{key}}" ] && opts+=(-v "{{key}}:{{key}}:ro")
    [ -n "${STOCK:-}" ] && opts+=(-v "$STOCK:$STOCK:ro")
    [ -d "$HOME/.local/share/y2linux" ] && opts+=(-v "$HOME/.local/share/y2linux:/home/builder/.local/share/y2linux:ro")
    shift
    exec docker run "${opts[@]}" {{image}} "$@"
