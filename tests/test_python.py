"""Python API tests: the fixtures, the reading's fields and diagnostics, errors,
and the writing, byte for byte as the Rust and Node.js tests compare it.

From the repository root, ``python -m pip install -e ".[test]"`` builds the
extension; then run ``python -m pytest tests``.
"""

from collections import Counter
from pathlib import Path

import cifmi
import pytest

DATA = Path(__file__).parent / "data" / "cif"
# Each fixture cifmi reads, as it writes it.
WRITTEN = sorted((Path(__file__).parent / "data" / "written").glob("*.cif"))


def atoms_by_species(structure: cifmi.Structure) -> Counter:
    """(element, occupancy, position rounded to 1e-6) for every atom."""
    out = Counter()
    for atom in structure.atoms:
        position = tuple(round(v % 1.0, 6) % 1.0 for v in atom.fractional)
        for species in structure.sites[atom.site].species:
            out[(species.element, round(species.occupancy, 6), position)] += 1
    return out


@pytest.mark.parametrize("path", sorted(DATA.glob("*.cif")), ids=lambda p: p.name)
def test_every_fixture_reads_or_refuses_with_a_reason(path):
    if path.name == "cod__2100690.cif":
        with pytest.raises(ValueError, match="modulated"):
            cifmi.read_cif(path)
        return
    structure = cifmi.read_cif(path)
    assert structure.atoms and structure.sites and structure.operations
    assert sum(atoms_by_species(structure).values()) == sum(
        len(structure.sites[atom.site].species) for atom in structure.atoms
    )


def test_reading_and_its_diagnostics():
    # The Hall symbol names origin choice 1; the atoms are in origin choice 2.
    structure = cifmi.read_cif(DATA / "cod__2003493.cif")
    assert structure.name == "2003493"
    assert structure.cell.a == pytest.approx(10.1429)
    assert structure.space_group.symbol == "F d 3 m"
    assert structure.symmetry_source.kind == "table"
    assert (len(structure.sites), len(structure.atoms), len(structure.operations)) == (
        4,
        88,
        192,
    )
    assert [d.kind for d in structure.diagnostics] == ["setting-chosen"]
    assert "88 atoms" in repr(structure)


def test_text_reads_as_the_file_does():
    path = DATA / "cod__5910103.cif"
    from_file = cifmi.read_cif(path)
    from_text = cifmi.read_cif_str(path.read_text())
    assert atoms_by_species(from_file) == atoms_by_species(from_text)


def test_species_sharing_a_site_keep_their_rows_labels():
    text = """data_mixed
_cell_length_a 5
_cell_length_b 6
_cell_length_c 7
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_occupancy
Fe1 Fe 0.1 0.2 0.3 0.6
Mn1 Mn 0.1 0.2 0.3 0.4
"""
    site = cifmi.read_cif_str(text).sites[0]
    assert [(s.label, s.element, s.occupancy) for s in site.species] == [
        ("Fe1", "Fe", 0.6),
        ("Mn1", "Mn", 0.4),
    ]


def test_errors_are_raised_not_papered_over(tmp_path):
    with pytest.raises(OSError):
        cifmi.read_cif(tmp_path / "missing.cif")
    with pytest.raises(ValueError, match="no data block named"):
        cifmi.read_cif(DATA / "cod__2003493.cif", block="other")
    with pytest.raises(ValueError):
        cifmi.read_cif_str("not a CIF\n")
    # A modulated structure is refused rather than read as its average.
    with pytest.raises(ValueError, match="modulated"):
        cifmi.read_cif(DATA / "cod__2100690.cif")


def plain(structure: cifmi.Structure) -> cifmi.Structure:
    """``structure`` built again from plain numbers, strings and lists."""
    cell = structure.cell
    return cifmi.Structure(
        structure.name,
        cifmi.Cell(cell.a, cell.b, cell.c, cell.alpha, cell.beta, cell.gamma),
        [
            cifmi.Operation([list(row) for row in op.rotation], list(op.translation))
            for op in structure.operations
        ],
        [
            cifmi.Site(
                site.label,
                list(site.fractional),
                [
                    cifmi.Species(s.label, s.element, s.occupancy, s.oxidation, s.u_iso)
                    for s in site.species
                ],
            )
            for site in structure.sites
        ],
    )


