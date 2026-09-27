#!/bin/bash
# Builds the NSIS test fixtures on Linux, under Wine, with no Windows host.
#
# The compilers `compilers.txt` lists run in the image `Dockerfile` describes,
# with this directory bind-mounted. Each fixture is compiled in a scratch
# directory inside the container, beside the deterministic payloads its script
# packs, and lands with its 7-Zip listing - the parser tests' ground truth:
#
#   <name>.nsi -> ../fixtures/<name>.exe
#                 ../fixtures/expected/<name>.7z.txt
#                 logs/<name>.log
#
# Usage:  ./build-wine.sh [fixture ...]     (no arguments rebuilds everything)
#
# Verify the charset line in each log before committing: makensis 3.x prints
# `writing output (x86-ansi)` or `(x86-unicode)`, and a fixture whose log does
# not show the expected target is a failed build, not a fixture.

set -euo pipefail

cd "$(dirname "$0")"

IMAGE="nsis-fixtures:wine"

# The image is rebuilt when anything it is built from is newer than it, so a
# compiler added to `compilers.txt` is picked up without a manual step.
created="$(docker image inspect -f '{{.Created}}' "$IMAGE" 2>/dev/null || true)"
newest="$(stat -c %Y Dockerfile compilers.txt install-nsis.sh | sort -n | tail -1)"
if [ -z "$created" ] || [ "$(date -d "$created" +%s)" -lt "$newest" ]; then
    docker build -t "$IMAGE" .
fi

# Which compiler builds which fixture; anything unlisted uses NSIS 3.10.
compiler_for() {
    case "$1" in
        nsis246_ansi_solid|nsis246_ansi_latin1|dirs_nsis246_ansi_solid) echo "2.46" ;;
        nsis225_ansi)  echo "2.25" ;;
        nsis203_ansi)  echo "2.03" ;;
        park1_unicode) echo "park1" ;;
        park2_unicode|park2_opcodes) echo "park2" ;;
        park3_unicode|park3_opcodes) echo "park3" ;;
        opcodes_logbuild|plugin_logbuild) echo "3.10log" ;;
        nsis1x|nsis1x_uninst|nsis1x_bzip2) echo "1.98" ;;
        *)             echo "3.10" ;;
    esac
}

# The executable in that directory. NSIS 1.98 chooses its compressor when
# makensis is built rather than per script, so its distribution ships a second
# binary for bzip2, and the fixtures that need it name it.
compiler_exe_for() {
    case "$1" in
        nsis1x_bzip2) echo "makensis-bz2.exe" ;;
        *)            echo "makensis.exe" ;;
    esac
}

# The 7-Zip type to force: 7-Zip mis-detects the larger Park stubs as plain
# PE files. It cannot open an NSIS 1.x installer at all, so that listing holds
# the refusal - which is the ground truth for it.
listing_type_for() {
    case "$1" in
        park2_unicode|park3_unicode|park2_opcodes|park3_opcodes) echo "-tnsis" ;;
        *)                           echo "" ;;
    esac
}

targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
    for nsi in *.nsi; do targets+=("${nsi%.nsi}"); done
fi

mkdir -p logs ../fixtures/expected
failed=()
skipped=()
for name in "${targets[@]}"; do
    [ -f "${name}.nsi" ] || { echo "no ${name}.nsi here" >&2; exit 1; }
    compiler="$(compiler_for "$name")"
    exe="$(compiler_exe_for "$name")"
    listing="$(listing_type_for "$name")"

    echo "  Building ${name} (NSIS ${compiler})..."
    # Payloads, the same for every fixture. (The Windows builds these replace
    # did not agree: three batches packed a 53-, 54- and 55-byte payload.txt,
    # the remote shell leaving spaces before the line break.)
    # config.ini repeats its keys so every compressor shrinks it: it is the
    # entry the budget tests need compressed, which the 23-byte version never
    # was and the 27-byte one only was by accident. big.bin is 72 MiB of
    # zeros: larger than the parser's default 64 MiB budget, but
    # it compresses to almost nothing so the fixtures stay small enough to
    # commit. Their modification time is pinned, because NSIS packs it: with
    # it fixed, a rebuild is byte-identical to the last. `ansi3_latin1.nsi`
    # and `nsis246_ansi_latin1.nsi` are Windows-1252 and reach the compiler
    # byte for byte: `cp` does not re-encode.
    status=0
    docker run --rm -v "$PWD/..:/tests" "$IMAGE" bash -c "
        set -e
        compiler=\"\$WINEPREFIX/drive_c/nsis/${compiler}/${exe}\"
        test -f \"\$compiler\" || exit 3
        work=\$(mktemp -d) && cd \"\$work\"
        cp /tests/build_fixtures/*.nsi .
        printf 'This is a test payload for NSIS fixture generation.\r\n' > payload.txt
        { printf '[Settings]\r\n'; for i in 1 2 3 4 5 6 7 8; do printf 'Key%s=Value\r\n' \$i; done; } > config.ini
        truncate -s 75497472 big.bin
        touch -d '2026-01-01T00:00:00Z' payload.txt config.ini big.bin
        wine 'C:\\nsis\\${compiler}\\${exe}' /V4 '${name}.nsi' \
            > /tests/build_fixtures/logs/${name}.log 2>&1 || exit 4
        wineserver -w
        cp '${name}.exe' /tests/fixtures/
        # Listed by bare name from beside it, so the listing names the fixture
        # and not wherever the container mounted it.
        cd /tests/fixtures
        wine 'C:\\7zip\\7z.exe' l -slt -sccUTF-8 ${listing} '${name}.exe' \
            > expected/${name}.7z.txt 2>&1 || true
        wineserver -w
        chown $(id -u):$(id -g) /tests/fixtures/${name}.exe \
            /tests/fixtures/expected/${name}.7z.txt /tests/build_fixtures/logs/${name}.log
    " 2> >(grep -v XDG_RUNTIME_DIR >&2) || status=$?

    case "$status" in
        0) ;;
        3) echo "    SKIP (no NSIS ${compiler} in the image; add it to compilers.txt)"; skipped+=("$name") ;;
        *) echo "    FAILED - see logs/${name}.log"; failed+=("$name") ;;
    esac
done

echo
echo "Built: ${#targets[@]} requested, ${#skipped[@]} skipped, ${#failed[@]} failed"
[ ${#skipped[@]} -eq 0 ] || printf '  skipped %s\n' "${skipped[@]}"
if [ ${#failed[@]} -gt 0 ]; then
    printf '  failed %s\n' "${failed[@]}"
    exit 1
fi
