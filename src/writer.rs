//! cifmi's writing of a structure: one CIF 1.1 data block in the core
//! dictionary's current names, in the structure's own setting. The design
//! document (§7) states every rule.

use crate::crystallographic::{cell_lattice, cell_symmetry, OCCUPANCY_ROUNDING};
use crate::lattice::Lattice;
use crate::options::CifOptions;
use crate::sites::{moving, orbit, snap_thirds, Grid};
use crate::spacegroups::Setting;
use crate::structure::{CellParameters, Occupant, Site, Structure};
use crate::symmetry::{check_group, gcd, setting_of, xyz};
use crate::symop::SymOp;
use crate::Error;
use rustc_hash::FxHashSet;
use std::borrow::Cow;
use std::fmt::Write;

/// Decimals of the cell, the coordinates, the occupancies and U: enough for
/// a site on a special position to read back onto it.
const DECIMALS: usize = 8;

/// A written occupancy is within this of the structure's.
const OCCUPANCY_PRECISION: f64 = 0.5e-8;

/// The longest line CIF 1.1 allows (paragraph 28 of its syntax), and its
/// longest data block code (paragraph 30).
const MAX_LINE: usize = 2048;
const MAX_BLOCK_CODE: usize = 75;

/// What writing takes from a structure's parts once they are checked: the
/// cell's lattice, the setting the operations are in, and each site's orbit
/// and coordinates as written.
pub(crate) struct Parts {
    pub(crate) lattice: Lattice,
    pub(crate) setting: Option<&'static Setting>,
    pub(crate) orbits: Vec<Vec<[f64; 3]>>,
    pub(crate) coordinates: Vec<[String; 3]>,
}

/// Check that a structure's parts make a CIF that cifmi reads back as them,
/// the way the reader checks a file: `name` a data block code; a cell the
/// reader takes; operations that form a group, each once, of which the cell
/// has the symmetry; and sites each labelled as its first species, on a
/// position of its own up to symmetry, at finite coordinates, holding each
/// species once, with occupancies above 0 adding up to at most 1, U at least
/// 0, finite oxidation states and labels CIF 1.1 can write.
pub(crate) fn check(
    name: &str,
    cell: &CellParameters,
    operations: &[SymOp],
    sites: &[Site],
) -> Result<Parts, Error> {
    block_code(name)?;
    let lattice = cell_lattice(cell)?;
    if operations.is_empty() {
        return Err(Error::cif(
            "no symmetry operations; the identity alone is P1",
        ));
    }
    if let Some(op) = operations
        .iter()
        .find(|op| !op.translation.iter().all(|t| t.is_finite()))
    {
        return Err(Error::cif(format!(
            "operation {}: a translation is not a finite number",
            xyz(op, ",")
        )));
    }
    let setting = setting_of(operations);
    if setting.is_none() {
        check_group(operations).map_err(Error::cif)?;
    }
    cell_symmetry(operations, &lattice)?;
    if sites.is_empty() {
        return Err(Error::cif("no atom sites"));
    }
    for site in sites {
        check_site(site)?;
    }
    // Rows on one position, up to symmetry, are read as one site: rows at the
    // coordinates written, those near a third snapped to it, as the reader
    // takes them.
    let reading = CifOptions::default();
    let images = moving(operations);
    let mut grid = Grid::new(reading.site_tolerance);
    let mut coordinates = Vec::with_capacity(sites.len());
    for site in sites {
        let written = site.position.map(coordinate);
        let position = written.each_ref().map(|text| {
            let read = text.parse().expect("a written coordinate");
            snap_thirds(read, reading.frac_tolerance)
        });
        if let Some((j, _)) = grid.site_of(&images, position) {
            return Err(Error::cif(format!(
                "sites {} and {} are on one position, up to symmetry, which reading makes one site",
                sites[j].label, site.label
            )));
        }
        grid.insert(position);
        coordinates.push(written);
    }
    let orbits = sites
        .iter()
        .map(|site| orbit(operations, site.position))
        .collect();
    Ok(Parts {
        lattice,
        setting,
        orbits,
        coordinates,
    })
}

