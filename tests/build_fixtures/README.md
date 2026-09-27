# Building the test fixtures

Every fixture in `../fixtures/` is compiled here from its `.nsi` script by the
official `makensis` of the version it tests, with a 7-Zip listing of the result
in `../fixtures/expected/` as the parser tests' ground truth. The fixtures are
committed, so CI runs on them; nothing here is third-party.

## Under Wine (Linux)

```bash
tests/build_fixtures/build-wine.sh                  # every fixture
tests/build_fixtures/build-wine.sh deflate_solid    # named fixtures
```

[`build-wine.sh`](build-wine.sh) runs the compilers under Wine in a Docker image
([`Dockerfile`](Dockerfile)), built on first use and whenever the compiler list
changes. [`compilers.txt`](compilers.txt) lists each compiler and where it comes
from: the NSIS release zips, the 3.10 logging build laid over 3.10, the Jim
Park Unicode forks and NSIS 2.03, which ship only as installers and are
unpacked by 7-Zip rather than run, and NSIS 1.98 (below). The listings come from
7-Zip 24.08 for Windows, also under Wine, because only it turns an ANSI
installer's file names into UTF-8 through the Windows codepage.

Each fixture is compiled beside the same deterministic payloads, with their
modification time pinned, so a rebuild is byte-identical to the last. Check the
charset line in each `logs/<name>.log` before committing: makensis 3.x prints
`writing output (x86-ansi)` or `(x86-unicode)`, and a fixture whose log shows
the wrong target is a failed build.

**Control.** Rebuilt this way, every fixture passes the full test suite, and its
listing matches the one it replaces in everything but timestamps and the
payload. The payload is the one intended change: the Windows builds these
replaced did not agree on it. Three batches packed three `payload.txt` files of
53, 54 and 55 bytes (the Windows shell left spaces before the line break), and
the tests tolerated any of them. Now every fixture carries one payload and the
test checks it exactly. `config.ini` repeats its keys so every compressor
shrinks it, which the budget tests need and which the old 23-byte version never
gave them.

## NSIS 1.98

SourceForge no longer lists NSIS 1.98; it comes from the download on Justin
Frankel's Nullsoft NSIS page, pinned by hash in `compilers.txt`. It is the one
compiler installed by running its installer, since 7-Zip cannot open an NSIS 1.x
file: silently, under Xvfb, stopped after a minute because it opens its
documentation when done, and judged by the `makensis.exe` and
`makensis-bz2.exe` it leaves. 7-Zip cannot list its output either, so the three
`nsis1x*` fixtures are checked against their build logs instead.