@pytest.mark.parametrize("expected", WRITTEN, ids=lambda p: p.name)
def test_every_fixture_is_written_byte_for_byte_and_reads_back(expected):
    text = cifmi.write_cif_str(cifmi.read_cif(DATA / expected.name))
    assert text == expected.read_bytes().decode("ascii")
    assert cifmi.write_cif_str(cifmi.read_cif_str(text)) == text
    # Built again from plain data, the structure is written the same.
    built = plain(cifmi.read_cif(DATA / expected.name))
    assert built.symmetry_source.kind == "given"
    assert cifmi.write_cif_str(built) == text


def test_written_files_read_back(tmp_path):
    structure = cifmi.read_cif(DATA / "cod__7046814.cif")
    path = tmp_path / "written.cif"
    assert cifmi.write_cif(path, structure) is None
    assert path.read_bytes().decode("ascii") == cifmi.write_cif_str(structure)
    assert len(cifmi.read_cif(path).atoms) == len(structure.atoms)


IDENTITY = cifmi.Operation([[1, 0, 0], [0, 1, 0], [0, 0, 1]], [0, 0, 0])
INVERSION = cifmi.Operation([[-1, 0, 0], [0, -1, 0], [0, 0, -1]], [0, 0, 0])


def test_a_structure_built_from_its_parts():
    built = cifmi.Structure(
        "mixed",
        cifmi.Cell(5, 6, 7, 90, 100, 90),
        [IDENTITY, INVERSION],
        [
            cifmi.Site(
                "M1",
                (0, 0, 0),
                [
                    cifmi.Species("M1", "Fe", 0.5, oxidation=3),
                    cifmi.Species("Mn1", "Mn", 0.5),
                ],
            ),
            cifmi.Site(
                "O1", (0.25, 0.1, 0.3), [cifmi.Species("O1", "O", 1, u_iso=0.0152)]
            ),
        ],
    )
    # The site at the inversion centre is one atom, the other two.
    assert [atom.site for atom in built.atoms] == [0, 1, 1]
    assert built.space_group.number is None
    assert (built.symmetry_source.kind, built.diagnostics) == ("given", [])
    text = cifmi.write_cif_str(built)
    assert "_space_group_name_H-M_alt   'P -1'\n" in text
    assert "  M1  Fe3+  1  0.00000000  0.00000000  0.00000000  0.50000000  ?\n" in text
    assert " _atom_site_U_iso_or_equiv\n" in text
    assert (
        "  O1  O  2  0.25000000  0.10000000  0.30000000  1.00000000  0.01520000\n"
        in text
    )
    read = cifmi.read_cif_str(text)
    assert [
        (s.label, s.element, s.occupancy, s.oxidation) for s in read.sites[0].species
    ] == [
        ("M1", "Fe", 0.5, 3.0),
        ("Mn1", "Mn", 0.5, 0.0),
    ]
    assert read.sites[1].species[0].u_iso == 0.0152


def test_structures_that_cannot_be_written_are_refused_with_the_reason():
    with pytest.raises(ValueError, match="unknown element symbol"):
        cifmi.Species("X1", "Xx", 1)
    full = [cifmi.Species("Na1", "Na", 0.6), cifmi.Species("K1", "K", 0.6)]
    with pytest.raises(
        ValueError, match="site Na1: occupancies add up to 1.2, more than 1"
    ):
        cifmi.Structure(
            "over",
            cifmi.Cell(5, 5, 5, 90, 90, 90),
            [IDENTITY],
            [cifmi.Site("Na1", (0, 0, 0), full)],
        )
    with pytest.raises(ValueError, match="cannot be a data block code"):
        cifmi.Structure(
            "two words",
            cifmi.Cell(5, 5, 5, 90, 90, 90),
            [IDENTITY],
            [cifmi.Site("Na1", (0, 0, 0), full[:1])],
        )
