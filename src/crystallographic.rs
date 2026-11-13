//! cifmi's reading of a CIF: standard syntax, checked symmetry, sites
//! snapped onto their special positions. The design document (§6) states
//! every rule.

use crate::document::{self, Block, Column};
use crate::element::Element;
use crate::lattice::Lattice;
use crate::lexer::Value;
use crate::options::CifOptions;
use crate::sites::{moving, orbit, snap_thirds, snap_to_special_position, snap_within, Grid};
use crate::structure::{
    Atom, CellParameters, Diagnostic, DiagnosticKind, Occupant, Site, Structure, SymmetrySource,
};
use crate::symmetry::{self, Candidate, Source};
use crate::symop::SymOp;
use crate::writer::{read_back, reads_back, written};
use crate::Error;
use rustc_hash::FxHashMap;
use std::f64::consts::PI;

const FRACT: [&str; 3] = [
    "_atom_site_fract_x",
    "_atom_site_fract_y",
    "_atom_site_fract_z",
];

/// Atoms closer than this, in Å, collide unless their sites' occupancies
/// allow only one of them at a time.
const COLLISION_DISTANCE: f64 = 0.5;

/// A site's occupancies may add up to this much above 1 from rounding; a
/// site fuller than that is overfull.
pub(crate) const OCCUPANCY_ROUNDING: f64 = 1e-6;

/// B = 8π²U: a displacement B per U, both in Å².
const B_PER_U: f64 = 8.0 * PI * PI;

/// A partly occupied site whose file says nothing of its site symmetry, and
/// whose cell's declared contents do not settle it, is on a special position
/// when it lies closer than this, in Å: farther, its images there are read
/// as an atom split around it. The declared formulas of the COD sample's
/// cases put the distance here: offsets up to 0.0105 Å are rounding, one of
/// 0.021 Å a split atom.
const ROUNDING_DISTANCE: f64 = 0.015;

/// Read the first structure in CIF text: the named block, or else the first
/// block listing atom positions that yields a structure.
pub(crate) fn read(text: &str, options: &CifOptions) -> Result<Structure, Error> {
    let blocks = document::parse(text)?;
    let mut first_error = None;
    for block in &blocks {
        let wanted = match &options.block {
            Some(name) => block.name.eq_ignore_ascii_case(name),
            None => block.has(FRACT[0]),
        };
        if !wanted {
            continue;
        }
        match structure(block, options) {
            Ok(structure) => return Ok(structure),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| match &options.block {
        Some(name) => Error::cif(format!("no data block named {name:?}")),
        None => Error::cif("no data block lists atom positions (_atom_site_fract_x)"),
    }))
}

fn structure(block: &Block<'_>, options: &CifOptions) -> Result<Structure, Error> {
    if block.names().any(|k| {
        k.contains("_space_group_magn")
            || k.contains("_atom_site_moment")
            || k.contains("_space_group_symop_magn")
    }) {
        return Err(Error::cif("magnetic CIFs are not supported"));
    }
    if block.names().any(|k| {
        k.starts_with("_space_group_symop_ssg")
            || k.starts_with("_cell_modulation_dimension")
            || k.starts_with("_cell_wave_vector")
    }) {
        return Err(Error::cif(
            "modulated structures (superspace groups) are not supported",
        ));
    }
    let mut diagnostics = block.diagnostics.clone();
    let cell = cell(block, &mut diagnostics)?;
    let lattice = cell_lattice(&cell)?;
    let space_group = symmetry::names(block);
    let rows = rows(block, &lattice, options, &mut diagnostics)?;
    if rows.is_empty() {
        return Err(Error::cif("no atom sites"));
    }
    let declared = declared_formula(block);
    let (operations, symmetry_source, assembly) = match symmetry::resolve(block, &mut diagnostics)?
    {
        Source::Listed { tag, operations } => {
            let assembly = assemble(&rows, &operations, &lattice, options, declared.as_deref())?;
            let source = SymmetrySource::Listed { tag: tag.into() };
            (operations, source, assembly)
        }
        Source::Named {
            tag,
            value,
            candidates,
        } => {
            let (candidate, assembly) = settle(
                &rows,
                &cell,
                &lattice,
                options,
                declared.as_deref(),
                tag,
                &value,
                candidates,
                &mut diagnostics,
            )?;
            let source = SymmetrySource::Table {
                tag: tag.into(),
                value,
            };
            (candidate.operations, source, assembly)
        }
        Source::Unspecified => {
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::NoSymmetry,
                "no symmetry operations, space-group name or number; read as P1",
            ));
            let operations = vec![SymOp::IDENTITY];
            let assembly = assemble(&rows, &operations, &lattice, options, declared.as_deref())?;
            (operations, SymmetrySource::P1, assembly)
        }
    };
    diagnostics.extend(cell_symmetry(&operations, &lattice)?);
    diagnostics.extend(assembly.diagnostics);
    Ok(Structure {
        name: block.name.to_string(),
        cell,
        lattice,
        space_group,
        symmetry_source,
        sites: assembly.sites,
        operations,
        atoms: assembly.atoms,
        diagnostics,
    })
}

/// The lattice of a cell, which must be at least 0.01 Å thick between each
/// pair of faces.
pub(crate) fn cell_lattice(cell: &CellParameters) -> Result<Lattice, Error> {
    let lattice =
        Lattice::from_parameters(cell.a, cell.b, cell.c, cell.alpha, cell.beta, cell.gamma)?;
    for hkl in [[1, 0, 0], [0, 1, 0], [0, 0, 1]] {
        if lattice.d_spacing(hkl) < 0.01 {
            return Err(Error::cif("cell thinner than 0.01 Å"));
        }
    }
    Ok(lattice)
}

/// A departure of the cell from the symmetry of its operations above this,
/// relative to the metric, is an error: about 1 % in a length or 0.6° in an
/// angle.
const CELL_SYMMETRY_ERROR: f64 = 1e-2;
/// Above this the departure is reported; below it is rounding.
const CELL_SYMMETRY_NOTE: f64 = 1e-5;

