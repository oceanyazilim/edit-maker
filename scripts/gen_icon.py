"""Generate the application icon (PNG + ICO) without third-party packages.

Usage: python scripts/gen_icon.py apps/desktop/src-tauri/icons
Design: near-black rounded square, white waveform bars with an accent bar (beat marker).
"""
import math
import struct
import sys
import zlib
from pathlib import Path


def png_bytes(size: int) -> bytes:
    px = bytearray()
    r = size * 0.22
    bars = [0.30, 0.55, 0.85, 0.45, 1.0, 0.62, 0.38, 0.72, 0.5]
    n = len(bars)
    margin = size * 0.20
    bw = (size - 2 * margin) / (n * 1.6)
    for y in range(size):
        px.append(0)  # filter: none
        for x in range(size):
            # rounded square mask with antialiasing
            dx = max(r - x, 0, x - (size - 1 - r))
            dy = max(r - y, 0, y - (size - 1 - r))
            d = math.hypot(dx, dy)
            a = max(0.0, min(1.0, r - d + 0.5)) if (dx > 0 and dy > 0) else 1.0
            col = (18, 18, 20)
            for i, h in enumerate(bars):
                x0 = margin + i * bw * 1.6
                if x0 <= x < x0 + bw:
                    half = h * (size - 2 * margin) / 2
                    if abs(y - size / 2) <= half:
                        col = (240, 192, 64) if i == 4 else (236, 236, 238)
            px.extend((*col, int(255 * a)))
    raw = bytes(px)

    def chunk(t, data):
        c = struct.pack(">I", len(data)) + t + data
        return c + struct.pack(">I", zlib.crc32(t + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico_bytes(pngs):
    header = struct.pack("<HHH", 0, 1, len(pngs))
    entries = b""
    data = b""
    offset = 6 + 16 * len(pngs)
    for size, png in pngs:
        s = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", s, s, 0, 0, 1, 32, len(png), offset)
        data += png
        offset += len(png)
    return header + entries + data


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    (out / "icon.png").write_bytes(png_bytes(512))
    (out / "icon.ico").write_bytes(ico_bytes([(s, png_bytes(s)) for s in (16, 32, 48, 256)]))
    print("icons written to", out)


if __name__ == "__main__":
    main()
