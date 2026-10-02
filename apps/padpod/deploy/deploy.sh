#!/bin/sh
# Copyright 2026 ligt (https://github.com/duyquang6/padpod)
# SPDX-License-Identifier: GPL-3.0-only
# Build for the device and install into spruce's app folder.
#
#   HOST=root@<device-ip> ./deploy/deploy.sh
#
# No default host on purpose: a baked-in address is an accidental deploy to
# whatever happens to answer at it.
set -e
cd "$(dirname "$0")/.."

if [ -z "${HOST:-}" ]; then
    echo "HOST is not set. Example: HOST=root@<device-ip> $0" >&2
    exit 1
fi
TARGET=aarch64-unknown-linux-musl
BINARY="../../target/$TARGET/release/padpod"
DEST=/mnt/SDCARD/App/Padpod

cargo build --release --target "$TARGET" -p padpod

ssh "$HOST" "mkdir -p $DEST && [ -w $DEST ]" || { echo "cannot write $DEST on $HOST" >&2; exit 1; }
# `scp -O`: the device's dropbear has no SFTP server on its default path.
# FAT will not replace a running executable; land it as .next instead and let
# launch.sh promote it on the next start.
if ! scp -O -q "$BINARY" "$HOST:$DEST/padpod" 2>/dev/null; then
    echo "the app is running; the new build takes effect the next time it opens" >&2
    scp -O -q "$BINARY" "$HOST:$DEST/padpod.next"
fi
scp -O -q deploy/launch.sh deploy/config.json deploy/icon.png ../../crates/brick/assets/OFL.txt ../../LICENSE ../../ADDITIONAL-TERMS.md ../../CREDITS.md "$HOST:$DEST/"
ssh "$HOST" "chmod +x $DEST/launch.sh $DEST/padpod* ; sync"
echo "installed to $HOST:$DEST"