/// Every operation must be an isometry of the cell: its rotation R must
/// keep the metric G = M Mᵀ (RᵀGR = G). The largest relative change of an
/// entry, |(RᵀGR − G)ᵢⱼ| / √(Gᵢᵢ Gⱼⱼ), is an error above
/// [`CELL_SYMMETRY_ERROR`] (a cubic name on an orthorhombic cell) and a
/// diagnostic above [`CELL_SYMMETRY_NOTE`] (an angle of 90.06° under a
/// two-fold axis), returned to report; the cell is read as given.
pub(crate) fn cell_symmetry(
    operations: &[SymOp],
    lattice: &Lattice,
) -> Result<Option<Diagnostic>, Error> {
    let m = lattice.matrix();
    let dot = |u: &[f64; 3], v: &[f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let mut g = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            g[i][j] = dot(&m[i], &m[j]);
        }
    }
    let mut worst: Option<(f64, &SymOp, usize, usize, f64)> = None;
    for op in operations {
        let r = op.rotation;
        for i in 0..3 {
            for j in i..3 {
                let mut image = 0.0;
                for k in 0..3 {
                    for l in 0..3 {
                        image += f64::from(r[k][i]) * g[k][l] * f64::from(r[l][j]);
                    }
                }
                let departure = (image - g[i][j]).abs() / (g[i][i] * g[j][j]).sqrt();
                if worst.is_none_or(|w| departure > w.0) {
                    worst = Some((departure, op, i, j, image));
                }
            }
        }
    }
    let Some((departure, op, i, j, image)) = worst else {
        return Ok(None);
    };
    if departure <= CELL_SYMMETRY_NOTE {
        return Ok(None);
    }
    let axes = ["a", "b", "c"];
    let what = if i == j {
        format!(
            "maps {} ({:.4} Å) onto a vector of {:.4} Å",
            axes[i],
            g[i][i].sqrt(),
            image.max(0.0).sqrt()
        )
    } else {
        let angle = |v: f64| {
            (v / (g[i][i] * g[j][j]).sqrt())
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees()
        };
        format!(
            "changes the angle between {} and {} from {:.3}° to {:.3}°",
            axes[i],
            axes[j],
            angle(g[i][j]),
            angle(image)
        )
    };
    if departure > CELL_SYMMETRY_ERROR {
        return Err(Error::cif(format!(
            "the cell does not have the symmetry of its operations: {} {what}",
            symmetry::xyz(op, ",")
        )));
    }
    Ok(Some(Diagnostic::new(
        DiagnosticKind::CellSymmetryMismatch,
        format!(
            "the cell departs from the symmetry of its operations: {} {what}",
            symmetry::xyz(op, ",")
        ),
    )))
}

// ---------------------------------------------------------------------------
// Values.

/// A number as CIF writes it, without its standard uncertainty (`0.25(3)`
/// is 0.25); `None` for a placeholder, `Some(Err)` for anything else that is
/// not a finite number.
fn number(value: &Value<'_>) -> Option<Result<f64, ()>> {
    if value.is_placeholder() {
        return None;
    }
    let text = value.text.trim();
    let digits = text.split_once('(').map_or(text, |(v, _)| v).trim();
    Some(
        digits
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or(()),
    )
}

/// Up to five of `items`, and how many more.
fn some_of(items: &[String]) -> String {
    let shown = items.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    match items.len() {
        n if n > 5 => format!("{shown} and {} more", n - 5),
        _ => shown,
    }
}

fn cell(block: &Block<'_>, diagnostics: &mut Vec<Diagnostic>) -> Result<CellParameters, Error> {
    let read = |tag: &str| block.value(tag).map(|v| (v, number(v)));
    let length = |tag: &str| match read(tag) {
        Some((_, Some(Ok(v)))) => Ok(v),
        Some((value, Some(Err(())))) => Err(Error::cif(format!(
            "{tag} {:?} is not a number",
            value.text
        ))),
        _ => Err(Error::cif(format!("missing {tag}"))),
    };
    let mut angle = |tag: &str| match read(tag) {
        Some((_, Some(Ok(v)))) => Ok(v),
        Some((value, Some(Err(())))) => Err(Error::cif(format!(
            "{tag} {:?} is not a number",
            value.text
        ))),
        _ => {
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::DefaultValue,
                format!("{tag} is not given; read as the CIF dictionary's default of 90°"),
            ));
            Ok(90.0)
        }
    };
    Ok(CellParameters {
        a: length("_cell_length_a")?,
        b: length("_cell_length_b")?,
        c: length("_cell_length_c")?,
        alpha: angle("_cell_angle_alpha")?,
        beta: angle("_cell_angle_beta")?,
        gamma: angle("_cell_angle_gamma")?,
    })
}

/// The cell's contents the file declares, `_chemical_formula_sum` times
/// `_cell_formula_units_Z`, by element; `None` without both, or when the
/// formula is not a plain run of symbols and counts (`C12 H20 O6 Zn`).
fn declared_formula(block: &Block<'_>) -> Option<Vec<(Element, f64)>> {
    let formula = block.value("_chemical_formula_sum")?;
    let z = block.value("_cell_formula_units_z")?;
    if formula.is_placeholder() || z.is_placeholder() {
        return None;
    }
    let z: f64 = z.text.split('(').next()?.trim().parse().ok()?;
    if !(z.is_finite() && z > 0.0) {
        return None;
    }
    let text = formula.text.trim();
    let bytes = text.as_bytes();
    let mut amounts = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if !bytes[i].is_ascii_uppercase() {
            return None;
        }
        let start = i;
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_lowercase() {
            i += 1;
        }
        let element = Element::from_symbol(&text[start..i]).ok()?;
        let digits = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        let count: f64 = if digits == i {
            1.0
        } else {
            text[digits..i].parse().ok()?
        };
        amounts.push((element, count * z));
    }
    (!amounts.is_empty()).then_some(amounts)
}

/// Whether an assembly holds the declared contents, element by element
/// (deuterium counted as hydrogen), within 1.5 % or 0.05 atoms.
fn matches_declared(assembly: &Assembly, declared: &[(Element, f64)]) -> bool {
    let mut cell: FxHashMap<u8, f64> = FxHashMap::default();
    for atom in &assembly.atoms {
        for o in &assembly.sites[atom.site].occupants {
            *cell.entry(o.element.z()).or_insert(0.0) += o.occupancy;
        }
    }
    let mut wanted: FxHashMap<u8, f64> = FxHashMap::default();
    for (element, amount) in declared {
        *wanted.entry(element.z()).or_insert(0.0) += amount;
    }
    cell.keys().chain(wanted.keys()).all(|z| {
        let have = cell.get(z).copied().unwrap_or(0.0);
        let want = wanted.get(z).copied().unwrap_or(0.0);
        (have - want).abs() <= (0.015 * want).max(0.05)
    })
}

// ---------------------------------------------------------------------------
// Atom rows.

/// One row of the atom-site loop, as read.
struct Row {
    label: String,
    element: Element,
    oxidation: f64,
    occupancy: f64,
    u_iso: f64,
    position: [f64; 3],
    /// `_atom_site_symmetry_multiplicity`.
    multiplicity: Option<usize>,
    /// `_atom_site_site_symmetry_order`.
    site_symmetry_order: Option<usize>,
}

/// Pseudo-atoms for groups, read as their heaviest atom.
const PSEUDO_ATOMS: [(&str, &str); 5] = [
    ("Wat", "O"),
    ("wat", "O"),
    ("OH2", "O"),
    ("OH", "O"),
    ("NO3", "N"),
];

