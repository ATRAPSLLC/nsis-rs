# syntax=docker/dockerfile:1
#
# Every NSIS compiler the fixtures are built with, under Wine, and the 7-Zip
# whose listings are the parser tests' ground truth - its Windows build, also
# under Wine, because only that one turns an ANSI installer's file names into
# UTF-8 through the Windows codepage, as the tests expect.
#
# `build-wine.sh` builds this image on first use (and whenever this file,
# `compilers.txt` or `install-nsis.sh` changes) and runs it with this directory
# bind-mounted, so an `.nsi` edit needs no image rebuild.
#
# Arch rather than Debian because its Wine is the "new WoW64" build: it runs
# a 32-bit PE - every `makensis.exe` here is one - with no 32-bit Linux
# userspace at all (securs measured and pinned the same base for the same
# reason).
FROM archlinux:base

RUN pacman -Syu --noconfirm --needed wine curl unzip 7zip xorg-server-xvfb \
    && pacman -Scc --noconfirm

# The prefix, provisioned once here rather than on every compile.
ENV WINEPREFIX=/opt/wine
ENV WINEDEBUG=-all
RUN wineboot -u && wineserver -w

# 7-Zip 24.08 for Windows (the listings were first made with this build and,
# for some, a Linux 25.01, which is where they came to disagree), unpacked (it is a 7-Zip self-extractor) rather than installed. The Linux
# 7-Zip above does the unpacking, here and in `install-nsis.sh`.
ARG SEVENZIP_URL=https://www.7-zip.org/a/7z2408-x64.exe
ARG SEVENZIP_SHA256=67cb9d3452c9dd974b04f4a5fd842dbcba8184f2344ff72e3662d7cdb68b099b
RUN curl -fsSL -o /tmp/7z.exe "${SEVENZIP_URL}" \
    && echo "${SEVENZIP_SHA256}  /tmp/7z.exe" | sha256sum -c - \
    && 7z x -y -bso0 -bsp0 -o"${WINEPREFIX}/drive_c/7zip" /tmp/7z.exe 7z.exe 7z.dll \
    && rm /tmp/7z.exe

# The compilers, one directory per build: `C:\nsis\<slug>`.
COPY compilers.txt install-nsis.sh /opt/nsis-setup/
RUN /opt/nsis-setup/install-nsis.sh /opt/nsis-setup/compilers.txt
