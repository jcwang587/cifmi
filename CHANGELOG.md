# Changelog

## 0.2.1

- PyPI has wheels for Linux and Windows on Arm64, so pip installs cifmi there without Rust.
- Python 3.10 is no longer supported. pip on Python 3.10 keeps installing 0.2.0.
- Node.js 18 is no longer supported. On Node.js 18, `npm install cifmi` keeps installing 0.2.0.

## 0.2.0

- cifmi writes crystal structures as CIF 1.1, with `write_cif` and `write_cif_str` in Rust and Python and `writeCifFile` and `writeCifStr` in Node.js.
- `Structure::new` in Rust and `cifmi.Structure` in Python build a structure to write from its cell, operations and sites.
- Space groups are written in the International Tables' current symbols, with the e glide where a group has one, and reading takes either spelling.
- Breaking: displacements are held and written as U in Å², as `u_iso` in Rust and Python and `uIso` in Node.js in place of `b_iso` and `bIso`.
- Breaking: a structure built from its parts has the new symmetry source `SymmetrySource::Given`.
- Coordinates at thirds stay on their special positions, so the text cifmi writes reads back as the same structure.
- Sites that end on one position are read as one site and reported as `merged-sites`.
- Reading keeps an atom type's own oxidation number when the file also lists a charged type of the same element.
- On npm, the binaries are the platform packages `@cifmi/cifmi-<platform>`, and installing `cifmi` adds the one for the system.

## 0.1.0

- First release: a reader of CIF 1.1 and 2.0 files into crystal structures, for Rust, Python and Node.js.