/// Water's own oxygen and hydrogen, which are atoms.
const WATER_ATOMS: [(&str, &str); 2] = [("Ow", "O"), ("Hw", "H")];

/// The element symbol a type symbol or label names, and whether it stands
/// for a group: a pseudo-atom or water prefix; else the first two
/// characters in title case when they name an element (`FE1` is Fe), else
/// the first alone (`O2-` is O); else a capital with its lowercase letters,
/// after a leading `w` for water, to be checked against the elements.
fn element_symbol(raw: &str) -> Option<(String, bool)> {
    for (prefix, element) in PSEUDO_ATOMS {
        if raw.starts_with(prefix) {
            return Some((element.to_string(), true));
        }
    }
    for (prefix, element) in WATER_ATOMS {
        if raw.starts_with(prefix) {
            return Some((element.to_string(), false));
        }
    }
    let mut two = String::new();
    for (i, c) in raw.chars().take(2).enumerate() {
        if i == 0 {
            two.extend(c.to_uppercase());
        } else {
            two.extend(c.to_lowercase());
        }
    }
    if Element::from_symbol(&two).is_ok() {
        return Some((two, false));
    }
    let first: String = raw.chars().take(1).flat_map(char::to_uppercase).collect();
    if !first.is_empty() && Element::from_symbol(&first).is_ok() {
        return Some((first, false));
    }
    let rest = raw.strip_prefix('w').unwrap_or(raw);
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {
            let lower: String = chars.take_while(|c| c.is_ascii_lowercase()).collect();
            let prefix = if rest.len() < raw.len() { "w" } else { "" };
            Some((format!("{prefix}{c}{lower}"), false))
        }
        _ => None,
    }
}

/// Columns read together: of one loop, or single items alike.
fn together(a: &Column<'_, '_>, b: &Column<'_, '_>) -> bool {
    if a.is_loop() {
        a.same_loop(b)
    } else {
        !b.is_loop()
    }
}

/// The atom types' oxidation numbers, `_atom_type_oxidation_number` by
/// `_atom_type_symbol`, also under the symbol with its charge removed where
/// no type is listed under that symbol (`Fe` keeps its own number beside
/// `Fe3+`). Each type keeps its own; a placeholder (`?`, `.`) leaves its type
/// without one, and the types whose numbers cannot be read are returned to
/// report.
fn oxidation_states(block: &Block<'_>) -> (FxHashMap<String, f64>, Vec<String>) {
    let mut out = FxHashMap::default();
    let mut unreadable = Vec::new();
    let (Some(symbols), Some(numbers)) = (
        block.column("_atom_type_symbol"),
        block.column("_atom_type_oxidation_number"),
    ) else {
        return (out, unreadable);
    };
    let mut read = Vec::new();
    for (i, symbol) in symbols.iter().enumerate() {
        let Some(value) = numbers.get(i) else {
            continue;
        };
        match number(value) {
            Some(Ok(oxidation)) => read.push((symbol.text.to_string(), oxidation)),
            Some(Err(())) => unreadable.push(format!("{} ({})", symbol.text, value.text)),
            None => {}
        }
    }
    for (symbol, oxidation) in &read {
        let mut stripped = symbol.clone();
        if stripped.ends_with(['+', ',', '-']) {
            stripped.pop();
        }
        if stripped.ends_with(|c: char| c.is_ascii_digit()) {
            stripped.pop();
        }
        out.insert(stripped, *oxidation);
    }
    for (symbol, oxidation) in &read {
        out.insert(symbol.clone(), *oxidation);
    }
    (out, unreadable)
}

/// Anisotropic displacements by label, as U (Å²): U11, U22, U33, U12, U13,
/// U23; B values are converted by U = B / 8π².
fn anisotropic(block: &Block<'_>) -> FxHashMap<String, [f64; 6]> {
    let mut out = FxHashMap::default();
    let Some(labels) = block.column("_atom_site_aniso_label") else {
        return out;
    };
    let names = |kind: &str| {
        ["11", "22", "33", "12", "13", "23"].map(|ij| format!("_atom_site_aniso_{kind}_{ij}"))
    };
    let (columns, scale) = match names("u").map(|n| block.column(&n)) {
        columns if columns.iter().all(Option::is_some) => (columns, 1.0),
        _ => (names("b").map(|n| block.column(&n)), 1.0 / B_PER_U),
    };
    if !columns
        .iter()
        .all(|c| c.is_some_and(|c| labels.same_loop(&c)))
    {
        return out;
    }
    for (row, label) in labels.iter().enumerate() {
        let values = columns.map(|c| {
            c.and_then(|c| c.get(row))
                .and_then(number)
                .and_then(Result::ok)
        });
        if values.iter().all(Option::is_some) {
            out.insert(
                label.text.to_string(),
                values.map(|v| v.unwrap_or(0.0) * scale),
            );
        }
    }
    out
}

/// U_eq, a third of the trace of U in Cartesian axes:
/// (1/3) Σ U^ij a*_i a*_j a_i·a_j.
fn u_equivalent(u: [f64; 6], lattice: &Lattice) -> f64 {
    let [u11, u22, u33, u12, u13, u23] = u;
    let tensor = [[u11, u12, u13], [u12, u22, u23], [u13, u23, u33]];
    let m = lattice.matrix();
    let r = lattice.reciprocal();
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let a_star = r.map(|v| dot(v, v).sqrt());
    let mut sum = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            sum += tensor[i][j] * a_star[i] * a_star[j] * dot(m[i], m[j]);
        }
    }
    sum / 3.0
}

/// Row `i` of `column`, unless missing or a placeholder.
fn at<'b, 'a>(column: &Option<Column<'b, 'a>>, i: usize) -> Option<&'b Value<'a>> {
    column.as_ref()?.get(i).filter(|v| !v.is_placeholder())
}

