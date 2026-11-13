//! The reading on real files and on small constructed ones: what cifmi
//! reads and what it reports.

use cifmi::{read_cif, read_cif_str, CifOptions, DiagnosticKind, Structure, SymmetrySource};
use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/cif")
        .join(name)
}

/// Occupancy summed over every atom of the cell, by element.
fn composition(s: &Structure) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for atom in &s.atoms {
        for o in &s.sites[atom.site].occupants {
            *out.entry(o.element.symbol().to_string()).or_insert(0.0) += o.occupancy;
        }
    }
    out.into_iter()
        .map(|(k, v)| (k, (v * 1e6).round() / 1e6))
        .collect()
}

fn formula(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn kinds(s: &Structure) -> Vec<DiagnosticKind> {
    s.diagnostics.iter().map(|d| d.kind).collect()
}

fn read_text(text: &str) -> Structure {
    read_cif_str(text, &CifOptions::default()).unwrap()
}

/// A 5 × 6 × 7 Å orthorhombic cell with `symmetry` and the atom rows `atoms`
/// (label, type symbol, x, y, z, occupancy).
fn cif(symmetry: &str, atoms: &str) -> String {
    format!(
        "data_test\n_cell_length_a 5\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 90\n{symmetry}loop_\n_atom_site_label\n_atom_site_type_symbol\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\n_atom_site_occupancy\n{atoms}"
    )
}

// ---------------------------------------------------------------------------
// Syntax.

#[test]
fn a_byte_order_mark_is_not_data_before_the_first_block() {
    let text = format!(
        "\u{feff}{}",
        cif("_symmetry_space_group_name_H-M 'P 1'\n", "O1 O 0 0 0 1\n")
    );
    let read = read_text(&text);
    assert_eq!(read.name, "test");
    assert_eq!(read.atoms.len(), 1);
}

#[test]
fn values_beyond_ascii_read_without_aborting() {
    // Each value's fifth byte falls inside a character; the heading check
    // once sliced there and panicked, which aborts the host process.
    let text = cif(
        "_symmetry_space_group_name_H-M 'P 1'\n_chemical_name_common Größe\n_journal_coden_ASTM αβγδε\n",
        "O1 O 0 0 0 1\n",
    );
    assert_eq!(read_text(&text).atoms.len(), 1);
    // A Latin-1 file: read_cif reads its degree sign as U+FFFD, three bytes.
    let latin1 = cif(
        "_symmetry_space_group_name_H-M 'P 1'\n_cell_measurement_temperature 123\u{b0}C\n",
        "O1 O 0 0 0 1\n",
    );
    let bytes: Vec<u8> = latin1.chars().map(|c| c as u8).collect();
    assert_eq!(read_text(&String::from_utf8_lossy(&bytes)).atoms.len(), 1);
}

#[test]
fn a_cell_without_the_symmetry_of_its_operations_is_refused() {
    // COD 5910128 names the cubic P 42 3 2 for a 2.82 × 5.52 × 11.46 Å cell;
    // the group's three-fold axes map one cell edge onto another.
    let text = cif(
        "_symmetry_space_group_name_H-M 'P 42 3 2'\n",
        "O1 O 0.1 0.2 0.3 1\n",
    );
    let err = read_cif_str(&text, &CifOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("the cell does not have the symmetry of its operations")
            && err.contains("maps a (5.0000 Å) onto a vector of 7.0000 Å"),
        "{err}"
    );
}

#[test]
fn a_cell_slightly_off_the_symmetry_of_its_operations_is_read_with_a_note() {
    // Monoclinic operations, b unique, with alpha given as 90.06°: read as
    // given, the departure reported.
    let text = "data_test\n_cell_length_a 5\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90.06\n_cell_angle_beta 90\n_cell_angle_gamma 90\nloop_\n_symmetry_equiv_pos_as_xyz\n'x,y,z'\n'-x,y,-z'\n'-x,-y,-z'\n'x,-y,z'\nloop_\n_atom_site_label\n_atom_site_type_symbol\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\nO1 O 0.1 0.2 0.3\n";
    let read = read_text(text);
    assert_eq!(read.atoms.len(), 4);
    assert_eq!(read.cell.alpha, 90.06);
    assert_eq!(kinds(&read), [DiagnosticKind::CellSymmetryMismatch]);
    let message = &read.diagnostics[0].message;
    assert!(
        message.contains("-x,y,-z changes the angle between b and c from 90.060° to 89.940°"),
        "{message}"
    );
    // The angle the two-fold axis allows to be oblique is no departure.
    let oblique = text.replace(
        "_cell_angle_alpha 90.06\n_cell_angle_beta 90",
        "_cell_angle_alpha 90\n_cell_angle_beta 95",
    );
    assert_eq!(kinds(&read_text(&oblique)), []);
}

#[test]
fn the_cell_settles_the_unique_axis_of_a_short_monoclinic_symbol() {
    // P 21/b exists on axes c and a; gamma is the oblique angle here.
    let text = "data_test\n_cell_length_a 5\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 100\n_symmetry_space_group_name_H-M 'P 21/b'\nloop_\n_atom_site_label\n_atom_site_type_symbol\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\nO1 O 0.1 0.2 0.3\n";
    let read = read_text(text);
    assert_eq!(read.operations.len(), 4);
    assert_eq!(read.atoms.len(), 4);
    assert_eq!(kinds(&read), [DiagnosticKind::SettingChosen]);
    let message = &read.diagnostics[0].message;
    assert!(
        message.contains("P 1 1 21/b is read: the cell's unique axis is c"),
        "{message}"
    );
}

// ---------------------------------------------------------------------------
// Real files.

#[test]
fn anisotropic_displacements_give_u_where_no_isotropic_one_is_listed() {
    let path = fixture("cod__1000126.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    let u = |s: &Structure| -> Vec<f64> {
        s.sites.iter().map(|site| site.occupants[0].u_iso).collect()
    };
    assert!(
        u(&read).iter().all(|&v| v > 0.001 && v < 0.04),
        "{:?}",
        u(&read)
    );
}

#[test]
fn dummy_atoms_are_skipped() {
    let path = fixture("cod__1010642.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    assert_eq!(composition(&read), formula(&[("Mg", 2.0), ("W", 2.0)]));
    assert_eq!(kinds(&read), [DiagnosticKind::DummyAtom]);
}

#[test]
fn a_listing_already_expanded_is_read_once() {
    // Rows such as Br1F* repeat atoms at symmetry images.
    let path = fixture("cod__1100217.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    // C12 H38 Br2 N8 Ni P2, Z = 4.
    let expected = formula(&[
        ("Br", 8.0),
        ("C", 48.0),
        ("H", 152.0),
        ("N", 32.0),
        ("Ni", 4.0),
        ("P", 8.0),
    ]);
    assert_eq!(composition(&read), expected);
    assert_eq!(kinds(&read), [DiagnosticKind::RepeatedAtom]);
}

#[test]
fn the_atoms_settle_an_origin_choice_the_name_gets_wrong() {
    // The Hall symbol names origin choice 1 of F d -3 m; the coordinates are
    // origin choice 2's.
    let path = fixture("cod__2003493.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    // O7 Ru2 Y2, Z = 8.
    assert_eq!(
        composition(&read),
        formula(&[("O", 56.0), ("Ru", 16.0), ("Y", 16.0)])
    );
    assert_eq!(kinds(&read), [DiagnosticKind::SettingChosen]);
    assert!(read.diagnostics[0].message.contains("F d -3 m:2 is read"));
}

#[test]
fn rhombohedral_axes_are_read_from_the_atoms() {
    // Willemite named only R -3, in rhombohedral axes.
    let path = fixture("cod__5910103.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    assert_eq!(read.operations.len(), 6);
    assert_eq!(
        composition(&read),
        formula(&[("O", 24.0), ("Si", 6.0), ("Zn", 12.0)])
    );
    assert!(read.diagnostics[0].message.contains("R -3:R is read"));
}

#[test]
fn a_site_whose_mirror_images_straddle_the_merge_tolerance_is_one_atom() {
    // Sc1 lies on (x, x + 1/2, 0) to the file's four decimals, so its two
    // diagonal-mirror images are 1e-4 away: rounding takes one inside the
    // merge tolerance and leaves the other out. Snapping averages over the
    // subgroup they generate, which puts Sc1 on the special position.
    let path = fixture("cod__4001733.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    // B2 Fe Ir5 Sc2, Z = 2.
    let expected = formula(&[("B", 4.0), ("Fe", 2.0), ("Ir", 10.0), ("Sc", 4.0)]);
    assert_eq!(composition(&read), expected);
    assert!(kinds(&read).is_empty(), "{:?}", kinds(&read));
}

#[test]
fn half_atoms_at_mirror_images_add_up_to_full_ones() {
    // SHELX lists the two H of a CH2 on a mirror at half occupancy each; they
    // add up to full atoms, as the formula says.
    let path = fixture("cod__7046814.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    // C8 H16 Br2 O2 Yb, Z = 2.
    let expected = formula(&[
        ("Br", 4.0),
        ("C", 16.0),
        ("H", 32.0),
        ("O", 4.0),
        ("Yb", 2.0),
    ]);
    assert_eq!(composition(&read), expected);
}

#[test]
fn slightly_overfull_sites_are_scaled_to_full_ones() {
    let path = fixture("cod__9005920.cif");
    let read = read_cif(&path, &CifOptions::default()).unwrap();
    let site = read.sites.iter().find(|s| s.label == "MgM2").unwrap();
    let total: f64 = site.occupants.iter().map(|o| o.occupancy).sum();
    assert!((total - 1.0).abs() < 1e-12, "{total}");
    assert_eq!(kinds(&read), [DiagnosticKind::OverfullSite]);
}

#[test]
fn modulated_structures_are_refused() {
    let path = fixture("cod__2100690.cif");
    let err = read_cif(&path, &CifOptions::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("modulated structures"), "{err}");
}

// ---------------------------------------------------------------------------
// Syntax.

const CSCL_ATOMS: &str = "loop_
_atom_site.label
_atom_site.type_symbol
_atom_site.fract_x
_atom_site.fract_y
_atom_site.fract_z
Cs1 Cs 0 0 0
Cl1 Cl 0.5 0.5 0.5
";

#[test]
fn standard_syntax_ddlm_names_text_fields_and_end_lines() {
    let text = format!(
        "data_ddlm
_Cell.Length_A 4.1
_cell.length_b 4.1
_CELL.LENGTH_C 4.1
_cell.angle_alpha 90
_cell.angle_beta 90
_cell.angle_gamma 90
_space_group.name_H-M_alt 'P m -3 m'
_publ_section_title 'A # in a title'   # a comment
_iucr_refine_fcf_details
;
data_embedded
_refln_index_h 1
;
{CSCL_ATOMS}END
not CIF, after the END line
"
    );
    let read = read_text(&text);
    assert_eq!(read.name, "ddlm");
    assert_eq!(read.cell.a, 4.1);
    assert_eq!(read.operations.len(), 48);
    assert_eq!(composition(&read), formula(&[("Cl", 1.0), ("Cs", 1.0)]));
    assert_eq!(
        read.symmetry_source,
        SymmetrySource::Table {
            tag: "_space_group_name_h-m_alt".into(),
            value: "P m -3 m".into()
        }
    );
    assert!(read.diagnostics.is_empty(), "{:?}", read.diagnostics);
}

#[test]
fn cif2_files_read_with_their_lists_and_triple_quotes() {
    let text = format!(
        "#\\#CIF_2.0
data_cif2
_audit_conform.dict_name ['cif_core.dic' 'cif_pow.dic']
_publ_section_title '''A title
over two lines'''
_cell.length_a 4.1
_cell.length_b 4.1
_cell.length_c 4.1
_cell.angle_alpha 90
_cell.angle_beta 90
_cell.angle_gamma 90
_space_group.name_H-M_alt 'P m -3 m'
{CSCL_ATOMS}"
    );
    let read = read_text(&text);
    assert_eq!(composition(&read), formula(&[("Cl", 1.0), ("Cs", 1.0)]));
}

// ---------------------------------------------------------------------------
// Rows.

#[test]
fn species_sharing_a_site_keep_their_rows_labels() {
    // A disordered site listed as two rows; a third row of Fe on the same
    // position adds to the first and keeps its label.
    let text = cif(
        "",
        "Fe1 Fe 0.1 0.2 0.3 0.4\nMn1 Mn 0.1 0.2 0.3 0.3\nFe2 Fe 0.1 0.2 0.3 0.2\nO1 O 0.5 0.5 0.5 1\n",
    );
    let read = read_text(&text);
    let labels: Vec<Vec<&str>> = read
        .sites
        .iter()
        .map(|site| site.occupants.iter().map(|o| o.label.as_str()).collect())
        .collect();
    assert_eq!(labels, [vec!["Fe1", "Mn1"], vec!["O1"]]);
}

#[test]
fn pseudo_atoms_and_deuterium() {
    let text = cif(
        "",
        "O1 OH 0.1 0.1 0.1 1\nD1 D 0.2 0.2 0.2 1\nO2 Ow 0.3 0.3 0.3 1\nW1 Wat 0.4 0.4 0.4 1\n",
    );
    // The hydroxyl pseudo-atom is its oxygen; deuterium stays D.
    let read = read_text(&text);
    assert_eq!(composition(&read), formula(&[("D", 1.0), ("O", 3.0)]));
    assert_eq!(
        kinds(&read),
        [DiagnosticKind::PseudoAtom, DiagnosticKind::NoSymmetry]
    );
    assert!(read.diagnostics[0]
        .message
        .contains("O1 (OH as O), W1 (Wat as O)"));
}

#[test]
fn missing_values_take_the_dictionary_defaults() {
    let text = "data_missing
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
loop_
_atom_site_label
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_occupancy
Na1 0 0 0 .
Cl1 0.5 0.5 0.5 1
X1 ? ? ? ?
O1 0.25 0.25 0.25 0
";
    let read = read_text(text);
    assert_eq!(read.cell.gamma, 90.0);
    assert_eq!(composition(&read), formula(&[("Cl", 1.0), ("Na", 1.0)]));
    assert_eq!(
        kinds(&read),
        [
            DiagnosticKind::DefaultValue,
            DiagnosticKind::DefaultValue,
            DiagnosticKind::DefaultValue,
            DiagnosticKind::PlaceholderRow,
            DiagnosticKind::ZeroOccupancy,
            DiagnosticKind::NoSymmetry,
        ]
    );
}

#[test]
fn negative_displacements_are_read_as_none() {
    let text = "data_b
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_atom_site_label
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_U_iso_or_equiv
Na1 0 0 0 -0.01
Cl1 0.5 0.5 0.5 0.01
";
    let read = read_text(text);
    assert_eq!(read.sites[0].occupants[0].u_iso, 0.0);
    assert_eq!(read.sites[1].occupants[0].u_iso, 0.01);
    assert!(kinds(&read).contains(&DiagnosticKind::IgnoredDisplacement));
}

#[test]
fn b_is_read_as_u_and_a_listed_u_comes_first() {
    let text = "data_b
_cell_length_a 5
_cell_length_b 5
_cell_length_c 5
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_atom_site_label
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_B_iso_or_equiv
_atom_site_U_iso_or_equiv
Na1 0 0 0 1.5 ?
Cl1 0.5 0.5 0.5 1.5 0.02
";
    let read = read_text(text);
    let u: Vec<f64> = read
        .sites
        .iter()
        .map(|site| site.occupants[0].u_iso)
        .collect();
    assert_eq!(u, [1.5 / (8.0 * PI * PI), 0.02]);
}

#[test]
fn an_unreadable_oxidation_number_leaves_the_others_theirs() {
    // Fe's number cannot be read and O's is a placeholder: Mn keeps its own
    // and Fe's is reported.
    let text = cif(
        "loop_\n_atom_type_symbol\n_atom_type_oxidation_number\nFe3+ x\nMn2+ 2\nO2- ?\n",
        "Fe1 Fe3+ 0 0 0 1\nMn1 Mn2+ 0.5 0.5 0.5 1\nO1 O2- 0.5 0 0 1\n",
    );
    let oxidation = |s: &Structure| -> BTreeMap<String, f64> {
        s.sites
            .iter()
            .map(|site| (site.label.clone(), site.occupants[0].oxidation))
            .collect()
    };
    let expected = |fe, mn, o| formula(&[("Fe1", fe), ("Mn1", mn), ("O1", o)]);
    let read = read_text(&text);
    assert_eq!(oxidation(&read), expected(0.0, 2.0, 0.0));
    let note = read
        .diagnostics
        .iter()
        .find(|d| d.kind == DiagnosticKind::IgnoredOxidationState)
        .expect("the unreadable number is reported");
    assert!(note.message.starts_with("Fe3+ (x)"), "{}", note.message);
}

// ---------------------------------------------------------------------------
// Symmetry.

fn operations(list: &str) -> String {
    format!("loop_\n_symmetry_equiv_pos_as_xyz\n{list}")
}

#[test]
fn listed_operations_must_form_a_group() {
    let text = cif(
        &operations("x,y,z\n-x,y+1/2,-z\nx+1/2,y,z\n"),
        "Na1 Na 0.1 0.2 0.3 1\n",
    );
    let err = read_cif_str(&text, &CifOptions::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("do not form a group"), "{err}");
}

#[test]
fn repeated_and_misnamed_operations_are_reported() {
    let text = cif(
        &format!(
            "_symmetry_space_group_name_H-M 'P 21/c'\n{}",
            operations("x,y,z\n-x,-y,-z\nx,y,z\n")
        ),
        "Na1 Na 0.1 0.2 0.3 1\n",
    );
    let read = read_text(&text);
    assert_eq!(read.operations.len(), 2);
    assert_eq!(read.atoms.len(), 2);
    assert_eq!(
        kinds(&read),
        [
            DiagnosticKind::DuplicateOperation,
            DiagnosticKind::SpaceGroupMismatch
        ]
    );
}

#[test]
fn listed_multiplicities_and_colliding_atoms_are_checked() {
    let text = "data_checks
_cell_length_a 5
_cell_length_b 6
_cell_length_c 7
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
loop_
_symmetry_equiv_pos_as_xyz
x,y,z
-x,-y,-z
loop_
_atom_site_label
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_symmetry_multiplicity
Na1 0.1 0.2 0.3 1
Cl1 0.14 0.2 0.3 2
";
    let read = read_text(text);
    assert_eq!(
        kinds(&read),
        [
            DiagnosticKind::MultiplicityMismatch,
            DiagnosticKind::CollidingAtoms
        ]
    );
    assert!(read.diagnostics[0]
        .message
        .contains("Na1 (listed 1, orbit 2)"));
    assert!(read.diagnostics[1]
        .message
        .contains("Na1 and Cl1, 0.200 Å apart"));
}

#[test]
fn a_column_of_site_symmetry_orders_is_read_as_orders() {
    // SHELXL-97 writes the site-symmetry order under the multiplicity tag:
    // 1 for a general position, 2 for a site on the mirror of P 2/m.
    let text = |column: &str| {
        format!(
            "data_shelxl97\n_cell_length_a 5\n_cell_length_b 6\n_cell_length_c 7\n_cell_angle_alpha 90\n_cell_angle_beta 90\n_cell_angle_gamma 90\nloop_\n_symmetry_equiv_pos_as_xyz\nx,y,z\n-x,y,-z\n-x,-y,-z\nx,-y,z\nloop_\n_atom_site_label\n_atom_site_fract_x\n_atom_site_fract_y\n_atom_site_fract_z\n_atom_site_symmetry_multiplicity\n{column}"
        )
    };
    let orders = read_text(&text("Na1 0.1 0.2 0.3 1\nCl1 0.3 0 0.3 2\n"));
    assert!(orders.diagnostics.is_empty(), "{:?}", orders.diagnostics);
    assert_eq!(orders.atoms.len(), 6);
    // The same listing with multiplicities reads the same, and a column
    // that is neither is reported.
    let multiplicities = read_text(&text("Na1 0.1 0.2 0.3 4\nCl1 0.3 0 0.3 2\n"));
    assert!(multiplicities.diagnostics.is_empty());
    assert_eq!(multiplicities.atoms.len(), 6);
    let wrong = read_text(&text("Na1 0.1 0.2 0.3 8\nCl1 0.3 0 0.3 2\n"));
    assert_eq!(kinds(&wrong), [DiagnosticKind::MultiplicityMismatch]);
    assert!(wrong.diagnostics[0]
        .message
        .contains("Na1 (listed 8, orbit 4)"));
}

#[test]
fn names_that_disagree_are_settled_by_the_declared_contents() {
    // The Hall symbol names P 1 2 1, the Hermann-Mauguin symbol C 2/m; the
    // formula puts eight atoms in the cell, which only C 2/m's orbit holds.
    let text = cif(
        "_symmetry_space_group_name_Hall 'P 2y'\n_symmetry_space_group_name_H-M 'C 2/m'\n_chemical_formula_sum 'Na8'\n_cell_formula_units_Z 1\n",
        "Na1 Na 0.1 0.2 0.3 1\n",
    );
    let read = read_text(&text);
    assert_eq!(
        kinds(&read),
        [
            DiagnosticKind::SpaceGroupMismatch,
            DiagnosticKind::SettingChosen
        ]
    );
    assert_eq!((read.operations.len(), read.atoms.len()), (8, 8));
    let chosen = &read.diagnostics[1].message;
    assert!(
        chosen.contains("C 1 2/m 1 is read: it holds the cell's declared contents"),
        "{chosen}"
    );
    // Without a formula, nothing tells them apart and the Hall symbol wins.
    let bare = read_text(&cif(
        "_symmetry_space_group_name_Hall 'P 2y'\n_symmetry_space_group_name_H-M 'C 2/m'\n",
        "Na1 Na 0.1 0.2 0.3 1\n",
    ));
    assert_eq!((bare.operations.len(), bare.atoms.len()), (2, 2));
    assert!(bare.diagnostics[1]
        .message
        .contains("P 1 2 1 is read: the file names it"));
    // A number that disagrees with the symbol is settled the same way.
    let numbered = read_text(&cif(
        "_symmetry_space_group_name_H-M 'P 2'\n_space_group_IT_number 10\n_chemical_formula_sum 'Na4'\n_cell_formula_units_Z 1\n",
        "Na1 Na 0.1 0.2 0.3 1\n",
    ));
    assert_eq!(
        kinds(&numbered),
        [
            DiagnosticKind::SpaceGroupMismatch,
            DiagnosticKind::SettingChosen
        ]
    );
    assert_eq!((numbered.operations.len(), numbered.atoms.len()), (4, 4));
}

#[test]
fn the_declared_contents_settle_a_site_near_a_special_position() {
    // Na off the mirror y = 0 by 0.024 Å at occupancy 0.5, which the file
    // places neither on it nor off it: one atom on the mirror holds half a
    // sodium per cell, an atom split around it one. The declared contents
    // settle it either way, before the distance; a formula fitting neither
    // reading leaves the distance to decide, as does none.
    let mirror = operations("x,y,z\nx,-y,z\n");
    let read = |formula: &str, row: &str| {
        read_text(&cif(
            &format!("{mirror}_chemical_formula_sum '{formula}'\n_cell_formula_units_Z 1\n"),
            &format!("{row}O1 O 0.5 0.5 0.5 1\n"),
        ))
    };
    let on = read("Na0.5 O", "Na1 Na 0.1 0.004 0.1 0.5\n");
    assert_eq!((on.atoms.len(), on.sites[0].position[1]), (2, 0.0));
    assert_eq!(kinds(&on), [DiagnosticKind::AmbiguousSpecialPosition]);
    assert!(
        on.diagnostics[0].message.contains(
            "settled by the cell's declared contents: Na1 (occupancy 0.5, 0.024 Å from it) on it"
        ),
        "{}",
        on.diagnostics[0].message
    );
    let split = read("Na O", "Na1 Na 0.1 0.004 0.1 0.5\n");
    assert_eq!(split.atoms.len(), 3);
    assert!(split.diagnostics[0]
        .message
        .contains("settled by the cell's declared contents: Na1 (occupancy 0.5, 0.024 Å from it) split around it"));
    // Closer than the rounding distance, the distance alone would put it on
    // the mirror; the contents say split.
    let apart = read("Na O", "Na1 Na 0.1 0.002 0.1 0.5\n");
    assert_eq!(apart.atoms.len(), 3);
    // Three sodiums fit neither reading: the distance decides, and says so.
    let neither = read("Na3 O", "Na1 Na 0.1 0.004 0.1 0.5\n");
    assert_eq!(neither.atoms.len(), 3);
    assert!(
        neither.diagnostics[0].message.contains(
            "and the declared contents do not settle, read by their distance from it (0.015 Å): Na1 (occupancy 0.5, 0.024 Å from it) split around it"
        ),
        "{}",
        neither.diagnostics[0].message
    );
    // Two open sites of one element are settled together: K at 0.25 near
    // the mirror and the Na above, with the formula holding both split.
    let both = read(
        "Na K0.5 O",
        "Na1 Na 0.1 0.004 0.1 0.5\nK1 K 0.3 0.003 0.3 0.25\n",
    );
    assert_eq!(both.atoms.len(), 5);
    assert!(both.diagnostics[0]
        .message
        .contains("Na1 (occupancy 0.5, 0.024 Å from it) split around it, K1 (occupancy 0.25, 0.018 Å from it) split around it"));
}

#[test]
fn sites_near_special_positions_snap_where_the_file_says_so() {
    // Off the mirror y = 0 by 0.024 Å (y = 0.004 in a 6 Å cell).
    let mirror = operations("x,y,z\nx,-y,z\n");
    let read = |row: &str| read_text(&cif(&mirror, row));
    // At occupancy 1 the two images cannot both be atoms: snapped.
    let full = read("Na1 Na 0.1 0.004 0.1 1\n");
    assert_eq!((full.atoms.len(), full.sites[0].position[1]), (1, 0.0));
    assert!(full.diagnostics.is_empty(), "{:?}", full.diagnostics);
    // At 0.5 they can be an atom split around the mirror: farther than the
    // rounding distance, kept apart and reported.
    let half = read("Na1 Na 0.1 0.004 0.1 0.5\n");
    assert_eq!(half.atoms.len(), 2);
    assert_eq!(kinds(&half), [DiagnosticKind::AmbiguousSpecialPosition]);
    assert!(half.diagnostics[0].message.contains("split around it"));
    // Closer than the rounding distance: on the mirror.
    let near = read("Na1 Na 0.1 0.002 0.1 0.5\n");
    assert_eq!(near.atoms.len(), 1);
    assert!(near.diagnostics[0].message.contains("on it"));
    // Within the site tolerance, images are one atom whatever the occupancy.
    let rounded = read("Na1 Na 0.1 0.00004 0.1 0.5\n");
    assert_eq!(rounded.atoms.len(), 1);
    assert!(rounded.diagnostics.is_empty(), "{:?}", rounded.diagnostics);
    // A listed site symmetry order decides without the distance.
    let listed = |order: u32| {
        let text = cif(&mirror, "Na1 Na 0.1 0.004 0.1 0.5\n").replace(
            "_atom_site_occupancy\n",
            "_atom_site_occupancy\n_atom_site_site_symmetry_order\n",
        );
        read_text(&text.replace("0.5\n", &format!("0.5 {order}\n")))
    };
    assert_eq!(listed(2).atoms.len(), 1);
    assert_eq!(listed(1).atoms.len(), 2);
    assert!(listed(1).diagnostics.is_empty());
}
