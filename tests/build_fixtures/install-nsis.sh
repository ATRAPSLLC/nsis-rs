#!/bin/bash
# Installs every compiler `compilers.txt` lists, one directory each.
#
# Usage (inside the image, see Dockerfile):  install-nsis.sh compilers.txt
#
# NSIS ships as a zip, a special build as a zip laid over a release, and the
# forks with no zip as installers NSIS built, which 7-Zip unpacks as archives.
# Only NSIS 1.98 is installed by running its installer: 7-Zip cannot open an
# NSIS 1.x file. Each result is checked for `makensis.exe`.

set -euo pipefail

ROOT="${WINEPREFIX}/drive_c/nsis"
mkdir -p "${ROOT}"

grep -v '^\s*\(#\|$\)' "$1" | while read -r slug how url extra; do
    dest="${ROOT}/${slug}"
    download="/tmp/nsis-${slug}.download"
    echo "[nsis ${slug}] ${how} ${url}"
    curl -fsSL -o "${download}" "${url}"
    base=""
    case "${extra:-}" in
        sha256=*) echo "${extra#sha256=}  ${download}" | sha256sum -c - ;;
        *)        base="${extra:-}" ;;
    esac

    case "${how}" in
        zip)
            unpacked="$(mktemp -d)"
            unzip -q "${download}" -d "${unpacked}"
            # A release zip holds one top-level directory, `nsis-<version>`.
            mv "${unpacked}"/*/ "${dest}"
            rm -rf "${unpacked}"
            ;;
        overlay)
            cp -a "${ROOT}/${base:?overlay needs a base}" "${dest}"
            unzip -q -o "${download}" -d "${dest}"
            ;;
        install)
            # `timeout` exits 124 when it stops the installer, which is the
            # expected end: what counts is what landed.
            timeout 60 xvfb-run -a wine "${download}" /S "/D=C:\\nsis\\${slug}" || true
            wineserver -k || true
            rm -f "${dest}"/uninst*.exe
            ;;
        unpack)
            7z x -y -bso0 -bsp0 -o"${dest}" "${download}"
            # What the installer would have written for itself rather than
            # installed: its own plugin scratch space and uninstaller.
            rm -rf "${dest}/\$PLUGINSDIR" "${dest}/uninst.exe" "${dest}/uninstall.exe"
            ;;
        *)
            echo "[nsis ${slug}] unknown kind ${how}" >&2
            exit 1
            ;;
    esac

    test -f "${dest}/makensis.exe" || { echo "[nsis ${slug}] no makensis.exe" >&2; exit 1; }
    rm -f "${download}"
    echo "[nsis ${slug}] installed"
done