fn block_code(name: &str) -> Result<(), Error> {
    let why = if name.is_empty() {
        "it is empty".to_string()
    } else if let Some(c) = name.chars().find(|c| !c.is_ascii_graphic()) {
        format!("{c:?} is not printable ASCII other than a space")
    } else if name.len() > MAX_BLOCK_CODE {
        format!("it is longer than {MAX_BLOCK_CODE} characters")
    } else {
        return Ok(());
    };
    Err(Error::cif(format!(
        "the name {name:?} cannot be a data block code: {why}"
    )))
}

fn check_site(site: &Site) -> Result<(), Error> {
    let label = &site.label;
    let Some(first) = site.occupants.first() else {
        return Err(Error::cif(format!("site {label}: no species")));
    };
    if first.label != *label {
        return Err(Error::cif(format!(
            "site {label}: its first species is labelled {}; a site is labelled as its first species",
            first.label
        )));
    }
    if !site.position.iter().all(|v| v.is_finite()) {
        return Err(Error::cif(format!(
            "site {label}: coordinates {:?} are not finite numbers",
            site.position
        )));
    }
    let mut total = 0.0;
    for (i, o) in site.occupants.iter().enumerate() {
        char_value(&o.label)?;
        if let Some(same) = site.occupants[..i]
            .iter()
            .find(|p| p.element == o.element && p.oxidation == o.oxidation)
        {
            return Err(Error::cif(format!(
                "site {label}: {} and {} are one species, which reading adds into one",
                same.label, o.label
            )));
        }
        let why = if !(o.occupancy > 0.0 && o.occupancy <= 1.0) {
            format!("occupancy {} is not above 0 and at most 1", o.occupancy)
        } else if is_zero(&fixed(o.occupancy)) {
            format!("occupancy {} is 0 to {DECIMALS} decimals", o.occupancy)
        } else if !(o.u_iso >= 0.0 && o.u_iso.is_finite()) {
            format!("U {} is not a finite number of at least 0", o.u_iso)
        } else if !o.oxidation.is_finite() {
            format!("oxidation state {} is not a finite number", o.oxidation)
        } else {
            total += o.occupancy;
            continue;
        };
        return Err(Error::cif(format!("{}: {why}", o.label)));
    }
    if total > 1.0 + OCCUPANCY_ROUNDING {
        return Err(Error::cif(format!(
            "site {label}: occupancies add up to {total}, more than 1"
        )));
    }
    Ok(())
}

/// A row of the atom-site loop: one species of a site.
struct Row<'s> {
    /// The species' label, made unique.
    label: Cow<'s, str>,
    /// `_atom_site_type_symbol`.
    symbol: String,
    occupant: &'s Occupant,
    /// The site's coordinates as written.
    coordinates: &'s [String; 3],
    multiplicity: usize,
    /// The occupancy as written.
    occupancy: String,
    /// U as written; `None` where the species has none to the decimals
    /// written.
    u_iso: Option<String>,
}

