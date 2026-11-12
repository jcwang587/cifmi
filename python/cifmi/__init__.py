"""Read CIF files into crystal structures, and write crystal structures as CIF.

cifmi is a CIF reader and writer written in Rust. It reads a file as the CIF
standard and its symmetry demand, reporting what the file left open in
``diagnostics``, and writes a structure as CIF 1.1 that it reads back as the
same structure.

Example::

    import cifmi

    structure = cifmi.read_cif("quartz.cif")
    structure.cell.a, structure.space_group.symbol, len(structure.atoms)
    cifmi.write_cif("quartz-copy.cif", structure)

A :class:`Structure` keeps the asymmetric unit (``sites``, rows on one
position merged), the symmetry ``operations`` and every atom of the unit cell
(``atoms``, each an image of a site). Lengths are in Å, angles in degrees,
displacements B in Å², occupancies as site fractions. A structure can also be
built from its parts, ``Structure(name, cell, operations, sites)``, with
:class:`Cell`, :class:`Operation`, :class:`Site` and :class:`Species`.
"""

from cifmi._core import (
    Atom,
    Cell,
    Diagnostic,
    Operation,
    Site,
    SpaceGroup,
    Species,
    Structure,
    SymmetrySource,
    __version__,
    read_cif,
    read_cif_str,
    write_cif,
    write_cif_str,
)

__all__ = [
    "Atom",
    "Cell",
    "Diagnostic",
    "Operation",
    "Site",
    "SpaceGroup",
    "Species",
    "Structure",
    "SymmetrySource",
    "__version__",
    "read_cif",
    "read_cif_str",
    "write_cif",
    "write_cif_str",
]
