"""Python API tests: the fixtures, the reading's fields and diagnostics, errors.

From the repository root, ``python -m pip install -e ".[test]"`` builds the
extension; then run ``python -m pytest tests``.
"""

from collections import Counter
from pathlib import Path

import cifmi
import pytest

DATA = Path(__file__).parent / "data" / "cif"


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
