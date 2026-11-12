//! The writing: every fixture written as `data/written` holds it (which the
//! Python and Node.js tests compare byte for byte), read back as the same
//! structure and written again as the same text; the names, the layout, and
//! the structures that cannot be written.

use cifmi::{
    read_cif, read_cif_str, write_cif, write_cif_str, CellParameters, CifOptions, Element,
    Occupant, Site, Structure, SymOp, SymmetrySource,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn data(directory: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(directory)
}

fn read_text(text: &str) -> Structure {
    read_cif_str(text, &CifOptions::default()).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-8 * a.abs().max(1.0)
}

/// The same fractional position, up to a lattice translation, to the written
/// precision.
fn same_position(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| {
        let d = a[i] - b[i];
        (d - d.round()).abs() <= 1e-8
    })
}

/// `read`, the reading of `written`'s text, is the same structure to the
/// written precision: its cell, operations, sites and atoms, with names that
/// are all given or all absent, and no diagnostic `written` was read without.
fn assert_same(written: &Structure, read: &Structure) {
    assert_eq!(read.name, written.name);
    let (w, r) = (written.cell, read.cell);
    for (x, y) in [
        (w.a, r.a),
        (w.b, r.b),
        (w.c, r.c),
        (w.alpha, r.alpha),
        (w.beta, r.beta),
        (w.gamma, r.gamma),
    ] {
        assert!(close(x, y), "cell {w:?} read back as {r:?}");
    }
    assert_eq!(read.operations, written.operations);
    let names = &read.space_group;
    assert_eq!(names.number.is_some(), names.hermann_mauguin.is_some());
    assert_eq!(names.number.is_some(), names.hall.is_some());
    assert_eq!(
        read.symmetry_source,
        SymmetrySource::Listed {
            tag: "_space_group_symop_operation_xyz".into()
        }
    );
    assert_eq!(read.sites.len(), written.sites.len());
    for (w, r) in written.sites.iter().zip(&read.sites) {
        assert_eq!(r.label, w.label);
        assert!(
            same_position(w.position, r.position),
            "{}: {:?} read back as {:?}",
            w.label,
            w.position,
            r.position
        );
        assert_eq!(r.occupants.len(), w.occupants.len(), "{}", w.label);
        for (a, b) in w.occupants.iter().zip(&r.occupants) {
            assert_eq!(
                (&b.label, b.element, b.oxidation),
                (&a.label, a.element, a.oxidation)
            );
            assert!(close(a.occupancy, b.occupancy), "{a:?} read as {b:?}");
            assert!(close(a.b_iso, b.b_iso), "{a:?} read as {b:?}");
        }
    }
    assert_eq!(read.atoms.len(), written.atoms.len());
    for (w, r) in written.atoms.iter().zip(&read.atoms) {
        assert_eq!(r.site, w.site);
        assert!(same_position(w.position, r.position), "{w:?} read as {r:?}");
    }
    for d in &read.diagnostics {
        assert!(
            written.diagnostics.iter().any(|w| w.kind == d.kind),
            "a new diagnostic: {d:?}"
        );
    }
}

/// `structure` written, read back as itself, and written again as the same
/// text, which is returned.
fn round_trip(structure: &Structure) -> String {
    let text = write_cif_str(structure).unwrap();
    let read = read_text(&text);
    assert_same(structure, &read);
    assert_eq!(write_cif_str(&read).unwrap(), text, "written again");
    text
}

