#!/usr/bin/env python3
"""Build a macOS .icns icon from the procedural 1024px source icon.

Pure Python so it works in restricted environments where `sips`/`iconutil`
cannot write their temporary files.
"""

import struct
import sys
import zlib
from pathlib import Path

from make_icon import SIZE as SOURCE_SIZE, build_rows, chunk

ROOT = Path(__file__).resolve().parent.parent
ICNS_PATH = ROOT / "assets" / "AppIcon.icns"
ICONSET_DIR = ROOT / "build" / "AppIcon.iconset"

# (file name, pixel size) pairs required by an .iconset.
ICONSET_FILES = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]

# ICNS entry type -> pixel size.
ICNS_ENTRIES = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
]


def downsample(rows, source_size: int, target_size: int):
    factor = source_size // target_size
    if factor * target_size != source_size:
        raise ValueError("target size must divide the source size evenly")

    output = []
    samples = factor * factor

    for target_y in range(target_size):
        source_y = target_y * factor
        row = bytearray()

        for target_x in range(target_size):
            source_x = target_x * factor
            red = green = blue = alpha = 0

            for offset_y in range(factor):
                source_row = rows[source_y + offset_y]
                start = source_x * 4
                end = (source_x + factor) * 4
                segment = source_row[start:end]
                red += sum(segment[0::4])
                green += sum(segment[1::4])
                blue += sum(segment[2::4])
                alpha += sum(segment[3::4])

            row += bytes(
                (
                    red // samples,
                    green // samples,
                    blue // samples,
                    alpha // samples,
                )
            )

        output.append(bytes(row))

    return output


def encode_png(size: int, rows) -> bytes:
    raw = b"".join(b"\x00" + row for row in rows)
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main() -> None:
    if SOURCE_SIZE != 1024:
        print("expected a 1024px source icon", file=sys.stderr)
        sys.exit(1)

    source_rows = build_rows()
    cache = {SOURCE_SIZE: source_rows}

    def rows_for(size: int):
        if size not in cache:
            cache[size] = downsample(source_rows, SOURCE_SIZE, size)
        return cache[size]

    ICONSET_DIR.mkdir(parents=True, exist_ok=True)
    for name, size in ICONSET_FILES:
        (ICONSET_DIR / name).write_bytes(encode_png(size, rows_for(size)))

    entries = bytearray()
    for entry_type, size in ICNS_ENTRIES:
        png = encode_png(size, rows_for(size))
        entries += entry_type + struct.pack(">I", len(png) + 8) + png

    icns = b"icns" + struct.pack(">I", len(entries) + 8) + bytes(entries)
    ICNS_PATH.write_bytes(icns)

    print(f"wrote {ICNS_PATH} ({len(icns):,} bytes)")


if __name__ == "__main__":
    main()