fn rows(
    block: &Block<'_>,
    lattice: &Lattice,
    options: &CifOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Row>, Error> {
    let fract = FRACT.map(|tag| block.column(tag));
    let [Some(xs), Some(ys), Some(zs)] = fract else {
        return Err(Error::cif(
            "atom sites need _atom_site_fract_x, _atom_site_fract_y and _atom_site_fract_z",
        ));
    };
    if !(together(&xs, &ys) && together(&xs, &zs)) {
        return Err(Error::cif(
            "_atom_site_fract_x, _y and _z are not given together",
        ));
    }
    let column = |tag: &str| block.column(tag).filter(|c| together(&xs, c));
    let labels = column("_atom_site_label");
    let types = column("_atom_site_type_symbol");
    let occupancies = column("_atom_site_occupancy");
    let b_iso = column("_atom_site_b_iso_or_equiv");
    let u_iso = column("_atom_site_u_iso_or_equiv");
    let calc = column("_atom_site_calc_flag");
    let multiplicities = column("_atom_site_symmetry_multiplicity");
    let orders = column("_atom_site_site_symmetry_order");
    if labels.is_none() && types.is_none() {
        return Err(Error::cif(
            "atom sites have neither _atom_site_label nor _atom_site_type_symbol",
        ));
    }
    let (oxidation, unreadable_oxidation) = oxidation_states(block);
    let aniso = anisotropic(block);

    let mut rows = Vec::with_capacity(xs.len());
    let (mut placeholders, mut dummies, mut empty, mut pseudo, mut ignored) =
        (0usize, Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..xs.len() {
        let label_value = at(&labels, i);
        let type_value = at(&types, i);
        let label = label_value
            .or(type_value)
            .map_or_else(|| format!("row {}", i + 1), |v| v.text.to_string());
        if at(&calc, i).is_some_and(|v| v.text.eq_ignore_ascii_case("dum")) {
            dummies.push(label);
            continue;
        }
        let raw = [xs.get(i), ys.get(i), zs.get(i)].map(|v| v.expect("one loop"));
        if raw.iter().all(|v| v.is_placeholder()) {
            placeholders += 1;
            continue;
        }
        let mut position = [0.0; 3];
        for (axis, value) in raw.iter().enumerate() {
            position[axis] = match number(value) {
                Some(Ok(v)) => snap_thirds(v, options.frac_tolerance),
                _ => {
                    return Err(Error::cif(format!(
                        "{label}: coordinate {:?} is not a number",
                        value.text
                    )))
                }
            };
        }
        let occupancy = match at(&occupancies, i).map(number) {
            None | Some(None) => 1.0,
            Some(Some(Ok(v))) => v,
            Some(Some(Err(()))) => {
                return Err(Error::cif(format!(
                    "{label}: occupancy {:?} is not a number",
                    at(&occupancies, i).map_or("", |v| v.text.as_ref())
                )))
            }
        };
        if !(occupancy > 0.0) {
            empty.push(label);
            continue;
        }
        let raw_symbol = type_value.or(label_value).map_or("", |v| v.text.as_ref());
        let unknown = || Error::cif(format!("{label}: {raw_symbol:?} names no element"));
        let (symbol, is_pseudo) = element_symbol(raw_symbol).ok_or_else(unknown)?;
        let element = Element::from_symbol(&symbol).map_err(|_| unknown())?;
        if is_pseudo {
            pseudo.push(format!("{label} ({raw_symbol} as {symbol})"));
        }
        let oxidation = {
            let base = oxidation.get(&symbol).copied().unwrap_or(0.0);
            oxidation.get(raw_symbol).copied().unwrap_or(base)
        };
        // U, else B/8π², else U_eq from the anisotropic displacement.
        let given = [
            at(&u_iso, i).and_then(number),
            at(&b_iso, i)
                .and_then(number)
                .map(|r| r.map(|b| b / B_PER_U)),
            aniso.get(&label).map(|u| Ok(u_equivalent(*u, lattice))),
        ];
        let u = match given.iter().flatten().find(|r| r.is_ok()) {
            Some(Ok(u)) if *u >= 0.0 => *u,
            Some(Ok(u)) => {
                ignored.push(format!("{label} (U = {u:.5} Å²)"));
                0.0
            }
            _ => {
                if given.iter().flatten().any(Result::is_err) {
                    ignored.push(format!("{label} (unreadable)"));
                }
                0.0
            }
        };
        let count = |column: &Option<Column<'_, '_>>| {
            at(column, i)
                .and_then(number)
                .and_then(Result::ok)
                .filter(|v| v.fract() == 0.0 && *v >= 1.0)
                .map(|v| v as usize)
        };
        let multiplicity = count(&multiplicities);
        let site_symmetry_order = count(&orders);
        rows.push(Row {
            label,
            element,
            oxidation,
            occupancy,
            u_iso: u,
            position,
            multiplicity,
            site_symmetry_order,
        });
    }

    if placeholders > 0 {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::PlaceholderRow,
            format!("{placeholders} atom row(s) list only placeholder coordinates and are skipped"),
        ));
    }
    let mut report = |kind, items: &[String], what: &str| {
        if !items.is_empty() {
            diagnostics.push(Diagnostic::new(kind, format!("{}: {what}", some_of(items))));
        }
    };
    report(
        DiagnosticKind::DummyAtom,
        &dummies,
        "dummy atoms (_atom_site_calc_flag dum), skipped",
    );
    report(
        DiagnosticKind::ZeroOccupancy,
        &empty,
        "zero occupancy, skipped",
    );
    report(
        DiagnosticKind::PseudoAtom,
        &pseudo,
        "pseudo-atoms for groups, read as one atom",
    );
    report(
        DiagnosticKind::IgnoredDisplacement,
        &ignored,
        "displacement parameters read as none",
    );
    report(
        DiagnosticKind::IgnoredOxidationState,
        &unreadable_oxidation,
        "oxidation numbers that cannot be read, read as none",
    );
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Sites and atoms.

/// Sites and atoms of rows expanded with a set of operations, and how well
/// they fit.
struct Assembly {
    sites: Vec<Site>,
    atoms: Vec<Atom>,
    diagnostics: Vec<Diagnostic>,
    /// Pairs of atoms on top of each other (`COLLISION_DISTANCE`).
    collisions: usize,
    /// Sites whose orbits differ from their listed multiplicity.
    multiplicity_mismatches: usize,
}

/// Cartesian length of a fractional displacement, across cell boundaries.
fn distance(lattice: &Lattice, a: [f64; 3], b: [f64; 3]) -> f64 {
    let m = lattice.matrix();
    let d = [0, 1, 2].map(|i| {
        let v = b[i] - a[i];
        v - v.round()
    });
    let cart = [0, 1, 2].map(|j| d[0] * m[0][j] + d[1] * m[1][j] + d[2] * m[2][j]);
    (cart[0] * cart[0] + cart[1] * cart[1] + cart[2] * cart[2]).sqrt()
}

/// A site in the making: its first row, and the species the rows on its
/// position add up to.
struct Draft<'r> {
    row: &'r Row,
    occupants: Vec<Occupant>,
}

/// Species joining a site by the rules for rows on one position: a species
/// the site holds already adds its occupancy, except that a row repeats the
/// site's atom when the listing is already expanded and the row lies at a
/// symmetry image, or when adding it would fill the site past the occupancy
/// tolerance with an equal occupancy. The rows read once are kept to report.
struct Joining {
    fully_expanded: bool,
    occupancy_tolerance: f64,
    listed_again: Vec<String>,
    overfilling: Vec<String>,
}

