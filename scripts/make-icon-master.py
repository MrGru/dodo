#!/usr/bin/env python3
"""Turn the supplied dodo artwork into the RGBA icon master.

    scripts/make-icon-master.py [--source <png>] [--out <png>] [--size 1024]

The artwork the project was given is an *opaque* PNG: a dark rounded-square
(squircle) tile painted on a pure-black canvas. An application icon must have
transparent corners instead — every platform draws its own shadow, hover ring
and (on macOS) rounded-rect grid slot around the artwork, and a black square
would show up as a black square.

So this script does exactly two things, and deliberately nothing else:

  1. Replaces everything outside the tile's rounded border with full
     transparency. The tile's own dark fill is kept.
  2. Resamples the result to a square RGBA master (1024x1024 by default).

The art itself is never touched: no recolouring, no re-cropping, no dropping
of the wordmark.

Why it is written in plain stdlib Python
----------------------------------------
The machine this was developed on has no ImageMagick and no Pillow, and adding
either to the project's manifests for a once-per-artwork-change chore is not a
trade worth making. `sips` can resize but cannot cut a shape out of an opaque
image. So the PNG codec below is hand-rolled: it is ~100 lines, it only has to
understand the two colour types involved, and it makes this script runnable on
any machine with python3 and nothing else.

It reads 8-bit non-interlaced RGB/RGBA PNGs only. Anything else is a hard
error with the reason printed — the one thing this must never do is emit a
plausible-looking but silently wrong icon.

How the cut is found
--------------------
Not by fitting a superellipse: a fitted curve that is a pixel off shows as a
clipped edge on one side and a black sliver on the other. Instead the tile's
own outline is used. The canvas background is exactly black (0,0,0) while the
tile's darkest fill sits around luminance 16-20, so a flood fill inward from
the canvas border over near-black pixels marks precisely the region outside
the tile, and stops at the tile's edge. Dark pixels *inside* the artwork are
never reached, because the fill is connected.

Downsampling then uses that mask twice, which is what keeps the edge clean:

  * alpha  = the mask's area coverage of the output pixel  -> a soft edge
  * colour = the source colour averaged over the *covered* part only

Averaging colour over the covered part is the whole trick. Averaging over the
full box would blend the transparent canvas's black into every edge pixel and
produce the dark halo that gives away a badly cut icon.
"""

from __future__ import annotations

import argparse
import struct
import sys
import zlib
from collections import deque
from pathlib import Path

# Luminance at or above this counts as "tile", below it as "canvas". The
# artwork's canvas is 0 with a little encoder noise (<=9 in practice) and the
# tile's darkest fill is ~15; 10 sits in the empty gap between the two.
TILE_LUMA_THRESHOLD = 10


def die(message: str) -> "NoReturn":  # noqa: F821 - message-only exit
    sys.stderr.write(f"make-icon-master.py: {message}\n")
    raise SystemExit(1)