/// The structure as one CIF 1.1 data block (§7 of the design document).
pub(crate) fn write(structure: &Structure) -> Result<String, Error> {
    let Parts {
        setting,
        orbits,
        coordinates,
        ..
    } = check(
        &structure.name,
        &structure.cell,
        &structure.operations,
        &structure.sites,
    )?;
    let species: Vec<(&[String; 3], usize, &Occupant)> = structure
        .sites
        .iter()
        .zip(&coordinates)
        .zip(&orbits)
        .flat_map(|((site, written), orbit)| {
            site.occupants
                .iter()
                .map(move |o| (written, orbit.len(), o))
        })
        .collect();
    let labels: Vec<&str> = species.iter().map(|(_, _, o)| o.label.as_str()).collect();
    let rows: Vec<Row> = species
        .into_iter()
        .zip(unique_labels(&labels))
        .map(|((coordinates, multiplicity, occupant), label)| Row {
            label,
            symbol: type_symbol(occupant),
            occupant,
            coordinates,
            multiplicity,
            occupancy: fixed(occupant.occupancy),
            u_iso: Some(fixed(occupant.u_iso)).filter(|u| !is_zero(u)),
        })
        .collect();

    let mut out = String::from("#\\#CIF_1.1\n");
    let _ = writeln!(out, "data_{}", structure.name);
    if let Some(setting) = setting {
        let symbol = match setting.symbol.split_once(':') {
            Some((base, choice)) => Cow::Owned(format!("{base} :{choice}")),
            None => Cow::Borrowed(setting.symbol),
        };
        item(
            &mut out,
            "_space_group_IT_number",
            &setting.number.to_string(),
        );
        item(&mut out, "_space_group_name_H-M_alt", &char_value(&symbol)?);
        item(
            &mut out,
            "_space_group_name_Hall",
            &char_value(setting.hall)?,
        );
    }
    let cell = &structure.cell;
    for (tag, value) in [
        ("_cell_length_a", cell.a),
        ("_cell_length_b", cell.b),
        ("_cell_length_c", cell.c),
        ("_cell_angle_alpha", cell.alpha),
        ("_cell_angle_beta", cell.beta),
        ("_cell_angle_gamma", cell.gamma),
    ] {
        item(&mut out, tag, &fixed(value));
    }
    let (formula, z) = formula(&rows);
    item(&mut out, "_chemical_formula_sum", &char_value(&formula)?);
    item(&mut out, "_cell_formula_units_Z", &z.to_string());

    loop_header(
        &mut out,
        &["_space_group_symop_id", "_space_group_symop_operation_xyz"],
    );
    for (i, op) in structure.operations.iter().enumerate() {
        let _ = writeln!(out, "  {}  {}", i + 1, char_value(&xyz(op, ", "))?);
    }

    if rows.iter().any(|row| row.occupant.oxidation != 0.0) {
        loop_header(
            &mut out,
            &["_atom_type_symbol", "_atom_type_oxidation_number"],
        );
        let mut listed = FxHashSet::default();
        for row in rows.iter().filter(|row| listed.insert(row.symbol.as_str())) {
            let _ = writeln!(out, "  {}  {}", row.symbol, row.occupant.oxidation);
        }
    }

    let with_u = rows.iter().any(|row| row.u_iso.is_some());
    loop_header(
        &mut out,
        &[
            "_atom_site_label",
            "_atom_site_type_symbol",
            "_atom_site_symmetry_multiplicity",
            "_atom_site_fract_x",
            "_atom_site_fract_y",
            "_atom_site_fract_z",
            "_atom_site_occupancy",
        ],
    );
    if with_u {
        out.push_str(" _atom_site_U_iso_or_equiv\n");
    }
    for row in &rows {
        let _ = write!(
            out,
            "  {}  {}  {}",
            char_value(&row.label)?,
            row.symbol,
            row.multiplicity
        );
        for value in row.coordinates {
            let _ = write!(out, "  {value}");
        }
        let _ = write!(out, "  {}", row.occupancy);
        if with_u {
            let _ = write!(out, "  {}", row.u_iso.as_deref().unwrap_or("?"));
        }
        out.push('\n');
    }

    if let Some((n, line)) = out
        .lines()
        .enumerate()
        .find(|(_, line)| line.len() > MAX_LINE)
    {
        return Err(Error::cif(format!(
            "line {} would hold {} characters, more than CIF 1.1 allows ({MAX_LINE}): {}...",
            n + 1,
            line.len(),
            &line[..40]
        )));
    }
    Ok(out)
}

/// `tag value` on a line of its own.
fn item(out: &mut String, tag: &str, value: &str) {
    let _ = writeln!(out, "{tag}   {value}");
}

/// The head of a loop: `loop_` and its tags.
fn loop_header(out: &mut String, tags: &[&str]) {
    out.push_str("loop_\n");
    for tag in tags {
        let _ = writeln!(out, " {tag}");
    }
}

/// A number to [`DECIMALS`] decimals.
fn fixed(value: f64) -> String {
    format!("{value:.DECIMALS$}")
}

/// Whether a number as written is 0.
fn is_zero(written: &str) -> bool {
    written.bytes().all(|b| matches!(b, b'0' | b'.' | b'-'))
}

/// A coordinate to [`DECIMALS`] decimals; one that rounds to 1 or -0 is the
/// same position as 0, and is written so.
fn coordinate(value: f64) -> String {
    let text = fixed(value);
    let rounds_to = |whole: &str| {
        text.strip_prefix(whole)
            .is_some_and(|rest| rest.bytes().all(|b| b == b'0'))
    };
    if rounds_to("1.") || rounds_to("-0.") {
        fixed(0.0)
    } else {
        text
    }
}

