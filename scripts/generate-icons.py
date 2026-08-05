#!/usr/bin/env python3
"""Regenerate every derived application-icon artifact from the 1024 master.

    scripts/generate-icons.py [--master <png>] [--remaster] [--check]

One command, so changing dodo's artwork is an edit plus a run rather than an
archaeology session. It writes:

    assets/macos/dodo.icns                          picked up by
                                                    scripts/macos-app-bundle.sh
    assets/windows/dodo.ico                         embedded in dodo.exe by
                                                    build.rs, and shipped loose
                                                    in the Windows zip
    assets/linux/hicolor/<n>x<n>/apps/dodo.png      shipped in the Linux tar.gz
    assets/branding/dodo-256.png                    embedded in the binary by
                                                    src/window_icon.rs

`assets/linux/dodo.desktop` is hand-written and committed, not generated - it
is text, not artwork.

dodo-256.png is the odd one out and the reason is worth stating: every other
artifact above is read by a packaging step, so it only has to exist next to the
binary. That one is read by dodo itself, at runtime, through `include_bytes!` -
which is what lets a *bare* executable (no .app, no installed .desktop) still
tell macOS and X11 what it looks like. It is therefore the only derived icon
whose bytes end up inside the shipped binary; 256 is the largest size either
consumer can actually draw, so it is also the smallest one that costs nothing
in quality. See RUNTIME_ICON_SIZE below.

It is byte-identical to assets/linux/hicolor/256x256/apps/dodo.png today, and
that is a decision rather than something to tidy away: git stores identical
content once, so the copy costs the repository nothing, while pointing
`include_bytes!` at a path under assets/linux/ from code that also runs on
macOS reads as a bug and couples the runtime icon to a packaging layout that
may legitimately change.

All of the above are committed. Packaging must not depend on this script (and
cannot: `iconutil` only exists on macOS, so a Linux runner could never build
the .icns), so the outputs are inputs to the release scripts.

--remaster additionally rebuilds the master itself from the committed original
artwork via scripts/make-icon-master.py, which is where the transparent-corner
cut is explained.

--check regenerates into a temporary directory and diffs against what is
committed, without writing anything. Useful in review; not wired into CI,
because the .icns is only reproducible on macOS.

Dependencies, and what happens when they are missing
----------------------------------------------------
Resizing is done here in stdlib Python (a box filter, which is the correct
filter for the large downscales an icon set needs) rather than by shelling out
to `sips` or ImageMagick, so the only external tool is `iconutil` for the
.icns. If `iconutil` is missing the script says so, still writes everything
else, and exits non-zero. It never writes a placeholder or an empty .icns:
an icon that looks fine to the build and blank to the user is the exact
failure this is written to avoid.
"""

from __future__ import annotations

import argparse
import filecmp
import importlib.util
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# The ten entries `iconutil` expects; anything missing silently degrades the
# rendered icon at that size, so the whole standard set is generated.
ICNS_SIZES = [
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

ICO_SIZES = [16, 32, 48, 64, 128, 256]

# The hicolor sizes a desktop environment actually looks for. 512 is included
# because GNOME Shell and KDE both use it for the app grid on HiDPI.
LINUX_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]

# The one derived PNG that is embedded in the binary rather than shipped beside
# it, read by src/window_icon.rs. 256 because that is the ceiling for both of
# its consumers: the macOS Dock draws at most 128pt, which is 256px on a Retina
# display, and an X11 _NET_WM_ICON is a task-bar/Alt-Tab thumbnail. Anything
# larger is bytes in every macOS and Linux binary that nothing ever draws --
# 512 costs 192,510 against this one's 46,586.
RUNTIME_ICON_SIZE = 256

# ICO entries at or below this edge are written as 32-bit BMP; larger ones as
# PNG. PNG-in-ICO needs Windows Vista or newer, which is not a real constraint
# any more, but the small sizes are the ones legacy shell code paths reach for,
# and BMP costs nothing there.
ICO_PNG_THRESHOLD = 64