#[test]
fn every_fixture_is_written_as_expected_and_reads_back_as_itself() {
    // CIFMI_UPDATE_WRITTEN=1 writes the expected texts anew, for a change to
    // the writing; review their diff.
    let update = std::env::var_os("CIFMI_UPDATE_WRITTEN").is_some();
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(data("cif"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    fixtures.sort();
    let mut written = BTreeSet::new();
    for path in fixtures {
        let name = path.file_name().unwrap().to_owned();
        // The modulated structure is refused, so there is nothing to write.
        let Ok(structure) = read_cif(&path, &CifOptions::default()) else {
            continue;
        };
        let text = round_trip(&structure);
        let expected = data("written").join(&name);
        if update {
            std::fs::write(&expected, &text).unwrap();
        }
        let expected_text = std::fs::read_to_string(&expected).unwrap();
        assert!(text == expected_text, "{name:?} is written otherwise");
        written.insert(name);
    }
    let expected: BTreeSet<_> = std::fs::read_dir(data("written"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(expected, written);
}

/// A P -1 structure with a mixed site, oxidation states and B.
fn mixed() -> Structure {
    let occupant = |label: &str, symbol: &str, oxidation, occupancy, b_iso| Occupant {
        label: label.into(),
        element: Element::from_symbol(symbol).unwrap(),
        oxidation,
        occupancy,
        b_iso,
    };
    let site = |label: &str, position, occupants| Site {
        label: label.into(),
        position,
        occupants,
    };
    Structure::new(
        "mixed",
        CellParameters {
            a: 5.0,
            b: 6.0,
            c: 7.0,
            alpha: 90.0,
            beta: 100.0,
            gamma: 90.0,
        },
        vec![SymOp::IDENTITY, SymOp::parse("-x,-y,-z").unwrap()],
        vec![
            site(
                "M1",
                [0.0, 0.0, 0.0],
                vec![
                    occupant("M1", "Fe", 3.0, 0.5, 0.5),
                    occupant("Mn1", "Mn", 2.0, 0.5, 0.0),
                ],
            ),
            site(
                "O1",
                [0.25, 0.1, 1.0 / 3.0],
                vec![occupant("O1", "O", -2.0, 1.0, 1.2)],
            ),
            site(
                "O2",
                [0.6, -0.7, 0.999999999],
                vec![occupant("O2", "O", 0.0, 1.0, 0.0)],
            ),
            site(
                "H 1",
                [0.1, 0.2, 0.3],
                vec![occupant("H 1", "D", 0.0, 0.25, 0.0)],
            ),
        ],
    )
    .unwrap()
}

#[test]
fn a_structure_is_written_in_the_core_dictionarys_names() {
    let text = round_trip(&mixed());
    let expected = "#\\#CIF_1.1
data_mixed
_space_group_IT_number   2
_space_group_name_H-M_alt   'P -1'
_space_group_name_Hall   '-P 1'
_cell_length_a   5.00000000
_cell_length_b   6.00000000
_cell_length_c   7.00000000
_cell_angle_alpha   90.00000000
_cell_angle_beta   100.00000000
_cell_angle_gamma   90.00000000
_chemical_formula_sum   'D0.5 Fe0.5 Mn0.5 O4'
_cell_formula_units_Z   1
loop_
 _space_group_symop_id
 _space_group_symop_operation_xyz
  1  'x, y, z'
  2  '-x, -y, -z'
loop_
 _atom_type_symbol
 _atom_type_oxidation_number
  Fe3+  3
  Mn2+  2
  O2-  -2
  O  0
  D  0
loop_
 _atom_site_label
 _atom_site_type_symbol
 _atom_site_symmetry_multiplicity
 _atom_site_fract_x
 _atom_site_fract_y
 _atom_site_fract_z
 _atom_site_occupancy
 _atom_site_B_iso_or_equiv
  M1  Fe3+  1  0.00000000  0.00000000  0.00000000  0.50000000  0.50000000
  Mn1  Mn2+  1  0.00000000  0.00000000  0.00000000  0.50000000  ?
  O1  O2-  2  0.25000000  0.10000000  0.33333333  1.00000000  1.20000000
  O2  O  2  0.60000000  -0.70000000  0.00000000  1.00000000  ?
  'H 1'  D  2  0.10000000  0.20000000  0.30000000  0.25000000  ?
";
    assert_eq!(text, expected);
}

#[test]
fn a_structure_built_from_its_parts_has_its_atoms_and_no_names() {
    let built = mixed();
    assert_eq!(built.symmetry_source, SymmetrySource::Given);
    assert_eq!(built.space_group, Default::default());
    assert!(built.diagnostics.is_empty());
    assert!(close(
        built.lattice.volume(),
        5.0 * 6.0 * 7.0 * 100f64.to_radians().sin()
    ));
    // The site at the inversion centre is one atom; the others are two.
    let per_site: Vec<usize> = (0..built.sites.len())
        .map(|k| built.atoms.iter().filter(|atom| atom.site == k).count())
        .collect();
    assert_eq!(per_site, [1, 2, 2, 2]);
}

#[test]
fn names_are_the_table_names_of_the_setting_the_operations_are_in() {
    let named = |symmetry: &str| {
        let text = format!("data_x\n_cell_length_a 5\n_cell_length_b 5\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 120\n{symmetry}\nloop_\n_atom_site_label\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\nNa1 0.1 0.2 0.3\n");
        let written = round_trip(&read_text(&text));
        let read = read_text(&written);
        let names = read.space_group;
        (
            names.number.unwrap(),
            names.hermann_mauguin.unwrap(),
            names.hall.unwrap(),
        )
    };
    let name =
        |number: u8, symbol: &str, hall: &str| (number, symbol.to_string(), hall.to_string());
    // Whatever the file calls the setting, it is written by the table's names.
    assert_eq!(
        named("_symmetry_space_group_name_H-M 'R -3 m'"),
        name(166, "R -3 m :H", "-R 3 2\"")
    );
    assert_eq!(
        named("_space_group_name_Hall '-P 3 2\"'"),
        name(164, "P -3 m 1", "-P 3 2\"")
    );
    assert_eq!(
        named("loop_\n_symmetry_equiv_pos_as_xyz\nx,y,z\n-y,x-y,z\n-x+y,-x,z"),
        name(143, "P 3", "P 3")
    );
}

#[test]
fn operations_in_no_setting_are_written_without_names() {
    // A doubled P 1 cell: its translation (1/2, 0, 0) is no centring.
    let text = "data_double\n_cell_length_a 10\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 90\nloop_\n_symmetry_equiv_pos_as_xyz\nx,y,z\nx+1/2,y,z\nloop_\n_atom_site_label\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\nNa1 0.1 0.2 0.3\n";
    let structure = read_text(text);
    let written = round_trip(&structure);
    assert!(!written.contains("_space_group_IT_number"), "{written}");
    assert!(!written.contains("_space_group_name"), "{written}");
    assert!(written.contains("  2  'x+1/2, y, z'\n"), "{written}");
    let read = read_text(&written);
    assert_eq!(read.space_group, Default::default());
    assert_eq!(read.atoms.len(), 2);
}

#[test]
fn split_sites_stay_split() {
    // Na 0.024 Å off the mirror y = 0 at half occupancy, read as an atom
    // split around it by the declared contents: written with its
    // multiplicity, it reads back split, and the declared contents are not
    // needed again.
    let text = "data_split\n_cell_length_a 5\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 90\n_chemical_formula_sum 'Na O'\n_cell_formula_units_Z 1\nloop_\n_symmetry_equiv_pos_as_xyz\nx,y,z\nx,-y,z\nloop_\n_atom_site_label\n_atom_site_type_symbol\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\n_atom_site_occupancy\nNa1 Na 0.1 0.004 0.1 0.5\nO1 O 0.5 0.5 0.5 1\n";
    let structure = read_text(text);
    assert_eq!(structure.atoms.len(), 3);
    let written = round_trip(&structure);
    assert!(written.contains("  Na1  Na  2  0.10000000  0.00400000  0.10000000  0.50000000\n"));
    assert!(read_text(&written).diagnostics.is_empty());
}

#[test]
fn repeated_labels_are_made_unique() {
    // Two rows labelled O1 at different positions, and a third labelled
    // O1_2: the second O1 is written as the first label no row has, O1_3.
    let text = cif_with_rows("O1 O 0.1 0.2 0.3\nO1 O 0.4 0.2 0.3\nO1_2 O 0.7 0.2 0.3\n");
    let structure = read_text(&text);
    let written = write_cif_str(&structure).unwrap();
    let labels: Vec<String> = read_text(&written)
        .sites
        .iter()
        .map(|site| site.label.clone())
        .collect();
    assert_eq!(labels, ["O1", "O1_3", "O1_2"]);
    assert_eq!(write_cif_str(&read_text(&written)).unwrap(), written);
}

/// A 5 × 6 × 7 Å P 1 cell with the atom rows `rows` (label, type symbol, x,
/// y, z).
fn cif_with_rows(rows: &str) -> String {
    format!("data_rows\n_cell_length_a 5\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 90\n_space_group_name_H-M_alt 'P 1'\nloop_\n_atom_site_label\n_atom_site_type_symbol\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\n{rows}")
}

#[test]
fn written_files_read_back() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("written.cif");
    let structure = read_cif(data("cif").join("cod__7046814.cif"), &CifOptions::default()).unwrap();
    write_cif(&path, &structure).unwrap();
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    assert_same(&structure, &read);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        write_cif_str(&structure).unwrap()
    );
}

#[test]
fn structures_that_cannot_be_written_are_refused_with_the_reason() {
    let refused = |change: &dyn Fn(&mut Structure), reason: &str| {
        let mut structure = mixed();
        change(&mut structure);
        let err = write_cif_str(&structure).unwrap_err().to_string();
        assert!(err.contains(reason), "{err:?} does not say {reason:?}");
    };
    refused(
        &|s| s.name = "two words".into(),
        "\"two words\" cannot be a data block code: ' ' is not printable ASCII",
    );
    refused(
        &|s| s.name.clear(),
        "cannot be a data block code: it is empty",
    );
    refused(
        &|s| s.name = "x".repeat(76),
        "it is longer than 75 characters",
    );
    refused(
        &|s| s.cell.alpha = 180.0,
        "cell angles must lie strictly between 0 and 180 degrees",
    );
    refused(&|s| s.operations.clear(), "no symmetry operations");
    refused(
        &|s| s.operations.push(SymOp::parse("-x,-y,-z+1").unwrap()),
        "the operations give -x,-y,-z+1 twice",
    );
    refused(
        &|s| s.operations.push(SymOp::parse("x+1/3,y,z").unwrap()),
        "the operations do not form a group",
    );
    refused(
        &|s| s.operations[1] = SymOp::parse("y,x,z").unwrap(),
        "the cell does not have the symmetry of its operations",
    );
    refused(&|s| s.sites.clear(), "no atom sites");
    refused(
        &|s| s.sites[0].label = "Fe1".into(),
        "site Fe1: its first species is labelled M1",
    );
    refused(
        &|s| s.sites[1].position[2] = f64::NAN,
        "site O1: coordinates [0.25, 0.1, NaN] are not finite numbers",
    );
    refused(
        &|s| s.sites[0].occupants[1].occupancy = 0.6,
        "site M1: occupancies add up to 1.1, more than 1",
    );
    refused(
        &|s| s.sites[1].occupants[0].b_iso = -1.0,
        "O1: B -1 is not a finite number of at least 0",
    );
    refused(
        &|s| {
            let mn = &mut s.sites[0].occupants[1];
            (mn.element, mn.oxidation) = (Element::from_symbol("Fe").unwrap(), 3.0);
        },
        "site M1: M1 and Mn1 are one species, which reading adds into one",
    );
    // O2 at the inversion image of O1.
    refused(
        &|s| s.sites[2].position = [0.75, 0.9, 2.0 / 3.0],
        "sites O1 and O2 are on one position, up to symmetry, which reading makes one site",
    );
    refused(
        &|s| s.sites[0].occupants[1].label = "Ø1".into(),
        "\"Ø1\": 'Ø' is not in CIF 1.1's character set",
    );
    // Structure::new refuses what cannot be written, the same way.
    let mut parts = mixed();
    parts.sites[0].occupants[0].occupancy = 0.0;
    let err = Structure::new(parts.name, parts.cell, parts.operations, parts.sites)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("M1: occupancy 0 is not above 0 and at most 1"),
        "{err}"
    );
}