impl Joining {
    /// Add `occupant`, on the site through a symmetry image when `image`.
    fn add(&mut self, occupants: &mut Vec<Occupant>, occupant: Occupant, image: bool) {
        let total: f64 = occupants.iter().map(|o| o.occupancy).sum();
        let Some(same) = occupants
            .iter_mut()
            .find(|o| o.element == occupant.element && o.oxidation == occupant.oxidation)
        else {
            occupants.push(occupant);
            return;
        };
        if image && self.fully_expanded {
            self.listed_again.push(occupant.label);
        } else if total + occupant.occupancy > self.occupancy_tolerance + 1e-9
            && (same.occupancy - occupant.occupancy).abs() <= 1e-3
        {
            self.overfilling.push(occupant.label);
        } else {
            same.occupancy += occupant.occupancy;
        }
    }
}

/// Rows merged into sites and expanded into atoms with `operations`.
fn assemble(
    rows: &[Row],
    operations: &[SymOp],
    lattice: &Lattice,
    options: &CifOptions,
    declared: Option<&[(Element, f64)]>,
) -> Result<Assembly, Error> {
    let mut diagnostics = Vec::new();
    let images = moving(operations);

    // Rows on one position, directly or through symmetry, make one site, and
    // rows of one species add their occupancies: a
    // pair of half atoms at mirror images is one full atom at each. A row
    // instead repeats its site's atom when the listing is already expanded,
    // or when adding it would fill the site past the tolerance with an equal
    // occupancy.
    let (expanded, closed) = listing_fit(rows, operations, options.site_tolerance);
    let mut joining = Joining {
        fully_expanded: expanded && closed,
        occupancy_tolerance: options.occupancy_tolerance,
        listed_again: Vec::new(),
        overfilling: Vec::new(),
    };
    let mut drafts: Vec<Draft> = Vec::new();
    let mut grid = Grid::new(options.site_tolerance);
    for row in rows {
        let occupant = Occupant {
            label: row.label.clone(),
            element: row.element,
            oxidation: row.oxidation,
            occupancy: row.occupancy,
            u_iso: row.u_iso,
        };
        match grid.site_of(&images, row.position) {
            Some((j, image)) => joining.add(&mut drafts[j].occupants, occupant, image),
            None => {
                grid.insert(row.position);
                drafts.push(Draft {
                    row,
                    occupants: vec![occupant],
                });
            }
        }
    }

    // Sites the file lists apart can end on one position, up to symmetry,
    // once on their special positions (N1 and N1' of COD 7234017, both on
    // (x, 2x, ¼)), or once written to the decimals cifmi writes. Those are
    // one site too, by the same rules, placed again as one, as reading what
    // cifmi writes of them would make them. Where placing moved no site and
    // writing moves none, the sites are where their rows were compared
    // above. How a site merged away was placed stays reported.
    let (mut merged, mut placed_before) = (Vec::new(), Vec::new());
    let placement = loop {
        let mut placement = place(&drafts, operations, lattice, options, declared)?;
        let moved = drafts.iter().zip(&placement.sites).any(|(draft, site)| {
            site.position != draft.row.position || !site.position.into_iter().all(reads_back)
        });
        let onto = if moved {
            on_one_position(&placement.sites, &images, options)
        } else {
            Vec::new()
        };
        if onto.is_empty() {
            break placement;
        }
        for (k, j, image) in onto {
            merged.push(format!(
                "{} on {}",
                drafts[k].row.label, drafts[j].row.label
            ));
            placed_before.extend(placement.open[k].take());
            for occupant in std::mem::take(&mut drafts[k].occupants) {
                joining.add(&mut drafts[j].occupants, occupant, image);
            }
        }
        drafts.retain(|draft| !draft.occupants.is_empty());
    };
    if !joining.listed_again.is_empty() {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::RepeatedAtom,
            format!(
                "the listing is already expanded: {} repeat atoms at symmetry images; each atom is read once",
                some_of(&joining.listed_again)
            ),
        ));
    }
    if !joining.overfilling.is_empty() {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::RepeatedAtom,
            format!(
                "{}: rows repeating an atom of their site, whose occupancies would otherwise add up to more than {}; each atom is read once",
                some_of(&joining.overfilling),
                options.occupancy_tolerance
            ),
        ));
    }
    if !merged.is_empty() {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::MergedSites,
            format!(
                "{}: sites listed apart that are on one position, up to symmetry, on their special positions or to the decimals cifmi writes; read as one site",
                some_of(&merged)
            ),
        ));
    }

    let Placement {
        sites,
        listed,
        overfull,
        open,
    } = placement;
    let (mut settled_by_contents, mut read_by_distance) = (Vec::new(), Vec::new());
    for (by_contents, entry) in placed_before.into_iter().chain(open.into_iter().flatten()) {
        if by_contents {
            settled_by_contents.push(entry);
        } else {
            read_by_distance.push(entry);
        }
    }
    let mut atoms = Vec::new();
    let mut multiplicity_mismatches = Vec::new();
    for (k, (site, listed)) in sites.iter().zip(listed).enumerate() {
        let images = orbit(operations, site.position);
        let size = images.len();
        atoms.extend(images.into_iter().map(|image| Atom {
            site: k,
            position: image,
        }));
        if let Some(listed) = listed {
            if listed != size {
                multiplicity_mismatches
                    .push(format!("{} (listed {listed}, orbit {size})", site.label));
            }
        }
    }
    if !overfull.is_empty() {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::OverfullSite,
            format!(
                "{}: occupancies add up to more than 1; scaled to a full site",
                some_of(&overfull)
            ),
        ));
    }
    if !settled_by_contents.is_empty() {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::AmbiguousSpecialPosition,
            format!(
                "partly occupied sites near a special position, which the file does not place on it or off it, settled by the cell's declared contents: {}",
                some_of(&settled_by_contents)
            ),
        ));
    }
    if !read_by_distance.is_empty() {
        let unsettled = if declared.is_some() {
            " and the declared contents do not settle"
        } else {
            ""
        };
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::AmbiguousSpecialPosition,
            format!(
                "partly occupied sites near a special position, which the file does not place on it or off it{unsettled}, read by their distance from it ({ROUNDING_DISTANCE} Å): {}",
                some_of(&read_by_distance)
            ),
        ));
    }
    if !multiplicity_mismatches.is_empty() {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::MultiplicityMismatch,
            format!(
                "{}: listed multiplicities differ from the orbits",
                some_of(&multiplicity_mismatches)
            ),
        ));
    }
    let (collisions, example) = collisions(lattice, &sites, &atoms);
    if let Some((a, b, d)) = example {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::CollidingAtoms,
            format!(
                "{collisions} pair(s) of atoms closer than {COLLISION_DISTANCE} Å, more than one full atom between them, such as {} and {}, {d:.3} Å apart",
                sites[atoms[a].site].label, sites[atoms[b].site].label
            ),
        ));
    }
    Ok(Assembly {
        sites,
        atoms,
        diagnostics,
        collisions,
        multiplicity_mismatches: multiplicity_mismatches.len(),
    })
}

