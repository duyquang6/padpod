#!/usr/bin/env python3
# Copyright 2026 ligt (https://github.com/duyquang6/padpod)
# SPDX-License-Identifier: GPL-3.0-only
"""Generate deploy/icon.png: a gamepad outline with a D-pad and two buttons,
in the SPRUCE theme's colours. Written by hand with zlib - the build host has no imaging
library."""
import struct
import zlib

W = H = 128
CREAM = (0xEB, 0xDB, 0xB2, 255)
ORANGE = (0xD6, 0x5D, 0x0E, 255)
CLEAR = (0, 0, 0, 0)


def pixel(x, y):
    # Body: a rounded bar with two grips hanging below it.
    body = 16 <= x < 112 and 36 <= y < 84
    grips = (16 <= x < 48 or 80 <= x < 112) and 84 <= y < 100
    corner = (x - 32) ** 2 + (y - 100) ** 2 > 16 ** 2 and y >= 92 and x < 48 or (x - 96) ** 2 + (y - 100) ** 2 > 16 ** 2 and y >= 92 and x >= 80
    if (body or grips) and not corner:
        # D-pad on the left, two buttons on the right, in the accent colour.
        dpad = (36 <= x < 44 and 48 <= y < 72) or (28 <= x < 52 and 56 <= y < 64)
        buttons = (x - 82) ** 2 + (y - 66) ** 2 <= 36 or (x - 94) ** 2 + (y - 54) ** 2 <= 36
        return ORANGE if dpad or buttons else CREAM
    return CLEAR


def chunk(tag, data):
    out = struct.pack(">I", len(data)) + tag + data
    return out + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


raw = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(W)) for y in range(H))
open(__file__.rsplit("/", 1)[0] + "/icon.png", "wb").write(
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(raw, 9))
    + chunk(b"IEND", b"")
)
