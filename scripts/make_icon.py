#!/usr/bin/env python3
"""Generates SimpleEditor's icon without third-party libraries.

Design (original, MIT like the rest of the repository): white "{ → }" on a
rounded square using the UX action blue (#0C66E4 → #0055CC). Rendered with
signed distance functions and analytic anti-aliasing.

Outputs (in assets/icon/):
  icon-1024.png        master image
  icon-<n>.png         16, 32, 48, 64, 128, 256, 512
  simpleeditor.ico     Windows icon (PNG entries 16, 32, 48, 256)
  icon-64.rgba         raw RGBA 64x64 for the window icon (include_bytes!)

macOS .icns is produced from these PNGs by scripts/package-macos.sh
(iconutil). Usage: python3 scripts/make_icon.py
"""

import math
import os
import struct
import sys
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "icon")

TOP = (0x0C, 0x66, 0xE4)
BOTTOM = (0x00, 0x55, 0xCC)


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    cx, cy = ax + t * dx - px, ay + t * dy - py
    return math.sqrt(cx * cx + cy * cy)


def rounded_rect_dist(px, py, cx, cy, hw, hh, r):
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    ox, oy = max(qx, 0.0), max(qy, 0.0)
    return math.sqrt(ox * ox + oy * oy) + min(max(qx, qy), 0.0) - r


def brace(cx, flip):
    """Polyline of a curly brace in unit coordinates (0..1)."""
    s = -1.0 if flip else 1.0
    w = 0.075
    pts = [
        (cx + s * w, 0.30),
        (cx + s * 0.02, 0.31),
        (cx, 0.36),
        (cx, 0.45),
        (cx - s * 0.035, 0.50),
        (cx, 0.55),
        (cx, 0.64),
        (cx + s * 0.02, 0.69),
        (cx + s * w, 0.70),
    ]
    return list(zip(pts, pts[1:]))


def shapes():
    segs = brace(0.30, False) + brace(0.70, True)
    # Arrow in the middle.
    segs += [((0.40, 0.50), (0.60, 0.50)), ((0.535, 0.44), (0.60, 0.50)), ((0.535, 0.56), (0.60, 0.50))]
    return segs


SEGMENTS = shapes()


def render(size):
    """Returns RGBA bytes (row-major) for a size x size icon."""
    # macOS icon grid: the shape covers ~80% of the canvas.
    margin = size * 0.10
    half = (size - 2 * margin) / 2
    radius = half * 0.45
    stroke = size * 0.045
    c = size / 2
    segs = [((ax * size, ay * size), (bx * size, by * size)) for (ax, ay), (bx, by) in SEGMENTS]
    out = bytearray(size * size * 4)
    for y in range(size):
        py = y + 0.5
        g = (py - margin) / (2 * half)
        g = max(0.0, min(1.0, g))
        bg = tuple(TOP[i] + (BOTTOM[i] - TOP[i]) * g for i in range(3))
        for x in range(size):
            px = x + 0.5
            d_bg = rounded_rect_dist(px, py, c, c, half, half, radius)
            a_bg = max(0.0, min(1.0, 0.5 - d_bg))
            if a_bg <= 0.0:
                continue
            d_fg = min(seg_dist(px, py, *a, *b) for a, b in segs) - stroke / 2
            a_fg = max(0.0, min(1.0, 0.5 - d_fg))
            r = bg[0] * (1 - a_fg) + 255 * a_fg
            gg = bg[1] * (1 - a_fg) + 255 * a_fg
            b = bg[2] * (1 - a_fg) + 255 * a_fg
            i = (y * size + x) * 4
            out[i : i + 4] = bytes((round(r), round(gg), round(b), round(255 * a_bg)))
    return bytes(out)


def png(size, rgba):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + rgba[y * size * 4 : (y + 1) * size * 4] for y in range(size))
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(pngs):
    """ICO container with PNG-compressed entries (Windows Vista+)."""
    header = struct.pack("<HHH", 0, 1, len(pngs))
    offset = 6 + 16 * len(pngs)
    entries, data = b"", b""
    for size, blob in pngs:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def main():
    os.makedirs(OUT, exist_ok=True)
    blobs = {}
    for size in (16, 32, 48, 64, 128, 256, 512, 1024):
        rgba = render(size)
        blob = png(size, rgba)
        blobs[size] = blob
        with open(os.path.join(OUT, f"icon-{size}.png"), "wb") as f:
            f.write(blob)
        if size == 64:
            with open(os.path.join(OUT, "icon-64.rgba"), "wb") as f:
                f.write(rgba)
        print(f"icon-{size}.png", file=sys.stderr)
    with open(os.path.join(OUT, "simpleeditor.ico"), "wb") as f:
        f.write(ico([(s, blobs[s]) for s in (16, 32, 48, 256)]))


if __name__ == "__main__":
    main()
