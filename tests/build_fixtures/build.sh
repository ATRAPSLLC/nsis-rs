#!/bin/bash
# Rebuild the NSIS test fixtures.
#
# Fixtures are compiled on a Windows host reachable over SSH, then downloaded
# together with a 7-Zip listing that serves as ground truth for the parser
# tests. Nothing about that host is baked in here — configure it with:
#
#   NSIS_BUILD_HOST   ssh destination, e.g. user@host or an ssh_config alias
#   NSIS_BUILD_PORT   ssh port (default 22)
#   NSIS_BUILD_DIR    remote working directory (default %USERPROFILE%\nsis_fixtures)
#
# Several fixtures pin a specific compiler, so the host needs more than one
# NSIS installation. Point these at the directory holding each makensis.exe:
#
#   NSIS_310   NSIS 3.10        (default C:\NSIS)
#   NSIS_246   NSIS 2.46
#   NSIS_225   NSIS 2.25
#   NSIS_203   NSIS 2.03
#   PARK_2461  NSIS 2.46.1-Unicode (Jim Park fork)
#   PARK_2462  NSIS 2.46.2-Unicode
#   PARK_2463  NSIS 2.46.3-Unicode
#
# Usage:  ./build.sh [fixture ...]     (no arguments rebuilds everything)

set -euo pipefail

: "${NSIS_BUILD_HOST:?set NSIS_BUILD_HOST to the ssh destination of the build machine}"
PORT="${NSIS_BUILD_PORT:-22}"
REMOTE_DIR="${NSIS_BUILD_DIR:-%USERPROFILE%\\nsis_fixtures}"

NSIS_310="${NSIS_310:-C:\\NSIS}"
NSIS_246="${NSIS_246:-}"
NSIS_225="${NSIS_225:-}"
NSIS_203="${NSIS_203:-}"
PARK_2461="${PARK_2461:-}"
PARK_2462="${PARK_2462:-}"
PARK_2463="${PARK_2463:-}"

SSH=(ssh -p "$PORT" "$NSIS_BUILD_HOST")
SCP_PORT=(-P "$PORT")
LOCAL_OUT="../fixtures"
LOG_DIR="logs"

# Which compiler builds which fixture. Anything unlisted uses NSIS 3.10.
compiler_for() {
    case "$1" in
        nsis246_ansi_solid|nsis246_ansi_latin1|dirs_nsis246_ansi_solid) echo "$NSIS_246" ;;
        nsis225_ansi)  echo "$NSIS_225" ;;
        nsis203_ansi)  echo "$NSIS_203" ;;
        park1_unicode) echo "$PARK_2461" ;;
        park2_unicode) echo "$PARK_2462" ;;
        park3_unicode) echo "$PARK_2463" ;;
        *)             echo "$NSIS_310" ;;
    esac
}

targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
    for nsi in *.nsi; do targets+=("${nsi%.nsi}"); done
fi

mkdir -p "$LOG_DIR"
echo "Preparing remote directory..."
"${SSH[@]}" "mkdir $REMOTE_DIR 2>nul & echo ok" >/dev/null

# ansi3_latin1.nsi and nsis246_ansi_latin1.nsi are stored as Windows-1252 and
# must reach the compiler byte for byte; scp copies verbatim, so do not
# normalise or re-encode them.
echo "Uploading scripts..."
scp "${SCP_PORT[@]}" -q ./*.nsi "${NSIS_BUILD_HOST}:${REMOTE_DIR}\\"

# Deterministic payloads. big.bin is 72 MiB of zeros: larger than the parser's
# default 64 MiB budget, but it compresses to almost nothing so the fixtures
# stay small enough to commit.
echo "Creating payloads..."
"${SSH[@]}" "cd /d $REMOTE_DIR && echo This is a test payload for NSIS fixture generation.> payload.txt" >/dev/null
"${SSH[@]}" "cd /d $REMOTE_DIR && (echo [Settings]& echo Key=Value)> config.ini" >/dev/null
"${SSH[@]}" "cd /d $REMOTE_DIR && if not exist big.bin fsutil file createnew big.bin 75497472" >/dev/null

failed=()
for name in "${targets[@]}"; do
    compiler="$(compiler_for "$name")"
    if [ -z "$compiler" ]; then
        echo "  SKIP $name (its compiler is not configured)"
        continue
    fi

    echo "  Building $name..."
    if "${SSH[@]}" "cd /d $REMOTE_DIR && \"${compiler}\\makensis.exe\" /V4 ${name}.nsi" \
        > "${LOG_DIR}/${name}.log" 2>&1; then
        scp "${SCP_PORT[@]}" -q "${NSIS_BUILD_HOST}:${REMOTE_DIR}\\${name}.exe" "${LOCAL_OUT}/${name}.exe"
    else
        echo "    FAILED — see ${LOG_DIR}/${name}.log"
        failed+=("$name")
        continue
    fi

    # Ground truth for the parser tests. 7-Zip mis-detects the larger Park
    # stubs as plain PE files, so those need an explicit archive type.
    listing_flags=(-slt -sccUTF-8)
    case "$name" in
        park2_unicode|park3_unicode) listing_flags+=(-tnsis) ;;
    esac
    7z l "${listing_flags[@]}" "${LOCAL_OUT}/${name}.exe" \
        > "${LOCAL_OUT}/expected/${name}.7z.txt" 2>&1
done

echo
echo "Built: ${#targets[@]} requested, ${#failed[@]} failed"
if [ ${#failed[@]} -gt 0 ]; then
    printf '  %s\n' "${failed[@]}"
    exit 1
fi
echo "Verify the charset line in each log before committing: makensis 3.x prints"
echo "  'writing output (x86-ansi)' or '(x86-unicode)'. A fixture whose log does"
echo "  not show the expected target is a failed build, not a fixture."
