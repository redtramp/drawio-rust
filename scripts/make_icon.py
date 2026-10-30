#!/usr/bin/env python3
"""Generate the drawio-rust application icon (pure stdlib, no PIL).

Writes a 128x128 RGBA PNG: blue rounded square with a white two-node
diagram motif (two rounded rectangles connected by a line).

Usage: python3 scripts/make_icon.py [out.png]
"""
import struct
import sys
import zlib

W = H = 128
R_BG = 28  # background corner radius


def in_rounded(px, py, x0, y0, x1, y1, r):
    if px < x0 or px > x1 or py < y0 or py > y1:
        return False
    # check the four corner squares
    cx = None
    cy = None
    if px < x0 + r and py < y0 + r:
        cx, cy = x0 + r, y0 + r
    elif px > x1 - r and py < y0 + r:
        cx, cy = x1 - r, y0 + r
    elif px < x0 + r and py > y1 - r:
        cx, cy = x0 + r, y1 - r
    elif px > x1 - r and py > y1 - r:
        cx, cy = x1 - r, y1 - r
    if cx is None:
        return True
    return (px - cx) ** 2 + (py - cy) ** 2 <= r * r


def dist_to_segment(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    l2 = vx * vx + vy * vy
    t = max(0.0, min(1.0, (wx * vx + wy * vy) / l2)) if l2 else 0.0
    dx, dy = px - (ax + t * vx), py - (ay + t * vy)
    return (dx * dx + dy * dy) ** 0.5


def pixel(px, py):
    # background rounded square
    if in_rounded(px, py, 0, 0, W - 1, H - 1, R_BG):
        bg = (37, 99, 235, 255)  # #2563eb
    else:
        bg = (0, 0, 0, 0)
    # motif: two white nodes + connector
    node_a = in_rounded(px, py, 28, 36, 60, 68, 8)
    node_b = in_rounded(px, py, 68, 84, 100, 116, 8)
    connector = dist_to_segment(px, py, 58, 62, 72, 88) <= 3.5
    if node_a or node_b or connector:
        return (255, 255, 255, 255)
    return bg


def write_png(path, rows):
    raw = b""
    for row in rows:
        raw += b"\x00" + bytes(v for px in row for v in px)
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0)
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "src-tauri/icons/icon.png"
    rows = [[pixel(x, y) for x in range(W)] for y in range(H)]
    write_png(out, rows)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