# --- PNG ------------------------------------------------------------------


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def read_png(path: Path) -> tuple[int, int, int, bytearray]:
    """Return (width, height, channels, pixels) for an 8-bit RGB/RGBA PNG."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\x0a":
        die(f"{path} is not a PNG")

    pos, idat, width, height, ctype = 8, bytearray(), None, None, None
    while pos + 8 <= len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, depth, ctype, _comp, _filt, interlace = struct.unpack(
                ">IIBBBBB", body
            )
            if depth != 8:
                die(f"{path}: only 8-bit PNGs are supported (this one is {depth}-bit)")
            if interlace:
                die(f"{path}: interlaced PNGs are not supported")
            if ctype not in (2, 6):
                die(
                    f"{path}: only RGB (2) and RGBA (6) PNGs are supported "
                    f"(this one is colour type {ctype})"
                )
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if width is None:
        die(f"{path}: no IHDR chunk")

    raw = zlib.decompress(bytes(idat))
    channels = 3 if ctype == 2 else 4
    stride = width * channels
    if len(raw) != height * (stride + 1):
        die(f"{path}: decompressed size {len(raw)} does not match the header")

    pixels = bytearray(height * stride)
    prev = bytearray(stride)
    read = 0
    for y in range(height):
        f = raw[read]
        read += 1
        line = bytearray(raw[read : read + stride])
        read += stride
        if f == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif f == 4:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                upleft = prev[i - channels] if i >= channels else 0
                line[i] = (line[i] + _paeth(left, prev[i], upleft)) & 0xFF
        elif f != 0:
            die(f"{path}: unknown scanline filter {f}")
        pixels[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, channels, pixels


def write_rgba_png(path: Path, width: int, height: int, pixels: bytes) -> None:
    stride = width * 4
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter 0; the image is tiny and zlib handles the rest
        raw += pixels[y * stride : (y + 1) * stride]

    def chunk(kind: bytes, body: bytes) -> bytes:
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )

    path.write_bytes(
        b"\x89PNG\r\n\x1a\x0a"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


# --- the cut --------------------------------------------------------------


def tile_mask(width: int, height: int, channels: int, pixels: bytearray) -> bytearray:
    """1 where the tile is, 0 for the canvas around it.

    A flood fill inward from the canvas border over near-black pixels. Using
    the artwork's own outline rather than a fitted curve is what guarantees
    the cut lands exactly on the tile's edge.
    """
    count = width * height
    luma = bytearray(count)
    for i in range(count):
        j = i * channels
        luma[i] = (pixels[j] * 299 + pixels[j + 1] * 587 + pixels[j + 2] * 114) // 1000

    outside = bytearray(count)
    queue: deque[int] = deque()

    def seed(i: int) -> None:
        if not outside[i] and luma[i] < TILE_LUMA_THRESHOLD:
            outside[i] = 1
            queue.append(i)

    for x in range(width):
        seed(x)
        seed((height - 1) * width + x)
    for y in range(height):
        seed(y * width)
        seed(y * width + width - 1)

    while queue:
        i = queue.popleft()
        x, y = i % width, i // width
        if x > 0:
            seed(i - 1)
        if x + 1 < width:
            seed(i + 1)
        if y > 0:
            seed(i - width)
        if y + 1 < height:
            seed(i + width)

    if not any(outside):
        die(
            "found no canvas around the tile: the source does not look like the "
            "expected black-background artwork, refusing to guess"
        )
    return bytearray(1 - v for v in outside)


def resample(
    width: int,
    height: int,
    channels: int,
    pixels: bytearray,
    mask: bytearray,
    size: int,
) -> bytearray:
    """Area-average down to `size` x `size` straight-alpha RGBA.

    Colour is averaged over the masked-in part of each source box only; see
    the halo note in the module docstring.
    """
    out = bytearray(size * size * 4)
    xs = [x * width / size for x in range(size + 1)]
    ys = [y * height / size for y in range(size + 1)]

    for oy in range(size):
        top, bottom = ys[oy], ys[oy + 1]
        y0, y1 = int(top), min(height, int(bottom - 1e-9) + 1)
        for ox in range(size):
            left, right = xs[ox], xs[ox + 1]
            x0, x1 = int(left), min(width, int(right - 1e-9) + 1)

            total = covered = 0.0
            r = g = b = 0.0
            for sy in range(y0, y1):
                wy = min(bottom, sy + 1) - max(top, sy)
                if wy <= 0:
                    continue
                row = sy * width
                for sx in range(x0, x1):
                    wx = min(right, sx + 1) - max(left, sx)
                    if wx <= 0:
                        continue
                    weight = wx * wy
                    total += weight
                    if mask[row + sx]:
                        covered += weight
                        j = (row + sx) * channels
                        r += pixels[j] * weight
                        g += pixels[j + 1] * weight
                        b += pixels[j + 2] * weight

            o = (oy * size + ox) * 4
            if covered > 0.0:
                out[o] = min(255, int(r / covered + 0.5))
                out[o + 1] = min(255, int(g / covered + 0.5))
                out[o + 2] = min(255, int(b / covered + 0.5))
                out[o + 3] = min(255, int(255.0 * covered / total + 0.5))
    return out


def main() -> None:
    repo_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        description="Cut the black canvas off the dodo artwork and scale it to "
        "the RGBA icon master."
    )
    parser.add_argument(
        "--source",
        type=Path,
        default=repo_root / "assets/branding/dodo-artwork-source.png",
        help="the untouched artwork (default: the committed original)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=repo_root / "assets/branding/dodo-1024.png",
        help="where to write the master (default: assets/branding/dodo-1024.png)",
    )
    parser.add_argument("--size", type=int, default=1024, help="master edge, in px")
    args = parser.parse_args()

    if not args.source.is_file():
        die(f"no such source artwork: {args.source}")

    width, height, channels, pixels = read_png(args.source)
    if width != height:
        die(f"{args.source} is {width}x{height}; the artwork is expected to be square")

    mask = tile_mask(width, height, channels, pixels)
    rgba = resample(width, height, channels, pixels, mask, args.size)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    write_rgba_png(args.out, args.size, args.size, rgba)

    corner = rgba[3]
    centre = rgba[((args.size // 2) * args.size + args.size // 2) * 4 + 3]
    if corner != 0 or centre != 255:
        die(
            f"sanity check failed: corner alpha {corner} (want 0), centre alpha "
            f"{centre} (want 255) — the mask did not come out as expected"
        )
    print(f"wrote {args.out} ({args.size}x{args.size} RGBA)")


if __name__ == "__main__":
    main()
