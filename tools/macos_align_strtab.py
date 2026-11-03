"""Realign the symbol string pool of a Mach-O binary for the macOS 27 loader.

The Xcode 27 linker (ld-27037) writes the LC_SYMTAB string pool directly after
the 4-byte indirect-symbol entries without padding. With an odd number of
those entries the pool lands 4 bytes off an 8-byte boundary, and the macOS 27
loader then refuses the library ("mis-aligned LINKEDIT string pool"). This
moves the pool forward into the padding before the code signature, updates
``stroff`` and re-signs ad hoc. Aligned binaries are left untouched.

Every macOS link in this repository runs it through tools/macos-link, set as
the linker in .cargo/config.toml; it can also be run by hand:

    python3 tools/macos_align_strtab.py path/to/library.dylib

``--quiet`` prints nothing and skips files that are not thin 64-bit Mach-O,
as the linker wrapper needs.
"""

import struct
import subprocess
import sys
from pathlib import Path

MH_MAGIC_64 = 0xFEEDFACF
LC_SYMTAB = 0x2
LC_CODE_SIGNATURE = 0x1D


def is_macho(path: Path) -> bool:
    with path.open("rb") as handle:
        header = handle.read(4)
    return len(header) == 4 and struct.unpack("<I", header)[0] == MH_MAGIC_64


def align(path: Path) -> bool:
    data = bytearray(path.read_bytes())
    magic, _, _, _, ncmds, _, _, _ = struct.unpack_from("<IiiIIIII", data, 0)
    if magic != MH_MAGIC_64:
        raise SystemExit(f"{path}: not a thin 64-bit Mach-O file")
    offset = 32
    symtab_at = None
    blobs = []
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", data, offset)
        if cmd == LC_SYMTAB:
            symtab_at = offset
        elif cmd == LC_CODE_SIGNATURE:
            blobs.append(struct.unpack_from("<II", data, offset + 8)[0])
        offset += size
    if symtab_at is None:
        return False
    symoff, nsyms, stroff, strsize = struct.unpack_from("<IIII", data, symtab_at + 8)
    pad = (-stroff) % 8
    if pad == 0:
        return False
    limit = min([b for b in blobs if b >= stroff + strsize] or [len(data)])
    if stroff + strsize + pad > limit:
        raise SystemExit(f"{path}: no room to realign the string pool")
    data[stroff + pad : stroff + pad + strsize] = data[stroff : stroff + strsize]
    data[stroff : stroff + pad] = bytes(pad)
    struct.pack_into("<IIII", data, symtab_at + 8, symoff, nsyms, stroff + pad, strsize)
    path.write_bytes(bytes(data))
    subprocess.run(
        ["codesign", "--force", "--sign", "-", str(path)],
        check=True,
        capture_output=True,
    )
    return True


if __name__ == "__main__":
    quiet = "--quiet" in sys.argv[1:]
    for name in (arg for arg in sys.argv[1:] if arg != "--quiet"):
        path = Path(name)
        if quiet and not is_macho(path):
            continue
        changed = align(path)
        if not quiet:
            print(f"{name}: {'realigned' if changed else 'already aligned'}")
