#!/bin/sh
# The image build; `just image` runs it inside the container, from Y2Linux. Arguments go to
# tools/production/build.py (--output <dir> [--public] [--resume <stage>]).
#
# It never rewrites tools/build/inputs.lock.json. When Alpine's mirror has dropped a locked package
# (and it is not in .cache), it stops and asks for `just refresh-alpine-lock`: a deliberate source
# change, reviewed and committed in the fork.
set -eu
output=out/y2linux-platform-v1-build; previous=; resume=
for argument in "$@"; do
    [ "$previous" = --output ] && output=$argument
    [ "$argument" = --resume ] && resume=1
    previous=$argument
done
status=0
python3 "$(dirname "$0")/refresh-alpine-lock.py" --check || status=$?
case $status in
    0) ;;
    3) echo "image: Alpine no longer serves some locked packages; run just refresh-alpine-lock," \
            "review the change in Y2Linux and commit it" >&2; exit 1 ;;
    *) echo "image: checking the Alpine lock failed (exit $status, see above)" >&2; exit 1 ;;
esac
# build.py reads the kernel version from .cache/sources, so a fresh checkout fetches the inputs first.
[ -f .cache/environment/.y2-build-lock ] && [ -f .cache/sources/linux-6.18/Makefile ] ||
    python3 tools/build/prepare.py
# build.py needs a fresh output folder; an empty one left by a run that stopped early is removed.
[ -n "$resume" ] || { [ -d "$output" ] && rmdir "$output" 2>/dev/null || :; }
exec python3 tools/production/build.py "$@"
