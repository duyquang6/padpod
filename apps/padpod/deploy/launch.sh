#!/bin/sh
# Copyright 2026 ligt (https://github.com/duyquang6/padpod)
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
# The launcher's entry point for the app, under spruceOS or the stock TrimUI
# firmware. The binary owns /dev/fb0, reads the pad, and stands in for the
# system's bluetoothd while it runs.

# spruce's helpers, absent on stock firmware: guarded, and nothing else here
# needs them.
HELPERS=/mnt/SDCARD/spruce/scripts/helperFunctions.sh
if [ -f "$HELPERS" ]; then
    . "$HELPERS"
    command -v set_smart >/dev/null 2>&1 && set_smart
fi

export HOME="$(dirname "$0")"
cd "$HOME"

# FAT refuses to replace a running executable, so deploy.sh may have left the
# new build beside the old one. Promote it now that nothing runs.
if [ -x ./padpod.next ]; then
    mv -f ./padpod.next ./padpod
fi

# A sleeping handheld is a disconnected gamepad. Stock firmware's keymon
# honours two flags, as TrimUI's own music player sets them: stay_awake holds
# off the idle timer, stay_alive the suspend. Under spruce they are inert.
# Removed below after any exit.
echo 1 > /tmp/stay_awake
echo 1 > /tmp/stay_alive

./padpod > "$HOME/padpod.log" 2>&1

rm -f /tmp/stay_awake /tmp/stay_alive
# The app puts the system's bluetoothd and the backlight back on a clean exit.
# After a crash the markers it left in /tmp are still there; this restores
# from them (and does nothing otherwise).
./padpod restore 2>/dev/null
sync
