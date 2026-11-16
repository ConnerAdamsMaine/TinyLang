#!/usr/bin/env bash
# Fuzz campaign driver for tynra_core (frame + artifact targets).
# Untracked tooling. Logs live on the Windows side so they survive WSL VM restarts.
set -u
cd -- "$(dirname -- "${BASH_SOURCE[0]}")" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"
LOGDIR='/mnt/c/Users/Conner Adams/Desktop/404connernotfound/TinyOne/docs/evidence'
rm -f "$LOGDIR/fuzz-frame-2026-11-11.log" "$LOGDIR/fuzz-artifact-2026-11-11.log" "$LOGDIR/fuzz-done-2026-11-11.marker"

cargo +nightly fuzz run frame -- -max_total_time=300 -rss_limit_mb=4096 > "$LOGDIR/fuzz-frame-2026-11-11.log" 2>&1
rc=$?
echo "FRAME_EXIT=$rc" >> "$LOGDIR/fuzz-frame-2026-11-11.log"

cargo +nightly fuzz run artifact -- -max_total_time=300 -rss_limit_mb=4096 > "$LOGDIR/fuzz-artifact-2026-11-11.log" 2>&1
rc=$?
echo "ARTIFACT_EXIT=$rc" >> "$LOGDIR/fuzz-artifact-2026-11-11.log"

echo ALL_FUZZ_DONE > "$LOGDIR/fuzz-done-2026-11-11.marker"
