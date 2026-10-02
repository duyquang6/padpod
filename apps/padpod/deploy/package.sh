#!/bin/sh
# Copyright 2026 ligt (https://github.com/duyquang6/padpod)
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
# Build Padpod and package it for both firmwares.
#
#   VERSION=2026.10.02 ./apps/padpod/deploy/package.sh
#       -> dist/Padpod-<version>-{spruceOS,stockOS}.zip
#
# The binary inside both archives is byte-identical. What differs is the app
# manifest and the directory it sits in: spruceOS reads /mnt/SDCARD/App, stock
# TrimUI reads /mnt/SDCARD/Apps. Each archive carries that directory itself, so
# installing is unzipping onto the root of the card. (As Truepod packages.)
set -e
cd "$(dirname "$0")/.."

VERSION="${VERSION:?set VERSION, for example VERSION=2026.10.02}"
TARGET=aarch64-unknown-linux-musl
ROOT=../..
APP=Padpod

echo "packaging Padpod $VERSION"
# The version shows in the app's menu.
PADPOD_VERSION="$VERSION" cargo build --release --target "$TARGET" -p padpod

# `zip` is not installed everywhere, and `python3 -m zipfile` drops the
# executable bit, which matters to anyone who unzips onto a real filesystem
# before copying. So: zip when it is there, otherwise write the archive with
# the mode bits set.
archive_dir() {
    out=$1
    shift
    if command -v zip >/dev/null 2>&1; then
        zip -qr "$out" "$@"
        return
    fi
    python3 - "$out" "$@" <<'PY'
import os, stat, sys, zipfile

out, *roots = sys.argv[1:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    def add(path):
        info = zipfile.ZipInfo.from_file(path, path)
        info.external_attr = stat.S_IMODE(os.stat(path).st_mode) << 16
        info.compress_type = zipfile.ZIP_DEFLATED
        with open(path, "rb") as f:
            z.writestr(info, f.read())

    for root in roots:
        if os.path.isfile(root):
            add(root)
            continue
        for base, _, files in os.walk(root):
            for name in sorted(files):
                add(os.path.join(base, name))
PY
}

DIST="$ROOT/dist"
rm -rf "$DIST"
mkdir -p "$DIST"

# spruce reads `App`, stock reads `Apps`; the manifests differ in their keys:
# stock wants `package`/`icontop`, spruce wants `icon` and its `devices` list.
for firmware in spruceOS stockOS; do
    case "$firmware" in
        spruceOS) manifest=deploy/config.json;       parent=App ;;
        stockOS)  manifest=deploy/config.stock.json; parent=Apps ;;
    esac

    stage="$DIST/$firmware/$parent/$APP"
    mkdir -p "$stage"
    cp "$ROOT/target/$TARGET/release/padpod" "$stage/padpod"
    cp deploy/launch.sh deploy/icon.png "$stage/"
    cp "$manifest" "$stage/config.json"
    # The license, the credits and the font's license travel with the binary,
    # as the licenses ask.
    cp "$ROOT/LICENSE.md" "$ROOT/CREDITS.md" "$ROOT/crates/brick/assets/OFL.txt" "$stage/"
    chmod +x "$stage/padpod" "$stage/launch.sh"

    cat > "$DIST/$firmware/INSTALL.txt" <<EOF
Padpod $VERSION - $firmware

1. Copy the "$parent" folder next to this file onto the root of the SD card,
   merging it with the "$parent" folder already there, so that you end up with

       /mnt/SDCARD/$parent/$APP/padpod

2. Turn Bluetooth on in the firmware's settings.

3. Open "Padpod". Press SELECT to choose a mode: PC, Xbox or PS4. Hold MENU
   for two seconds to quit.

This archive is for $firmware. The other firmware reads a different directory;
download the archive named for it instead of renaming this one.

Padpod is made by ligt. Its only official download is
https://github.com/duyquang6/padpod/releases - please share that link
rather than this file: re-uploading it anywhere else is not permitted (see
LICENSE.md).
EOF

    archive="Padpod-$VERSION-$firmware.zip"
    ( cd "$DIST/$firmware" && archive_dir "../$archive" "$parent" INSTALL.txt )
    rm -rf "${DIST:?}/$firmware"
done

ls -la "$DIST"
echo
echo "checksums:"
( cd "$DIST" && sha256sum ./*.zip )
