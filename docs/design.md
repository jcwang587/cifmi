# cifmi design

cifmi reads CIF files into crystal structures and writes crystal structures
as CIF: a Rust library with Python and Node.js bindings, giving one
structure for one file, and one text for one structure, in every runtime.
This document records the evidence behind it, the requirements, the rules
it reads files and writes structures by, its data model, and how it is
validated.

Status: cifmi reads files as §6 describes, validated on 6,000 and 10,000
random COD files and on the whole archive (§2), and writes structures as §7
describes, validated by writing and reading back 50,000 random COD files and
the whole archive (§7).

## 1. Why another reader

CIF readers disagree. On 6,000 random COD files, two widely used readers
built different structures for about 10% of files. Most differences were
position offsets below 1e-4, invisible in a viewer. About 2% differed
visibly: one refused 60 files that the other accepts ("the symmetry does
not fit the listed atoms"), showed 46 that it rejects, and built a
different number of atoms for 10. Each reader settles what a file leaves
open by rules of its own, mostly unstated, so the structure depends on the
program that read it. cifmi reads every file by explicit crystallographic
rules and reports every choice the file leaves open.

## 2. Evidence

The same seeded sample of 6,000 of the 521,901 CIFs in a COD archive
(`cod-rev297631-2025.02.07.zip`) is used throughout.

Speed on the sample: cifmi natively 6.4 s, compiled to WebAssembly in
Node 8.2 s.

What the files contain:

| Feature | Files |
| --- | ---: |
| Symmetry operations listed | 5,956 |
| ... of which do not close as a group | 0 |
| Hall symbol given | 5,975 |
| Hall symbol only, no operations | 22 |
| Hermann-Mauguin symbol only | 17 |
| No symmetry at all | 5 |
| DDLm data names (`_atom_site.fract_x`) | 28 |
| `_atom_site_symmetry_multiplicity` given | 2,749 |
| Formula and Z given | 5,968 |
| Anisotropic displacement loop | 5,280 |
| A `data_` line inside a text field (an embedded reflection file) | 15 |
| More than one data block | 0 |

77 files of the sample need a rule of their own:

- 38 have a site whose occupancies add up to more than 1. In 7 the file
  lists atoms again at symmetry-equivalent positions (a listing already
  partly expanded, labels such as `Br1F*`), whose rows add up to 2, 4,
  12 or 60. 24 are over by at most 0.064, from rounding or refinement; 8 of
  these are AMCSD entries whose occupancies are padded with zeros
  (`0.91200`), so the listed digits do not tell rounding apart. 7 are over by
  0.2 to 2.2, errors in the file (an O site at occupancy 2, O and F both at 1
  on one site, a water pseudo-atom at 3.2).
- 15 embed the refinement's reflection file in a text field
  (`_iucr_refine_fcf_details`, `_shelx_fcf_file`), whose `data_` line is
  text, not a new block.
- 22 list only placeholder coordinates (`? ? ? ?`), 1 has no cell, and 1
  names its atoms with the placeholder type `M`.

Special positions: of 435,418 sites in the sample, snapping within 0.05 Å
merges images at 75. At 65 the occupancy forbids separate atoms there; their
offsets, up to 0.024 Å (median 0.0013 Å), are rounding or unconstrained
refinement. At the other 10 (occupancy 0.04 to 0.5) separate atoms are
possible. Where the file lists a site symmetry order or multiplicity, it says
which (two sites on the special position, two off it). Where the declared
formula decides, offsets up to 0.0105 Å are on the special position, and one
of 0.021 Å is an atom split around it, as SHELX writes a solvent molecule
straddling an inversion centre (`PART -1`).

Rows at symmetry-equivalent positions add their occupancies: the declared
formula agrees in 39 of 40 files where that matters. SHELX lists the
two H atoms of a CH₂ on a mirror at half occupancy each, and their sum is one
full atom at each position. Only where the sum overfills a site (2, 4, 12,
60 above) does a row repeat an atom.

cifmi reads 5,968 of the 6,000 files and refuses 32: 24 with no positions
or no cell, 2 modulated structures, and 6 with errors in them (sites at
1.2 to 2 full, a placeholder element). Among the files read are the 60
with SHELX half-atom pairs, the 40 overfull by rounding or with an embedded
reflection file, and the 7 listings already expanded. Where the reading of
a file was in doubt and its declared formula × Z could decide, the formula
sided with cifmi's reading in 15 files and with no reading in 4, and 1 file
has none. cifmi reads the 6,000 files in 4.3 s from Python on one thread,
and 4,200 corrupted or truncated variants of 600 of them give errors,
never a crash.

A second, 10,000-file sample repeats the picture: cifmi reads 9,943
files, and where a reading was in doubt the declared formula sided with it
20 times and against it once. That sample found SHELXL-97's
site-symmetry orders under `_atom_site_symmetry_multiplicity` (42% of the
files), one file whose Hall and Hermann-Mauguin symbols disagree, and one
naming the cubic `P 42 3 2` for an orthorhombic cell, all read right or
refused since.

The whole archive, all 521,901 files: no crash; cifmi reads 519,196,
every refusal class counted (no atom sites 1,571, a site over 1.1 full
692, modulated 295, a cell without the symmetry of its operations 53, no
cell 46, and 48 one-offs); 0.46% of the files carry colliding atoms, 0.3%
an overfull site, 0.16% listed multiplicities that differ from their
orbits, 0.05% a cell that departs slightly from the symmetry of its
operations. Of the 10,077 files refused or flagged, where the declared
formula decides a reading in doubt it sides with cifmi 115 times and
against it 28, 18 of those being formulas that count dummy atoms listed at
placeholder coordinates; 19 partly occupied sites near a special position
that the distance alone read against the formula are settled by the
declared contents since.

No reader is the arbiter; objective checks are (§10).

## 3. Requirements

1. **Consistency.** One file read gives one structure in every runtime, bit
   for bit, and one structure written gives one text, byte for byte.
2. **One reading.** No alternative reading and no fallback: a file is read
   by the rules of §6 or refused with the reason; no other reader is
   consulted. A structure is written by the rules of §7 or refused with the
   reason.
3. **Correctness.** Text fields, settings, special positions, pre-expanded
   listings, deuterium, displacement parameters, symmetry that does not
   fit.
4. **Robustness.** No crash, unbounded memory or runaway time on any input;
   errors carry the reason (and the line, for syntax). No silent fallback: a
   file the reader cannot read is an error, and every choice the
   file leaves open is reported as a diagnostic, never made quietly.
5. **Reach.** Native Rust, with Python bindings through PyO3 and Node.js
   bindings through napi-rs, all reading and writing through the same code.
6. **Performance.** About a thousand COD files per second on one thread;
   writing a structure takes a fraction of the time reading it does.
7. **Determinism.** Results do not depend on thread count or platform.
8. **Round trip.** A structure cifmi reads is written as a CIF it reads back
   as the same structure, which writes as the same text: its setting, sites
   and atoms as they are, with the standard names.

## 4. Language: Rust

Rust reaches Python (PyO3) and Node (napi-rs) from one code base, is memory
safe on files downloaded from the internet, and compiles to WebAssembly
should a browser ever need the reader. TypeScript cannot serve Python; C++
reaches every runtime only through three build systems and gives up memory
safety. Speed does not decide it: compiled languages tie.

## 5. Architecture

```text
bytes ─▶ lexer ─▶ document ─▶ extraction ─▶ symmetry ─▶ expansion ─▶ Structure
                   (blocks,    (cell, rows,   (operations,  (sites,       + diagnostics
                    loops)      names)         source)       atoms)
                                                                │
                      Rust API (read_cif, read_cif_str) ◀───────┤
                      Python package (PyO3) ◀───────────────────┤
                      Node.js addon (napi-rs) ◀─────────────────┘

Structure (read, or built from its parts) ─▶ checks ─▶ writer ─▶ CIF 1.1 text
                      (write_cif, write_cif_str; the Python package and the Node.js addon)
```

1. **Lexer.** CIF 1.1: comments start at a `#` outside values, a quoted
   string closes at its
   quote followed by whitespace, text fields keep their line breaks, UTF-8 is
   kept; files with the `#\#CIF_2.0` magic get CIF 2.0 strings, lists and
   tables. A bare `END` line closes the block, as AMCSD and RRUFF files end.
   Tokens borrow from the input and carry their line.
2. **Document.** Blocks, items and loops. Data names are case-insensitive
   and DDLm names (`_atom_site.fract_x`) equal
   their CIF 1 spelling; `data_` lines inside text fields are text; save
   frames are skipped.
3. **Extraction.** The cell, the space group as the file names it, the atom
   rows (label, type symbol, coordinates, occupancy and its uncertainty, B or
   U, calc flag, multiplicity), the atom types' oxidation numbers and the
   anisotropic displacements, joined by label.
4. **Symmetry.** Operations from the file or from a space-group table, and a
   record of where they came from (§6). Every operation must be an
   isometry of the cell: a cell without the symmetry of its operations (a
   cubic name on an orthorhombic cell, COD 5910128) is refused, and one
   that departs from it by less than 1 % (an angle of 90.06° under a
   two-fold axis) is read as given and reported (§9).
5. **Expansion.** Rows merge into sites, sites expand into the atoms of the
   cell, by the rules of §6.
6. **Diagnostics.** Coded findings attached to the structure rather than
   printed (§9). Consumers decide what to show.
7. **Writing.** A structure is checked as the reader checks a file and
   written as one CIF 1.1 data block by the rules of §7.

## 6. The reading

cifmi reads every file one way; what the file leaves open is settled by
the rules below and reported, never chosen quietly, and what it cannot read
is an error with the reason. §2 holds the evidence behind each rule;
nothing falls back to another reading.

| Rule | What cifmi does |
| --- | --- |
| Blocks | `data_` headings outside text fields; the first block that yields a structure |
| Lexer, data names | CIF 1.1 and 2.0 syntax; case-insensitive and DDLm names; the last duplicate wins and is reported |
| Missing values | the CIF dictionary's defaults: occupancy 1 (also for `.`), and cell angles 90° (reported). Rows listing only placeholder coordinates (`? ? ?`) are skipped and reported |
| Unreadable numbers | coordinates, cell and occupancy an error; displacement ignored and reported; an unreadable oxidation number leaves its atom type without one, the others keeping theirs, and is reported |
| Symmetry | listed operations: every one must be readable, and together they must close as a group, else an error; translations within 0.001 of a multiple of 1/24 (`0.3333`) are read as it; a repeated operation is dropped and reported. Else the settings table by Hall symbol, Hermann-Mauguin symbol (with `:1`, `:2`, `:S`, `:Z`, `:H`, `:R`, `e` glides, and the old cubic `F d 3 m` for `F d -3 m`; a short monoclinic symbol on unique axis b where the table knows it so, on the axis `:a`, `:b` or `:c` names, and otherwise on every axis that has it: `P 21/b` is `P 1 1 21/b` or `P 21/b 1 1`), then number; nothing given: P1, reported. A name the table lacks is an error; names that disagree with each other are reported and the settings of each stay candidates for the atoms to settle (next row); names that disagree with the listed operations are reported |
| Setting left open | the settings the name admits, and the other origin choice or axes of a setting it names exactly, settled by the atoms (an expanded listing fits only its own setting; otherwise the setting without colliding atoms whose orbits match the listed multiplicities), then the cell's declared contents (`_chemical_formula_sum` × `_cell_formula_units_Z`, within 1.5 %), then the cell (rhombohedral or hexagonal axes; the unique axis of a monoclinic cell), then the setting the file names, then the table's reference setting; the choice is reported |
| Elements | the type symbol, else the label: the first two characters in title case when they name an element (`FE1` is Fe), else the first alone (`O2-` is O), else a capital with its lowercase letters; `Ow` and `Hw` are water's O and H; the pseudo-atoms `Wat`, `OH` and `OH2` are read as O and `NO3` as N, each reported; deuterium stays D; a symbol naming no element is an error |
| Coordinates | values within 1e-4 (relative) of 1/3 or 2/3 snap to them. Images within the site tolerance are one atom, onto their mean over the subgroup the operations that bring them there generate (a tolerance can cut between two mirror images of a site; the subgroup restores the other). Images within 0.05 Å are one atom on the special position where the site's occupancy forbids separate atoms there; where it allows them, the file's site symmetry order or multiplicity decides, then the cell's declared contents (the formula × Z: the reading that holds them, where every reading holding them agrees; such sites are settled together, up to 16 at a time), and failing those the distance does (on it within 0.015 Å, an atom split around it beyond), reported |
| Rows on one position | merged up to symmetry, their occupancies adding, except that a row repeats its site's atom, read once and reported, when the listing is already expanded or when adding it would fill the site past the occupancy tolerance with an equal occupancy |
| Expansion | per site, exact orbits |
| Dummy atoms | skipped (`_atom_site_calc_flag dum`), reported |
| Occupancy above 1 | a site total up to 1.1 is scaled to a full site and reported; beyond, an error, a hydrogen placed on its oxygen included |
| Displacement | B, or 8π²U, or U_eq from anisotropic U when no isotropic value is listed; a nonpositive value is ignored and reported |
| Modulated and magnetic structures | an error |
| Checks | listed multiplicities against the orbits (a column of site-symmetry orders under `_atom_site_symmetry_multiplicity`, as SHELXL-97 and -93 write it, is read as orders); atoms closer than 0.5 Å holding more than one atom between them (reported) |

The tolerances are `CifOptions`: the site tolerance (1e-4, fractional, per
axis), the thirds tolerance (1e-4, relative), the occupancy tolerance (1.1)
and the special-position distance (0.05 Å).

## 7. The writing

cifmi writes a structure, read or built from its parts, as one CIF 1.1 data
block in the core dictionary's current names, never the older `_symmetry_*`
ones. It writes the structure as it is, in its own setting, axes and origin
with its own coordinates: split sites stay split, and nothing is merged,
moved or re-standardized. A structure it cannot write as a CIF that it reads
is an error with the reason.

| Rule | What cifmi writes |
| --- | --- |
| Block | `#\#CIF_1.1`, then `data_` and the structure's name, which must be a block code: printable ASCII without spaces, at most 75 characters |
| Space group | `_space_group_IT_number`, `_space_group_name_H-M_alt` and `_space_group_name_Hall` of the setting the operations are in, the first in the table of settings with the same operations up to lattice translations, whatever the file called it: the setting's symbol with its origin choice or axes where the group has a choice (`P 1 21/n 1`, `F d -3 m :2`, `R -3 m :H`), and its Hall symbol (`-P 2yn`). Operations in no setting of the table (a primitive cell of a centred group, a supercell, an origin no setting uses) are written without names |
| Cell | `_cell_length_a` to `_cell_angle_gamma`, as the structure has them |
| Contents | `_chemical_formula_sum` in Hill order (C, H, then the other elements alphabetically; without carbon, all alphabetically) and `_cell_formula_units_Z`, from the multiplicities and the occupancies as written: Z is the greatest common divisor of the amounts where all are whole to the written precision, else 1 |
| Operations | `_space_group_symop_id` and `_space_group_symop_operation_xyz`: every operation, centring included, in the structure's order, as `-x+1/2, y, -z+3/4`; translations that are multiples of 1/24 as fractions, others as decimals |
| Atom sites | a row per species of each site, in order: `_atom_site_label`, `_type_symbol`, `_symmetry_multiplicity` (the size of the site's orbit), `_fract_x`, `_y`, `_z` and `_occupancy`, with `_atom_site_B_iso_or_equiv` where a species has B (`?` for those without). A site's first row carries its label; a label already used takes the first of `_2`, `_3`... that no row has |
| Oxidation states | where a species has one, the type symbols carry them (`Fe3+`, `O2-`), and `_atom_type_symbol` and `_atom_type_oxidation_number` list every type, at 0 for those without |
| Numbers | the cell, coordinates, occupancies and B to 8 decimals; a coordinate that rounds to 1 or -0 as 0 |
| Values | bare where CIF 1.1 allows, else in quotes, else a text field |
| Refused | a name that is no block code; a cell the reader refuses; operations that repeat one, do not form a group, or whose symmetry the cell does not have; no sites; a site without species, labelled other than its first species, or at coordinates that are not finite; two sites on one position up to symmetry, within the site tolerance, or a species twice on a site, which reading would make one; an occupancy outside (0, 1], or a site's adding up to more than 1; B below 0 or not finite; an oxidation state that is not finite; characters outside CIF 1.1's set; a line longer than 2,048 characters |

The multiplicities tell the reader how each site was read (§6): a site split
around a special position reads back split, without the cell's declared
contents. A structure built from its parts (§8) is written as it is given,
and its text is read by the rules of §6: a coordinate within the tolerance
of a third, for one, snaps to it.

Evidence: the whole COD archive, 521,901 files, of which cifmi reads 519,196
(§2). Each structure is written, read back, compared with the structure it
was written from, and written again. 517,070 read back as the same
structure, with no new diagnostic, and write again as the same text. 2,107
have repeated labels, written unique (COD 2001310's second `H9aA` as
`H9aA_2`). 14 have a site whose reading is not a fixed point of the reading:
a coordinate within 1e-4 of a third on a special position that ties it to
another axis (COD 1528562's F1 on (x, ½ − x, ¼) of `R -3 c :R`, listed at
(−0.1667, 0.6667)). The reader snaps 0.6667 to ⅔, then averages the site
onto its special position, which moves it off ⅔ again, so each reading moves
the site half way on to (−⅙, ⅔), here by 4e-6 to 2e-5. 5 are refused: two
sites the file lists apart, which the reading snaps onto one position (COD
7234017's `N1` and `N1'`, both onto (x, 2x, ¼)), and which reading the
written text would make one site. 300 have operations in no setting of the
table, their origins shifted from every setting (COD 9002552,
`C m c m (a-1/4,b-1/4,c)`), and are written without names; they read back
unchanged. A random sample of 50,000 of the files (seed 20260111) gives the
same picture: 49,714 read, 49,512 unchanged, 198 with repeated labels and 4
sites that are no fixed point. On one thread over the sample, reading takes 579 µs
per file and writing 124 µs per structure; reading the written text back
takes 316 µs.

## 8. Data model and APIs

```rust
pub fn read_cif(path, &CifOptions) -> Result<Structure, Error>;
pub fn read_cif_str(text, &CifOptions) -> Result<Structure, Error>;
pub fn write_cif(path, &Structure) -> Result<(), Error>;
pub fn write_cif_str(&Structure) -> Result<String, Error>;

pub struct Structure {
    pub name: String,                 // data block
    pub cell: CellParameters,         // as read
    pub lattice: Lattice,             // c along z, a in the xz plane
    pub space_group: SpaceGroupNames, // number, Hermann-Mauguin, Hall, as written
    pub symmetry_source: SymmetrySource, // listed, table, P1, or given
    pub sites: Vec<Site>,             // asymmetric unit: label, position, occupants
    pub operations: Vec<SymOp>,
    pub atoms: Vec<Atom>,             // every atom of the cell: site index, position
    pub diagnostics: Vec<Diagnostic>,
}

impl Structure {
    pub fn new(name, CellParameters, Vec<SymOp>, Vec<Site>) -> Result<Structure, Error>;
}
```

An occupant is an element with its oxidation state, occupancy, B and the
label of the row it comes from; a site's label is its first occupant's.
`Structure::new` builds a structure from a crystal model's parts as they
are: the lattice from the cell and the atoms as the sites' orbits, with no
names, the symmetry source `Given` and no diagnostics. It checks the parts
as the writer does (§7). The Node addon (`readCifFile`, off the main
thread, and `readCifStr`) returns the same structure as plain objects
(lattice parameters, sites with species, row-major operations, space-group
names), plus the diagnostics; `writeCifFile`, off the main thread, and
`writeCifStr` write any object of that shape, of which they read the name,
cell, operations and sites. The Python package `cifmi`
(`read_cif(path, *, block=None)` and `read_cif_str`) returns it as frozen
objects with the same fields; `Structure(name, cell, operations, sites)`
builds one from `Cell`, `Operation`, `Site` and `Species`, and
`write_cif(path, structure)` and `write_cif_str(structure)` write either.

## 9. Diagnostics

Each finding has a stable kind and a message naming the rows or values
involved.

- `no-symmetry`: no operations, names or number; read as P1.
- `setting-chosen`: a name admitting several settings, which was read and
  why.
- `space-group-mismatch`: names that disagree with each other or with the
  listed operations.
- `cell-symmetry-mismatch`: a cell whose lengths or angles depart from the
  symmetry of its operations by less than 1 %, read as given; above that
  the file is refused.
- `duplicate-operation`, `duplicate-data-name`, `skipped-save-frame`.
- `default-value`: a cell angle the file leaves out, read as 90°.
- `placeholder-row`, `dummy-atom`, `pseudo-atom`, `zero-occupancy`: rows
  skipped, or read as another element.
- `repeated-atom`: rows repeating their site's atom, read once.
- `overfull-site`: a site total above 1, up to 1.1, scaled to a full site.
- `ambiguous-special-position`: partly occupied sites near a special
  position that the file places neither on it nor off it, settled by the
  cell's declared contents or, failing those, read by distance.
- `ignored-displacement`: negative or unreadable displacements, read as none.
- `ignored-oxidation-state`: oxidation numbers that cannot be read; their
  atom types are read without one.
- `multiplicity-mismatch`: listed multiplicities that differ from the orbits.
- `colliding-atoms`: atoms closer than 0.5 Å holding more than one atom
  between them.

Declared formula and Z are not checked here: missing hydrogens and
unmodelled solvent make them disagree in sound files. The validation
program uses them (§10).

## 10. Validation program

- **Evidence:** random COD samples and every file cifmi flags (§2); every
  category of reading in doubt is explained and recorded, and the file's
  declared formula × Z and the crystallographic rules decide (§2).
- **Round trip:** every structure read from the fixtures and from random
  COD samples is written, read back, compared with itself and written again
  as the same text (§7). The fixtures' written texts (`tests/data/written`)
  are compared byte for byte in Rust, Python and Node.js.
- **Bit-identity across refactors:** a fingerprint of every lattice, site,
  operation count and error message on the 6,000-file sample, compared
  before and after each change to the reader.
- **Correctness:** cifmi scored by objective checks rather than by another
  reader: listed multiplicities, declared formula and Z, space-group
  closure, no colliding atoms, occupancies at most 1.
- **Robustness:** corrupted and truncated files; no panics, bounded time and
  memory.
- **Fixtures:** every quirk found becomes a fixture with its provenance (COD
  entries are public domain).

## 11. Performance

Most of the time goes to a few multi-megabyte files with embedded reflection
lists. Planned: skip `_refln_` and `_pd_` loops unless asked for, and avoid
per-token allocation. Batch parallelism stays with the caller.

## 12. Decisions

Settled:

- **Plain Node objects.** The addon returns plain objects that survive JSON
  and structured cloning.
- **No WebAssembly build for now.** A native addon serves Node.js; the crate
  compiles to WebAssembly (timed in §2) should a browser need the reader.
- **Symmetry tables, not a symmetry library.** The checks cifmi needs
  (operations that close, atoms that fit, settings settled by the cell) are
  geometric, and the names it writes are the table's. gemmi's table of
  settings is compiled in; finding the symmetry of a structure is left to
  symmetry libraries.
- **One reading.** Nothing in cifmi falls back to another reading, and no
  other reader is consulted.
- **The reading's rule:** what is crystallographically right, with every
  choice the file leaves open reported (§6), checked case by case on the
  evidence (§2).
- **Overfull sites.** A site total up to 1.1 is scaled to a full site and
  reported; beyond it, an error. That reads the 24 sample files over by
  rounding or refinement and rejects the 7 with errors in them (§2).
- **Split sites near special positions.** Reviewed as "always snap within
  0.05 Å, and report"; revised on the evidence of the declared formulas (§2),
  which show one partly occupied site 0.021 Å from a special position to be
  an atom split around it. Sites are snapped where the occupancy forbids
  separate atoms; where it allows them, the file's site symmetry order or
  multiplicity decides, and failing those a distance of 0.015 Å, reported.
- **Settings the file leaves open** are settled by the atoms, then the cell,
  then the setting the file names, then the table's
  reference setting, and the choice is reported, rather than refusing the
  file. A setting the file names exactly is still checked against the atoms:
  one sample file names origin choice 1 of F d -3 m with origin choice 2's
  coordinates.
- **CIF 2.0 and magnetic CIFs.** CIF 2.0 syntax is read; magnetic structures
  are rejected.
- **Standard CIF 1.1, in the structure's own setting.** The writer writes
  the core dictionary's current names and keeps the structure as it is:
  nothing is re-standardized, merged or moved, and nothing falls back. A
  structure the reader would refuse, or read as another (two sites on one
  position, a species twice on a site), is refused with the reason rather
  than written.
- **Names from the table of settings, for the operations written.** The
  names are the table's for the setting the operations are in, whatever the
  file called it, so they always describe the operations written and reading
  finds no mismatch. Operations in no setting of the table are written
  without names: the operations define the space group, any name would
  disagree with them on reading, and finding another setting for them is a
  symmetry library's work (the tables decision above). gemmi's symbols are
  written as they are: `C m c a` where the International Tables now write
  `C m c e`, and the settings that close its table outside the
  International Tables' list (`C -1`, `I -1`, `F 1 2/m 1`).
- **8 decimals.** Enough for a site on a special position to read back onto
  it, the reader snapping what rounding leaves (§6). The contents and Z are
  computed from the occupancies as written, so writing again gives the same
  text.
- **A row per species, with its site's multiplicity.** The multiplicity
  settles on reading what the cell's declared contents or the distance
  settled the first time (§6).
- **Unique labels.** A label names its row; a repeated one takes the first
  free `_2`, `_3`... suffix.
- **B as B.** The structure holds B (8π²U), written as
  `_atom_site_B_iso_or_equiv` without a conversion; `?` where a species has
  none.
- **Oxidation states as atom types.** The type symbol carries the state, and
  every type is listed with its number, 0 for those without. Reading takes a
  type listed under its own symbol at its own number (`Fe` beside `Fe3+`),
  where it took `Fe3+`'s before.
- **Structures built from their parts** (`Structure::new`, Python's
  `Structure`, Node.js's plain objects) are checked as the writer checks a
  structure, so a built structure can be written; their text is read by the
  reading's rules (§6).