/// Each site on the position of an earlier one, up to symmetry, with that
/// site and whether through an image. The positions are compared as cifmi
/// writes them, where reading the written text compares them, so that
/// rounding in a mean cannot put two sites the tolerance apart (S1 and Se1
/// of COD 1526224, 1e-4 apart on a three-fold axis) on one position in one
/// reading and apart in the other.
fn on_one_position(
    sites: &[Site],
    images: &[&SymOp],
    options: &CifOptions,
) -> Vec<(usize, usize, bool)> {
    let mut grid = Grid::new(options.site_tolerance);
    let (mut owners, mut onto) = (Vec::new(), Vec::new());
    for (k, site) in sites.iter().enumerate() {
        let position = read_back(&written(site.position), options.frac_tolerance);
        match grid.site_of(images, position) {
            Some((i, image)) => onto.push((k, owners[i], image)),
            None => {
                grid.insert(position);
                owners.push(k);
            }
        }
    }
    onto
}

/// The sites as the reading places them, and what it reports.
struct Placement {
    sites: Vec<Site>,
    /// Each site's listed multiplicity.
    listed: Vec<Option<usize>>,
    overfull: Vec<String>,
    /// For each site the file places neither on its special position nor off
    /// it, whether the declared contents settled it (else the distance), and
    /// the report's entry.
    open: Vec<Option<(bool, String)>>,
}

/// Each site placed by the rules of §6: its occupancies, scaled to a full
/// site where they add up to a little more than 1, and its position, on its
/// special position or split around it.
fn place(
    drafts: &[Draft<'_>],
    operations: &[SymOp],
    lattice: &Lattice,
    options: &CifOptions,
    declared: Option<&[(Element, f64)]>,
) -> Result<Placement, Error> {
    // Each site's exact position (images within the site tolerance merged
    // onto their mean) with its stabilizers: the operations fixing it there,
    // and those bringing an image within the special-position distance.
    let order = operations.len();
    let stabilizers: Vec<([f64; 3], usize, usize)> = drafts
        .iter()
        .map(|draft| {
            let exact = snap_within(
                lattice,
                operations,
                draft.row.position,
                options.site_tolerance,
            );
            let same = operations
                .iter()
                .filter(|op| {
                    let image = op.apply(exact);
                    (0..3).all(|a| {
                        let d = image[a] - exact[a];
                        (d - d.round()).abs() < options.site_tolerance
                    })
                })
                .count()
                .max(1);
            let near = operations
                .iter()
                .filter(|op| {
                    distance(lattice, exact, op.apply(exact)) < options.special_position_tolerance
                })
                .count();
            (exact, same, near)
        })
        .collect();
    // SHELXL-97 and -93 write the site-symmetry order under
    // _atom_site_symmetry_multiplicity (1 for every general position). The
    // column is read as orders when that fits the listing better than
    // multiplicities do; a multiplicity is then the group's order over it.
    let (as_multiplicities, as_orders) = drafts
        .iter()
        .zip(&stabilizers)
        .filter_map(|(draft, (_, same, _))| {
            draft.row.multiplicity.map(|listed| (listed, order / same))
        })
        .fold((0, 0), |(m, o), (listed, orbit)| {
            (
                m + usize::from(listed == orbit),
                o + usize::from(listed * orbit == order),
            )
        });
    let column_holds_orders = as_orders > as_multiplicities;
    let listed_multiplicity = |row: &Row| {
        row.multiplicity.and_then(|listed| {
            if column_holds_orders {
                order.is_multiple_of(listed).then(|| order / listed)
            } else {
                Some(listed)
            }
        })
    };

    // Each site's position: settled by the rules below, or left open between
    // the special position and the split around it, for the cell's declared
    // contents and then the distance to settle.
    enum Reading {
        Settled([f64; 3]),
        Open {
            exact: [f64; 3],
            snapped: [f64; 3],
            offset: f64,
        },
    }
    struct Prepared {
        label: String,
        occupants: Vec<Occupant>,
        total: f64,
        listed: Option<usize>,
        reading: Reading,
    }
    let mut prepared = Vec::with_capacity(drafts.len());
    let mut overfull = Vec::new();
    for (k, draft) in drafts.iter().enumerate() {
        let label = draft.row.label.clone();
        let mut occupants = draft.occupants.clone();
        let total: f64 = occupants.iter().map(|o| o.occupancy).sum();
        if total > options.occupancy_tolerance + 1e-9 {
            return Err(Error::cif(format!(
                "site {label}: occupancies add up to {total}, more than {}",
                options.occupancy_tolerance
            )));
        }
        if total > 1.0 + OCCUPANCY_ROUNDING {
            overfull.push(format!("{label} ({total:.4})"));
            for o in &mut occupants {
                o.occupancy /= total;
            }
        }
        let total = total.min(1.0);
        let listed = listed_multiplicity(draft.row);

        // Images within the site tolerance are one atom: the site moves onto
        // their mean, so they coincide exactly.
        // Images farther apart, within the special-position distance, are one
        // atom on the special position too when the site's occupancy rules
        // out separate atoms there. When it allows them, as for an atom
        // refined split around the special position, the file's site
        // symmetry order or multiplicity decides; failing those, the site
        // stays open for the declared contents, then the distance.
        let (exact, same, near) = stabilizers[k];
        let reading = if near <= same {
            Reading::Settled(exact)
        } else {
            let snapped = snap_to_special_position(
                lattice,
                operations,
                exact,
                options.special_position_tolerance,
            );
            let (orbit_apart, orbit_snapped) = (operations.len() / same, operations.len() / near);
            let on_special = if total * (near / same) as f64 > 1.0 + OCCUPANCY_ROUNDING {
                Some(true)
            } else if let Some(order) = draft.row.site_symmetry_order {
                Some(order > 1)
            } else if listed == Some(orbit_snapped) {
                Some(true)
            } else if listed == Some(orbit_apart) {
                Some(false)
            } else {
                None
            };
            match on_special {
                Some(true) => Reading::Settled(snapped),
                Some(false) => Reading::Settled(exact),
                None => Reading::Open {
                    exact,
                    snapped,
                    offset: distance(lattice, exact, snapped),
                },
            }
        };
        prepared.push(Prepared {
            label,
            occupants,
            total,
            listed,
            reading,
        });
    }

    // Open sites: the cell's declared contents settle those on which every
    // reading holding them agrees; the distance settles the rest.
    let open: Vec<usize> = prepared
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p.reading, Reading::Open { .. }))
        .map(|(i, _)| i)
        .collect();
    let by_contents: Vec<Option<bool>> = match declared {
        Some(declared) if !open.is_empty() => {
            let mut settled: FxHashMap<u8, f64> = FxHashMap::default();
            for p in &prepared {
                if let Reading::Settled(position) = p.reading {
                    let size = orbit(operations, position).len() as f64;
                    for o in &p.occupants {
                        *settled.entry(o.element.z()).or_insert(0.0) += size * o.occupancy;
                    }
                }
            }
            let open_sites: Vec<OpenSite> = open
                .iter()
                .map(|&i| {
                    let Reading::Open {
                        exact,
                        snapped,
                        offset,
                    } = prepared[i].reading
                    else {
                        unreachable!("open sites are open")
                    };
                    let contents = |position: [f64; 3]| -> Vec<(u8, f64)> {
                        let size = orbit(operations, position).len() as f64;
                        prepared[i]
                            .occupants
                            .iter()
                            .map(|o| (o.element.z(), size * o.occupancy))
                            .collect()
                    };
                    OpenSite {
                        on_by_distance: offset < ROUNDING_DISTANCE,
                        split: contents(exact),
                        on: contents(snapped),
                    }
                })
                .collect();
            settle_open_sites(&open_sites, &settled, declared)
        }
        _ => vec![None; open.len()],
    };

    let mut sites = Vec::with_capacity(prepared.len());
    let mut listed = Vec::with_capacity(prepared.len());
    let mut open_readings = Vec::with_capacity(prepared.len());
    let mut next_open = 0;
    for p in prepared {
        let (position, open_reading) = match p.reading {
            Reading::Settled(position) => (position, None),
            Reading::Open {
                exact,
                snapped,
                offset,
            } => {
                let settled = by_contents[next_open];
                next_open += 1;
                let on = settled.unwrap_or(offset < ROUNDING_DISTANCE);
                let entry = format!(
                    "{} (occupancy {}, {offset:.3} Å from it) {}",
                    p.label,
                    p.total,
                    if on { "on it" } else { "split around it" }
                );
                let position = if on { snapped } else { exact };
                (position, Some((settled.is_some(), entry)))
            }
        };
        listed.push(p.listed);
        open_readings.push(open_reading);
        sites.push(Site {
            label: p.label,
            position,
            occupants: p.occupants,
        });
    }
    Ok(Placement {
        sites,
        listed,
        overfull,
        open: open_readings,
    })
}

