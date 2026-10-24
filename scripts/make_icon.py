#!/usr/bin/env python3
"""Generate the 1024x1024 source icon used by `tauri icon`.

Uses only the standard library so the asset can be regenerated anywhere.
"""

import math
import struct
import zlib
from pathlib import Path

SIZE = 1024
OUTPUT = Path(__file__).resolve().parent.parent / "assets" / "icon.png"

NAVY_TOP = (30, 64, 101)
NAVY_BOTTOM = (8, 22, 37)
FRONT_BUBBLE = (246, 250, 255)
BACK_BUBBLE = (255, 255, 255)
INK = (17, 42, 68)
ACCENT = (90, 169, 255)


def clamp(value: float) -> float:
    return max(0.0, min(1.0, value))


def mix(a, b, t: float):
    return (
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    )


def sd_round_rect(x, y, cx, cy, half_w, half_h, radius):
    qx = abs(x - cx) - half_w + radius
    qy = abs(y - cy) - half_h + radius
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius


def coverage(distance: float) -> float:
    return clamp(0.5 - distance)


def blend(pixel, color, alpha: float):
    r, g, b = pixel
    return (
        r + (color[0] - r) * alpha,
        g + (color[1] - g) * alpha,
        b + (color[2] - b) * alpha,
    )


def in_box(x, y, cx, cy, half_w, half_h) -> bool:
    return abs(x - cx) <= half_w + 2 and abs(y - cy) <= half_h + 2


def build_rows():
    rows = []

    for y in range(SIZE):
        py = y + 0.5
        row = bytearray()
        for x in range(SIZE):
            px = x + 0.5

            bg_distance = sd_round_rect(px, py, 512, 512, 512, 512, 232)
            bg_alpha = coverage(bg_distance)
            if bg_alpha <= 0:
                row += b"\x00\x00\x00\x00"
                continue

            shade = py / SIZE
            color = mix(NAVY_TOP, NAVY_BOTTOM, shade)

            # Soft highlight in the upper-left for a little depth.
            glow = clamp(1.0 - math.hypot(px - 260, py - 210) / 780.0)
            color = mix(color, (120, 180, 255), glow * 0.10)

            # Back speech bubble.
            if in_box(px, py, 386, 414, 252, 168):
                alpha = coverage(sd_round_rect(px, py, 386, 414, 252, 168, 72))
                color = blend(color, BACK_BUBBLE, alpha * 0.17)

            # Lines inside the back bubble.
            for cx, cy, half_w in ((330, 352, 138), (292, 412, 100)):
                if in_box(px, py, cx, cy, half_w, 14):
                    alpha = coverage(sd_round_rect(px, py, cx, cy, half_w, 14, 14))
                    color = blend(color, BACK_BUBBLE, alpha * 0.32)

            # Front speech bubble.
            if in_box(px, py, 642, 606, 258, 172):
                alpha = coverage(sd_round_rect(px, py, 642, 606, 258, 172, 72))
                color = blend(color, FRONT_BUBBLE, alpha)

            # Lines inside the front bubble.
            for cx, cy, half_w in ((622, 544, 168), (587, 610, 133), (552, 676, 98)):
                if in_box(px, py, cx, cy, half_w, 15):
                    alpha = coverage(sd_round_rect(px, py, cx, cy, half_w, 15, 15))
                    color = blend(color, INK, alpha)

            # Accent dot, bottom-right of the front bubble.
            if in_box(px, py, 806, 706, 27, 27):
                alpha = coverage(math.hypot(px - 806, py - 706) - 27)
                color = blend(color, ACCENT, alpha)

            row += bytes(
                (
                    round(clamp(color[0] / 255.0) * 255),
                    round(clamp(color[1] / 255.0) * 255),
                    round(clamp(color[2] / 255.0) * 255),
                    round(bg_alpha * 255),
                )
            )
        rows.append(bytes(row))

    return rows


def chunk(tag: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def main() -> None:
    rows = build_rows()
    raw = b"".join(b"\x00" + row for row in rows)
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_bytes(png)
    print(f"wrote {OUTPUT} ({OUTPUT.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