def load_master_module():
    """Import scripts/make-icon-master.py for its PNG codec and box filter."""
    path = REPO_ROOT / "scripts" / "make-icon-master.py"
    if not path.is_file():
        sys.exit(f"generate-icons.py: missing {path}")
    spec = importlib.util.spec_from_file_location("dodo_icon_master", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


MASTER = load_master_module()


def resize(width: int, height: int, pixels: bytearray, size: int) -> bytearray:
    """Box-filter the RGBA master down to size x size, straight alpha."""
    out = bytearray(size * size * 4)
    xs = [x * width / size for x in range(size + 1)]
    ys = [y * height / size for y in range(size + 1)]

    for oy in range(size):
        top, bottom = ys[oy], ys[oy + 1]
        y0, y1 = int(top), min(height, int(bottom - 1e-9) + 1)
        for ox in range(size):
            left, right = xs[ox], xs[ox + 1]
            x0, x1 = int(left), min(width, int(right - 1e-9) + 1)

            total = alpha = 0.0
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
                    j = (row + sx) * 4
                    a = pixels[j + 3] / 255.0
                    total += weight
                    alpha += weight * a
                    # Colour is averaged in premultiplied space and divided
                    # back out, so a transparent neighbour never darkens an
                    # edge pixel. Same reasoning as the master's own cut.
                    r += pixels[j] * weight * a
                    g += pixels[j + 1] * weight * a
                    b += pixels[j + 2] * weight * a

            o = (oy * size + ox) * 4
            if alpha > 0.0:
                out[o] = min(255, int(r / alpha + 0.5))
                out[o + 1] = min(255, int(g / alpha + 0.5))
                out[o + 2] = min(255, int(b / alpha + 0.5))
                out[o + 3] = min(255, int(255.0 * alpha / total + 0.5))
    return out


def bmp_ico_entry(size: int, rgba: bytearray) -> bytes:
    """A 32-bit BGRA DIB as an .ico expects it: no file header, doubled height."""
    header = struct.pack(
        "<IiiHHIIiiII",
        40,  # biSize
        size,  # biWidth
        size * 2,  # biHeight: colour rows + the AND mask's rows
        1,  # biPlanes
        32,  # biBitCount
        0,  # biCompression = BI_RGB
        0,  # biSizeImage (may be 0 for BI_RGB)
        0,
        0,
        0,
        0,
    )
    body = bytearray()
    for y in range(size - 1, -1, -1):  # DIBs are bottom-up
        for x in range(size):
            j = (y * size + x) * 4
            body += bytes((rgba[j + 2], rgba[j + 1], rgba[j], rgba[j + 3]))
    # The AND mask is ignored for 32-bit entries but must still be present and
    # correctly sized (1bpp, rows padded to 4 bytes) or the entry is rejected.
    mask_stride = ((size + 31) // 32) * 4
    body += bytes(mask_stride * size)
    return header + bytes(body)


def write_ico(path: Path, entries: list[tuple[int, bytes]]) -> None:
    header = struct.pack("<HHH", 0, 1, len(entries))
    offset = len(header) + 16 * len(entries)
    directory = b""
    for size, payload in entries:
        directory += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,  # 0 means 256 in an .ico directory
            0 if size >= 256 else size,
            0,  # no palette
            0,  # reserved
            1,  # colour planes
            32,  # bits per pixel
            len(payload),
            offset,
        )
        offset += len(payload)
    path.write_bytes(header + directory + b"".join(p for _, p in entries))


def desktop_entry_present() -> Path:
    path = REPO_ROOT / "assets/linux/dodo.desktop"
    if not path.is_file():
        sys.exit(
            f"generate-icons.py: {path} is missing. It is hand-written and "
            "committed, not generated; restore it rather than regenerating."
        )
    return path


def generate(master_path: Path, out_root: Path) -> list[str]:
    """Write every artifact under out_root. Returns a list of problems."""
    width, height, channels, pixels = MASTER.read_png(master_path)
    if channels != 4:
        sys.exit(
            f"generate-icons.py: {master_path} has no alpha channel. Run with "
            "--remaster (or scripts/make-icon-master.py) first."
        )
    if width != height:
        sys.exit(f"generate-icons.py: {master_path} is {width}x{height}, not square")

    problems: list[str] = []

    # Every size is rendered once and reused: the iconset needs 32/256/512 twice
    # and the Linux and Windows sets overlap with it.
    needed = sorted(
        {s for _, s in ICNS_SIZES}
        | set(ICO_SIZES)
        | set(LINUX_SIZES)
        | {RUNTIME_ICON_SIZE}
    )
    rendered: dict[int, bytearray] = {}
    for size in needed:
        rendered[size] = (
            bytearray(pixels) if size == width else resize(width, height, pixels, size)
        )
        print(f"  rendered {size}x{size}")

    # --- macOS ------------------------------------------------------------
    macos_dir = out_root / "assets/macos"
    macos_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "dodo.iconset"
        iconset.mkdir()
        for name, size in ICNS_SIZES:
            MASTER.write_rgba_png(iconset / name, size, size, rendered[size])
        if shutil.which("iconutil") is None:
            problems.append(
                "iconutil not found: assets/macos/dodo.icns was NOT regenerated. "
                "It can only be built on macOS; the committed .icns is unchanged."
            )
        else:
            result = subprocess.run(
                ["iconutil", "-c", "icns", str(iconset), "-o", str(macos_dir / "dodo.icns")],
                capture_output=True,
                text=True,
            )
            if result.returncode != 0:
                problems.append(f"iconutil failed: {result.stderr.strip()}")
            else:
                print(f"  wrote {macos_dir / 'dodo.icns'}")

    # --- Windows ----------------------------------------------------------
    windows_dir = out_root / "assets/windows"
    windows_dir.mkdir(parents=True, exist_ok=True)
    entries = []
    for size in ICO_SIZES:
        if size <= ICO_PNG_THRESHOLD:
            entries.append((size, bmp_ico_entry(size, rendered[size])))
        else:
            png = Path(tempfile.mkstemp(suffix=".png")[1])
            MASTER.write_rgba_png(png, size, size, rendered[size])
            entries.append((size, png.read_bytes()))
            png.unlink()
    write_ico(windows_dir / "dodo.ico", entries)
    print(f"  wrote {windows_dir / 'dodo.ico'}")

    # --- Linux ------------------------------------------------------------
    for size in LINUX_SIZES:
        target = out_root / f"assets/linux/hicolor/{size}x{size}/apps"
        target.mkdir(parents=True, exist_ok=True)
        MASTER.write_rgba_png(target / "dodo.png", size, size, rendered[size])
    print(f"  wrote {len(LINUX_SIZES)} hicolor PNGs")

    # --- Embedded in the binary -------------------------------------------
    branding_dir = out_root / "assets/branding"
    branding_dir.mkdir(parents=True, exist_ok=True)
    runtime_icon = branding_dir / f"dodo-{RUNTIME_ICON_SIZE}.png"
    MASTER.write_rgba_png(
        runtime_icon,
        RUNTIME_ICON_SIZE,
        RUNTIME_ICON_SIZE,
        rendered[RUNTIME_ICON_SIZE],
    )
    print(f"  wrote {runtime_icon}")

    return problems


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--master",
        type=Path,
        default=REPO_ROOT / "assets/branding/dodo-1024.png",
        help="the RGBA master to derive from",
    )
    parser.add_argument(
        "--remaster",
        action="store_true",
        help="rebuild the master from the committed original artwork first",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate into a temporary tree and diff, writing nothing",
    )
    args = parser.parse_args()

    desktop_entry_present()

    if args.remaster:
        subprocess.run(
            [sys.executable, str(REPO_ROOT / "scripts/make-icon-master.py")], check=True
        )

    if not args.master.is_file():
        sys.exit(f"generate-icons.py: no such master: {args.master}")

    if args.check:
        with tempfile.TemporaryDirectory() as tmp:
            problems = generate(args.master, Path(tmp))
            differing = []
            for produced in sorted(Path(tmp).rglob("*")):
                if not produced.is_file():
                    continue
                rel = produced.relative_to(tmp)
                committed = REPO_ROOT / rel
                if not committed.is_file():
                    differing.append(f"{rel}: not committed")
                elif not filecmp.cmp(produced, committed, shallow=False):
                    differing.append(f"{rel}: differs")
            for line in differing:
                print(f"  {line}")
            if problems:
                for p in problems:
                    print(f"generate-icons.py: {p}", file=sys.stderr)
                raise SystemExit(1)
            raise SystemExit(1 if differing else 0)

    problems = generate(args.master, REPO_ROOT)
    if problems:
        for p in problems:
            print(f"generate-icons.py: {p}", file=sys.stderr)
        raise SystemExit(1)
    print("icons regenerated")


if __name__ == "__main__":
    main()