/// A partly occupied site near a special position that the file does not
/// place: what it adds to the cell, by atomic number, read on the position
/// (`on`) or split around it (`split`), and which way its distance reads it.
struct OpenSite {
    on_by_distance: bool,
    split: Vec<(u8, f64)>,
    on: Vec<(u8, f64)>,
}

/// Open sites settled jointly by the declared contents, at most.
const JOINT_LIMIT: usize = 16;

/// Which open sites the cell's declared contents settle: `Some(on)` where
/// every combination of readings that holds the declared amount of each
/// element involved (within `matches_declared`'s tolerance, the settled
/// sites' contents `settled` included) agrees on the site, `None` where no
/// combination holds them or they disagree. Up to `JOINT_LIMIT` sites are
/// settled jointly; beyond that each site is settled alone, the others read
/// by their distance.
fn settle_open_sites(
    sites: &[OpenSite],
    settled: &FxHashMap<u8, f64>,
    declared: &[(Element, f64)],
) -> Vec<Option<bool>> {
    let mut wanted: FxHashMap<u8, f64> = FxHashMap::default();
    for (element, amount) in declared {
        *wanted.entry(element.z()).or_insert(0.0) += amount;
    }
    let mut involved: Vec<u8> = sites
        .iter()
        .flat_map(|s| s.split.iter().chain(&s.on).map(|c| c.0))
        .collect();
    involved.sort_unstable();
    involved.dedup();
    let holds = |on: &dyn Fn(usize) -> bool| {
        involved.iter().all(|z| {
            let mut have = settled.get(z).copied().unwrap_or(0.0);
            for (i, site) in sites.iter().enumerate() {
                let contents = if on(i) { &site.on } else { &site.split };
                have += contents
                    .iter()
                    .filter(|c| c.0 == *z)
                    .map(|c| c.1)
                    .sum::<f64>();
            }
            let want = wanted.get(z).copied().unwrap_or(0.0);
            (have - want).abs() <= (0.015 * want).max(0.05)
        })
    };
    let n = sites.len();
    if n <= JOINT_LIMIT {
        let fitting: Vec<u32> = (0..1u32 << n)
            .filter(|&mask| holds(&|i| mask >> i & 1 == 1))
            .collect();
        (0..n)
            .map(|i| {
                let first = fitting.first()? >> i & 1;
                fitting
                    .iter()
                    .all(|mask| mask >> i & 1 == first)
                    .then_some(first == 1)
            })
            .collect()
    } else {
        (0..n)
            .map(|i| {
                let fits = |on_i: bool| {
                    holds(&|j| {
                        if j == i {
                            on_i
                        } else {
                            sites[j].on_by_distance
                        }
                    })
                };
                match (fits(true), fits(false)) {
                    (true, false) => Some(true),
                    (false, true) => Some(false),
                    _ => None,
                }
            })
            .collect()
    }
}

/// Pairs of atoms closer than `COLLISION_DISTANCE` whose sites hold more than
/// one full atom between them, with an example (indices, distance).
fn collisions(
    lattice: &Lattice,
    sites: &[Site],
    atoms: &[Atom],
) -> (usize, Option<(usize, usize, f64)>) {
    let total: Vec<f64> = sites
        .iter()
        .map(|s| s.occupants.iter().map(|o| o.occupancy).sum())
        .collect();
    // Bins at least as wide as the distance, measured across the cell's
    // planes, so close pairs lie in neighbouring bins.
    let bins = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
        .map(|hkl| ((lattice.d_spacing(hkl) / COLLISION_DISTANCE).floor() as i64).max(1));
    let key = |x: [f64; 3]| {
        [0, 1, 2].map(|a| ((x[a] * bins[a] as f64).floor() as i64).rem_euclid(bins[a]))
    };
    let mut cells: FxHashMap<[i64; 3], Vec<usize>> = FxHashMap::default();
    for (i, atom) in atoms.iter().enumerate() {
        cells.entry(key(atom.position)).or_default().push(i);
    }
    let mut count = 0;
    let mut example = None;
    for (cell, members) in &cells {
        let mut neighbours: Vec<[i64; 3]> = Vec::with_capacity(27);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let n = [
                        (cell[0] + dx).rem_euclid(bins[0]),
                        (cell[1] + dy).rem_euclid(bins[1]),
                        (cell[2] + dz).rem_euclid(bins[2]),
                    ];
                    if !neighbours.contains(&n) {
                        neighbours.push(n);
                    }
                }
            }
        }
        for n in neighbours {
            let Some(others) = cells.get(&n) else {
                continue;
            };
            for &i in members {
                for &j in others {
                    if j <= i {
                        continue;
                    }
                    let (a, b) = (&atoms[i], &atoms[j]);
                    if total[a.site] + total[b.site] <= 1.0 + 1e-3 {
                        continue;
                    }
                    let d = distance(lattice, a.position, b.position);
                    if d < COLLISION_DISTANCE {
                        count += 1;
                        if example.is_none_or(|(_, _, e)| d < e) {
                            example = Some((i, j, d));
                        }
                    }
                }
            }
        }
    }
    (count, example)
}

