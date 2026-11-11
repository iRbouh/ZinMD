#!/usr/bin/env python3
"""Pack PNG pictures into a Windows .ico file.

    scripts/ico.py out.ico 16.png 24.png 32.png 48.png 64.png 256.png

Small sizes are stored the old way, as plain bitmaps, which every part of
Windows can read. The 256 pixel picture is stored as the PNG it is, which is
what Windows expects at that size. Needs nothing beyond Python itself.
"""
import struct
import sys
import zlib


def read_png(data):
    """The size and the RGBA rows of an 8-bit PNG."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    at, packed, header = 8, b"", None
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at : at + 8])
        body = data[at + 8 : at + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            packed += body
        at += 12 + length
    width, height, depth, colour, _, _, interlaced = header
    assert depth == 8 and not interlaced, "only plain 8-bit PNGs"
    channels = {2: 3, 6: 4}[colour]
    stride = width * channels
    raw = zlib.decompress(packed)
    rows, previous = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        filter_type = raw[start]
        row = bytearray(raw[start + 1 : start + 1 + stride])
        for x in range(stride):
            left = row[x - channels] if x >= channels else 0
            up = previous[x]
            corner = previous[x - channels] if x >= channels else 0
            if filter_type == 1:
                row[x] = (row[x] + left) & 255
            elif filter_type == 2:
                row[x] = (row[x] + up) & 255
            elif filter_type == 3:
                row[x] = (row[x] + (left + up) // 2) & 255
            elif filter_type == 4:
                guess = left + up - corner
                near = min((abs(guess - left), 0), (abs(guess - up), 1), (abs(guess - corner), 2))[1]
                row[x] = (row[x] + (left, up, corner)[near]) & 255
        previous = row
        if channels == 3:
            row = bytearray(b"".join(bytes(row[i : i + 3]) + b"\xff" for i in range(0, stride, 3)))
        rows.append(row)
    return width, height, rows


def bitmap(width, height, rows):
    """A picture the way an icon stores a bitmap: bottom row first, blue first."""
    header = struct.pack("<IiiHHIIiiII", 40, width, height * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytearray()
    for row in reversed(rows):
        for x in range(0, width * 4, 4):
            r, g, b, a = row[x : x + 4]
            pixels += bytes((b, g, r, a))
    # The old one-bit mask is still expected; the real transparency is above.
    mask = bytes(((width + 31) // 32) * 4) * height
    return header + bytes(pixels) + mask


def main():
    out, sources = sys.argv[1], sys.argv[2:]
    pictures = []
    for source in sources:
        data = open(source, "rb").read()
        width, height, rows = read_png(data)
        assert width == height and width <= 256, f"{source}: icons are square, 256 at most"
        pictures.append((width, data if width == 256 else bitmap(width, height, rows)))
    directory, body = b"", b""
    offset = 6 + 16 * len(pictures)
    for size, data in pictures:
        directory += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        body += data
        offset += len(data)
    with open(out, "wb") as file:
        file.write(struct.pack("<HHH", 0, 1, len(pictures)) + directory + body)


if __name__ == "__main__":
    main()
