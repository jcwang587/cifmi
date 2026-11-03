"""Read CIF files into crystal structures.

cifmi is a CIF reader written in Rust. It reads a file as the CIF
standard and its symmetry demand, reporting what the file left open in
``diagnostics``.

Example::

    import cifmi

    structure = cifmi.read_cif("quartz.cif")
    structure.cell.a, structure.space_group.symbol, len(structure.atoms)

A :class:`Structure` keeps the asymmetric unit (``sites``, rows on one
position merged), the symmetry ``operations`` and every atom of the unit cell
(``atoms``, each an image of a site). Lengths are in Å, angles in degrees,
displacements B in Å², occupancies as site fractions.
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
]