// ---------------------------------------------------------------------------
// Settings the file leaves open.

/// The axes a cell is set on, when its shape says: `"R"` for rhombohedral,
/// `"H"` for hexagonal.
fn cell_axes(cell: &CellParameters) -> Option<&'static str> {
    let same_length = |x: f64, y: f64| (x - y).abs() <= 1e-3 * x.max(y);
    let same_angle = |x: f64, y: f64| (x - y).abs() <= 0.01;
    if same_length(cell.a, cell.b)
        && same_length(cell.b, cell.c)
        && same_angle(cell.alpha, cell.beta)
        && same_angle(cell.beta, cell.gamma)
        && !same_angle(cell.alpha, 90.0)
    {
        Some("R")
    } else if same_length(cell.a, cell.b)
        && same_angle(cell.alpha, 90.0)
        && same_angle(cell.beta, 90.0)
        && same_angle(cell.gamma, 120.0)
    {
        Some("H")
    } else {
        None
    }
}

/// The unique axis of a monoclinic cell, 0, 1 or 2 for a, b or c: the one
/// axis whose angle is not 90°. `None` when no angle is, or several are.
fn unique_axis(cell: &CellParameters) -> Option<usize> {
    let oblique = |x: f64| (x - 90.0).abs() > 0.01;
    match (oblique(cell.alpha), oblique(cell.beta), oblique(cell.gamma)) {
        (true, false, false) => Some(0),
        (false, true, false) => Some(1),
        (false, false, true) => Some(2),
        _ => None,
    }
}

/// Whether some row lies at a symmetry image of another row's position (the
/// listing is already expanded), and whether every image of every row lies
/// on a row.
fn listing_fit(rows: &[Row], operations: &[SymOp], tolerance: f64) -> (bool, bool) {
    let mut grid = Grid::new(tolerance);
    for row in rows {
        if grid.first_match(row.position).is_none() {
            grid.insert(row.position);
        }
    }
    let (mut expanded, mut closed) = (false, true);
    for (i, &p) in grid.points.iter().enumerate() {
        for op in operations {
            match grid.first_match(op.apply(p)) {
                Some(j) if j != i => expanded = true,
                Some(_) => {}
                None => closed = false,
            }
        }
    }
    (expanded, closed)
}

/// Pick, among the settings the space-group names admit, the one the atoms
/// fit: a listing already expanded fits only its own setting; otherwise the
/// setting with no atoms on top of each other and orbits matching the listed
/// multiplicities; then the one holding the cell's declared contents
/// (`declared`, the formula times Z); then the one the cell's axes or unique
/// axis call for, the one the file names (by Hall symbol or with its origin
/// choice or axes), and the reference setting. The choice is
/// reported.
#[allow(clippy::too_many_arguments)]
fn settle(
    rows: &[Row],
    cell: &CellParameters,
    lattice: &Lattice,
    options: &CifOptions,
    declared: Option<&[(Element, f64)]>,
    tag: &str,
    value: &str,
    candidates: Vec<Candidate>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(Candidate, Assembly), Error> {
    if candidates.len() == 1 {
        let candidate = candidates.into_iter().next().expect("one candidate");
        let assembly = assemble(rows, &candidate.operations, lattice, options, declared)?;
        return Ok((candidate, assembly));
    }
    let axes = cell_axes(cell);
    let unique = unique_axis(cell);
    type Score = (
        bool,
        bool,
        usize,
        usize,
        bool,
        bool,
        bool,
        bool,
        bool,
        usize,
    );
    let mut scored: Vec<(Score, Candidate, Assembly)> = Vec::new();
    let mut first_error = None;
    let names: Vec<&str> = candidates.iter().map(|c| c.setting.name).collect();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let assembly = match assemble(rows, &candidate.operations, lattice, options, declared) {
            Ok(assembly) => assembly,
            Err(e) => {
                first_error.get_or_insert(e);
                continue;
            }
        };
        let (expanded, closed) = listing_fit(rows, &candidate.operations, options.site_tolerance);
        let off_axes = matches!(
            (candidate.extension(), axes),
            ("H", Some("R")) | ("R", Some("H"))
        );
        let off_unique_axis = matches!(
            (candidate.unique_axis(), unique),
            (Some(setting), Some(cell)) if setting != cell
        );
        let off_formula = declared.is_some_and(|declared| !matches_declared(&assembly, declared));
        let score = (
            expanded && !closed,
            !(expanded && closed),
            assembly.collisions,
            assembly.multiplicity_mismatches,
            off_formula,
            off_axes,
            off_unique_axis,
            !candidate.explicit,
            !candidate.setting.reference,
            index,
        );
        scored.push((score, candidate, assembly));
    }
    if scored.is_empty() {
        return Err(first_error.expect("every candidate failed with an error"));
    }
    scored.sort_by_key(|(score, ..)| *score);
    let mut scored = scored.into_iter();
    let (best, candidate, assembly) = scored.next().expect("one candidate");
    let reason = match scored.next() {
        None => "the other settings cannot hold the listed atoms".to_string(),
        Some((runner_up, ..)) => {
            if best.0 != runner_up.0 {
                "the others do not map the already expanded listing onto itself".into()
            } else if best.1 != runner_up.1 {
                "the listing is already expanded and only this setting maps it onto itself".into()
            } else if best.2 != runner_up.2 {
                "the others put atoms on top of each other".into()
            } else if best.3 != runner_up.3 {
                "its orbits match the listed multiplicities".into()
            } else if best.4 != runner_up.4 {
                "it holds the cell's declared contents".into()
            } else if best.5 != runner_up.5 {
                let shape = if axes == Some("R") {
                    "rhombohedral"
                } else {
                    "hexagonal"
                };
                format!("the cell has {shape} axes")
            } else if best.6 != runner_up.6 {
                let axis = ["a", "b", "c"][unique.expect("the unique axis told them apart")];
                format!("the cell's unique axis is {axis}")
            } else if best.7 != runner_up.7 {
                "the file names it".into()
            } else if best.8 != runner_up.8 {
                "it is the reference setting".into()
            } else {
                "it comes first in the table".into()
            }
        }
    };
    diagnostics.push(Diagnostic::new(
        DiagnosticKind::SettingChosen,
        format!(
            "{tag} {value:?} admits the settings {}; {} is read: {reason}",
            names.join(", "),
            candidate.setting.name
        ),
    ));
    Ok((candidate, assembly))
}