/// A character value as CIF 1.1 writes it (paragraphs 9 to 20 of its
/// syntax): bare where the rules allow, else in single or double quotes, else
/// a text field, which starts and ends a line. A character outside CIF 1.1's
/// set, or a line starting with `;` in a text field, is an error.
fn char_value(text: &str) -> Result<Cow<'_, str>, Error> {
    if let Some(c) = text
        .chars()
        .find(|&c| !matches!(c, '\t' | '\n' | ' '..='~'))
    {
        return Err(Error::cif(format!(
            "{text:?}: {c:?} is not in CIF 1.1's character set, printable ASCII"
        )));
    }
    let starts_with = |prefix: &str| {
        text.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    let bare = !text.is_empty()
        && !text.starts_with(['_', '#', '$', '\'', '"', '[', ']', ';'])
        && !text.contains([' ', '\t', '\n'])
        && !["?", ".", "loop_", "stop_", "global_"]
            .iter()
            .any(|word| text.eq_ignore_ascii_case(word))
        && !starts_with("data_")
        && !starts_with("save_");
    if bare {
        return Ok(Cow::Borrowed(text));
    }
    if !text.contains('\n') {
        // A quoted value ends at its quote followed by white space.
        for quote in ['\'', '"'] {
            let mut chars = text.chars().peekable();
            let mut ends = false;
            while let Some(c) = chars.next() {
                ends |= c == quote && matches!(chars.peek(), Some(' ' | '\t'));
            }
            if !ends {
                return Ok(Cow::Owned(format!("{quote}{text}{quote}")));
            }
        }
    }
    if text.split('\n').skip(1).any(|line| line.starts_with(';')) {
        return Err(Error::cif(format!(
            "{text:?}: a text field cannot hold a line starting with ;"
        )));
    }
    Ok(Cow::Owned(format!("\n;{text}\n;")))
}

/// The labels of the rows, unique: a label repeated takes the first of `_2`,
/// `_3`... that no row has.
fn unique_labels<'a>(labels: &[&'a str]) -> Vec<Cow<'a, str>> {
    let mut taken: FxHashSet<Cow<'a, str>> =
        labels.iter().map(|&label| Cow::Borrowed(label)).collect();
    let mut seen = FxHashSet::default();
    labels
        .iter()
        .map(|&label| {
            if seen.insert(label) {
                return Cow::Borrowed(label);
            }
            let free = (2..)
                .map(|n| format!("{label}_{n}"))
                .find(|candidate| !taken.contains(candidate.as_str()))
                .expect("a free label");
            taken.insert(Cow::Owned(free.clone()));
            Cow::Owned(free)
        })
        .collect()
}

/// A species' `_atom_site_type_symbol`: its element, with its oxidation
/// state where it has one (`Fe3+`, `O2-`).
fn type_symbol(o: &Occupant) -> String {
    let symbol = o.element.symbol();
    if o.oxidation == 0.0 {
        return symbol.to_string();
    }
    let sign = if o.oxidation > 0.0 { '+' } else { '-' };
    format!("{symbol}{}{sign}", o.oxidation.abs())
}

/// `_chemical_formula_sum` and `_cell_formula_units_Z` of the cell's contents,
/// each row's multiplicity times its occupancy as written: elements in Hill
/// order (carbon, then hydrogen, then the rest alphabetically; without
/// carbon, all alphabetically), with Z the greatest common divisor of the
/// amounts where all are whole to the written precision, else 1, so that the
/// formula times Z is the cell's contents.
fn formula(rows: &[Row]) -> (String, i64) {
    // Each element's amount, and how far rounding the occupancies can move it.
    let mut amounts: Vec<(&str, f64, f64)> = Vec::new();
    for row in rows {
        let symbol = row.occupant.element.symbol();
        let occupancy: f64 = row.occupancy.parse().expect("a written number");
        let multiplicity = row.multiplicity as f64;
        let (amount, precision) = (multiplicity * occupancy, multiplicity * OCCUPANCY_PRECISION);
        match amounts.iter_mut().find(|(s, ..)| *s == symbol) {
            Some((_, total, bound)) => {
                *total += amount;
                *bound += precision;
            }
            None => amounts.push((symbol, amount, precision)),
        }
    }
    let carbon = amounts.iter().any(|(symbol, ..)| *symbol == "C");
    amounts.sort_by_key(|(symbol, ..)| {
        let rank = match (carbon, *symbol) {
            (true, "C") => 0,
            (true, "H") => 1,
            _ => 2,
        };
        (rank, *symbol)
    });
    let whole = |amount: f64, bound: f64| {
        let n = amount.round();
        (n >= 1.0 && (amount - n).abs() <= bound).then_some(n as i64)
    };
    let z = amounts
        .iter()
        .map(|&(_, amount, bound)| whole(amount, bound))
        .collect::<Option<Vec<i64>>>()
        .map_or(1, |counts| counts.into_iter().fold(0, gcd));
    let mut formula = String::new();
    for &(symbol, amount, bound) in &amounts {
        if !formula.is_empty() {
            formula.push(' ');
        }
        formula.push_str(symbol);
        match whole(amount, bound) {
            Some(n) if n / z == 1 => {}
            Some(n) => {
                let _ = write!(formula, "{}", n / z);
            }
            None => formula.push_str(fixed(amount).trim_end_matches('0').trim_end_matches('.')),
        }
    }
    (formula, z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;

    #[test]
    fn values_are_bare_quoted_or_text_fields_by_cif_rules() {
        let written = |text: &str| char_value(text).unwrap().into_owned();
        assert_eq!(written("Fe1"), "Fe1");
        assert_eq!(written("C1'"), "C1'");
        assert_eq!(written("P 1 21/n 1"), "'P 1 21/n 1'");
        assert_eq!(written("-R 3 2\""), "'-R 3 2\"'");
        for reserved in [
            "?", ".", "_x", "#1", "$1", "'a", "[1]", ";1", "loop_", "DATA_x",
        ] {
            assert_eq!(written(reserved), format!("'{reserved}'"));
        }
        assert_eq!(written(""), "''");
        assert_eq!(written("it's ok"), "'it's ok'");
        assert_eq!(written("O' 1"), "\"O' 1\"");
        assert_eq!(written("O' \"1\" 2"), "\n;O' \"1\" 2\n;");
        assert_eq!(written("one\ntwo"), "\n;one\ntwo\n;");
        assert!(char_value("one\n;two").is_err());
        assert!(char_value("Größe").is_err());
    }

    #[test]
    fn repeated_labels_take_the_first_free_suffix() {
        let labels = unique_labels(&["O1", "O1", "O1_2", "O1", "Fe1"]);
        assert_eq!(labels, ["O1", "O1_3", "O1_2", "O1_4", "Fe1"]);
    }

    #[test]
    fn coordinates_that_round_to_1_or_minus_0_are_0() {
        let written = coordinate;
        assert_eq!(written(0.999999999), "0.00000000");
        assert_eq!(written(-1e-10), "0.00000000");
        assert_eq!(written(1.0 / 3.0), "0.33333333");
        assert_eq!(written(-0.25), "-0.25000000");
        assert_eq!(written(1.25), "1.25000000");
    }

    #[test]
    fn formulas_are_in_hill_order_with_z() {
        let origin = [0.0; 3].map(coordinate);
        let written = |contents: &[(&str, usize, f64)]| {
            let occupants: Vec<Occupant> = contents
                .iter()
                .map(|&(symbol, _, occupancy)| Occupant {
                    label: symbol.into(),
                    element: Element::from_symbol(symbol).unwrap(),
                    oxidation: 0.0,
                    occupancy,
                    u_iso: 0.0,
                })
                .collect();
            let rows: Vec<Row> = occupants
                .iter()
                .zip(contents)
                .map(|(occupant, &(_, multiplicity, _))| Row {
                    label: Cow::Borrowed(&occupant.label),
                    symbol: type_symbol(occupant),
                    occupant,
                    coordinates: &origin,
                    multiplicity,
                    occupancy: fixed(occupant.occupancy),
                    u_iso: None,
                })
                .collect();
            formula(&rows)
        };
        assert_eq!(
            written(&[("O", 8, 1.0), ("C", 4, 1.0), ("H", 8, 1.0), ("Cl", 4, 1.0)]),
            ("C H2 Cl O2".to_string(), 4)
        );
        assert_eq!(
            written(&[("O", 4, 1.0), ("Fe", 2, 0.5), ("Mn", 2, 0.5)]),
            ("Fe Mn O4".to_string(), 1)
        );
        // Whole to the written precision: three thirds are one atom.
        assert_eq!(
            written(&[("Na", 3, 1.0 / 3.0), ("Cl", 1, 1.0)]),
            ("Cl Na".to_string(), 1)
        );
        assert_eq!(
            written(&[("Si", 2, 0.75), ("O", 4, 1.0), ("D", 2, 1.0)]),
            ("D2 O4 Si1.5".to_string(), 1)
        );
    }
}
