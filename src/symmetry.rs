//! Symmetry: the operations a file lists, checked to form a group, or the
//! settings its space-group name admits in gemmi's table of settings.

use crate::document::Block;
use crate::spacegroups::{Setting, SETTINGS, SETTINGS_BY_HALL, SETTINGS_BY_SYMBOL};
use crate::structure::{Diagnostic, DiagnosticKind, SpaceGroupNames};
use crate::symop::{table_operations, SymOp};
use crate::Error;
use rustc_hash::FxHashMap;

/// Data names of listed operations, in the order they are looked for.
const OPERATION_TAGS: [&str; 2] = [
    "_symmetry_equiv_pos_as_xyz",
    "_space_group_symop_operation_xyz",
];
/// Hermann-Mauguin symbol data names, in the order they are looked for.
const HERMANN_MAUGUIN_TAGS: [&str; 3] = [
    "_symmetry_space_group_name_h-m",
    "_space_group_name_h-m_alt",
    "_space_group_name_h-m_full",
];
const HALL_TAGS: [&str; 2] = ["_space_group_name_hall", "_symmetry_space_group_name_hall"];
const NUMBER_TAGS: [&str; 2] = ["_space_group_it_number", "_symmetry_int_tables_number"];

/// Listed translations within this of a multiple of 1/24, as rounded values
/// such as 0.3333 are, are read as that multiple: every space-group setting
/// translates by multiples of 1/24.
const TRANSLATION_TOLERANCE: f64 = 1e-3;

/// Where a structure's operations come from, before its atoms settle a
/// choice the file leaves open.
pub(crate) enum Source {
    /// Listed in the file under `tag`, and checked to form a group.
    Listed {
        tag: &'static str,
        operations: Vec<SymOp>,
    },
    /// The settings the space group named under `tag` admits, with distinct
    /// operations, in the table's order.
    Named {
        tag: &'static str,
        value: String,
        candidates: Vec<Candidate>,
    },
    /// The file gives no symmetry.
    Unspecified,
}

/// A setting a space-group name admits.
pub(crate) struct Candidate {
    pub(crate) setting: &'static Setting,
    pub(crate) operations: Vec<SymOp>,
    /// The file names this very setting: by Hall symbol, or by a
    /// Hermann-Mauguin symbol with its origin choice or axes.
    pub(crate) explicit: bool,
}

impl Candidate {
    fn new(index: u16, explicit: bool) -> Self {
        let setting = &SETTINGS[index as usize];
        Candidate {
            setting,
            operations: table_operations(setting.operations),
            explicit,
        }
    }

    /// Origin choice or axes: `"1"`, `"2"`, `"H"`, `"R"`, or `""`.
    pub(crate) fn extension(&self) -> &'static str {
        extension(self.setting.name)
    }

    /// The unique axis of a monoclinic setting, 0, 1 or 2 for a, b or c.
    pub(crate) fn unique_axis(&self) -> Option<usize> {
        (3..=15)
            .contains(&self.setting.number)
            .then(|| unique_axis(self.setting.name))
            .flatten()
    }
}

fn extension(name: &str) -> &str {
    name.rsplit_once(':').map_or("", |(_, e)| e)
}

/// Where the one non-trivial element of a full monoclinic symbol stands:
/// `P 1 21/c 1` is on b (1).
fn unique_axis(name: &str) -> Option<usize> {
    let parts: Vec<&str> = name.split_whitespace().collect();
    let &[_, p, q, r] = parts.as_slice() else {
        return None;
    };
    match (p != "1", q != "1", r != "1") {
        (true, false, false) => Some(0),
        (false, true, false) => Some(1),
        (false, false, true) => Some(2),
        _ => None,
    }
}

/// A setting's Hermann-Mauguin symbol without its origin choice or axes.
fn base_name(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

/// The first of `tags` (normalized) the block gives a value for, with the
/// value as written.
fn first_named(block: &Block<'_>, tags: &[&'static str]) -> Option<(&'static str, String)> {
    tags.iter().find_map(|&tag| {
        let value = block.value(tag)?;
        let text = value.text.trim();
        (!value.is_placeholder() && !text.is_empty()).then(|| (tag, text.to_string()))
    })
}

/// The space group as the block names it.
pub(crate) fn names(block: &Block<'_>) -> SpaceGroupNames {
    SpaceGroupNames {
        number: first_named(block, &NUMBER_TAGS).and_then(|(_, text)| table_number(&text)),
        hermann_mauguin: first_named(block, &HERMANN_MAUGUIN_TAGS).map(|(_, text)| text),
        hall: first_named(block, &HALL_TAGS).map(|(_, text)| text),
    }
}

fn table_number(text: &str) -> Option<u8> {
    let v: f64 = text.split('(').next()?.trim().parse().ok()?;
    (v.fract() == 0.0 && (1.0..=230.0).contains(&v)).then_some(v as u8)
}

/// Where the block's operations come from. Listed operations that cannot
/// be read are an error.
pub(crate) fn resolve(
    block: &Block<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Source, Error> {
    for tag in OPERATION_TAGS {
        let Some(column) = block.column(tag) else {
            continue;
        };
        let parsed: Result<Vec<SymOp>, Error> = column
            .iter()
            .filter(|v| !v.is_placeholder())
            .map(|v| SymOp::parse(&v.text))
            .collect();
        match parsed {
            Ok(operations) if operations.is_empty() => {}
            Ok(operations) => {
                let operations = checked_group(operations, tag, diagnostics)?;
                if let Ok(Some(named)) = named(block) {
                    if !named
                        .candidates
                        .iter()
                        .any(|c| same_group(&c.operations, &operations))
                    {
                        diagnostics.push(Diagnostic::new(
                            DiagnosticKind::SpaceGroupMismatch,
                            format!(
                                "the operations under {tag} are not those of {} {:?}; the listed operations are read",
                                named.tag, named.value
                            ),
                        ));
                    }
                }
                return Ok(Source::Listed { tag, operations });
            }
            Err(err) => return Err(Error::cif(format!("{tag}: {}", reason(&err)))),
        }
    }
    match named(block)? {
        Some(named) => {
            diagnostics.extend(named.notes);
            Ok(Source::Named {
                tag: named.tag,
                value: named.value,
                candidates: named.candidates,
            })
        }
        None => Ok(Source::Unspecified),
    }
}

/// An error's reason without its kind.
fn reason(err: &Error) -> String {
    match err {
        Error::Cif(message) | Error::Invalid(message) => message.clone(),
        other => other.to_string(),
    }
}

struct Named {
    tag: &'static str,
    value: String,
    candidates: Vec<Candidate>,
    notes: Vec<Diagnostic>,
}

/// The settings the block's space-group names admit: a Hall symbol's, else a
/// Hermann-Mauguin symbol's (narrowed by its origin choice or axes), else a
/// number's reference symbol's. Names that disagree are reported, and the
/// settings of each stay candidates for the atoms to settle. `None` when the
/// block names no space group.
fn named(block: &Block<'_>) -> Result<Option<Named>, Error> {
    let hall = first_named(block, &HALL_TAGS);
    let symbol = first_named(block, &HERMANN_MAUGUIN_TAGS);
    let number = first_named(block, &NUMBER_TAGS);
    let mut notes = Vec::new();
    let unknown = |tag: &str, text: &str| format!("{tag} {text:?} is not in the table of settings");

    let hall_settings = hall
        .as_ref()
        .and_then(|(_, text)| lookup(&SETTINGS_BY_HALL, &hall_key(text)));
    let symbol_settings = symbol.as_ref().map(|(_, text)| symbol_settings(text));
    let symbol_explicit = symbol
        .as_ref()
        .is_some_and(|(_, text)| split_extension(text).1.is_some());
    let number_value = number.as_ref().and_then(|(_, text)| table_number(text));

    let (tag, value, mut candidates) = if let (Some((tag, text)), Some(found)) =
        (&hall, hall_settings)
    {
        let mut candidates: Vec<Candidate> =
            found.iter().map(|&i| Candidate::new(i, true)).collect();
        match (&symbol, &symbol_settings) {
            (Some((symbol_tag, symbol_text)), Some(Some(others))) => {
                let agree = others.iter().any(|&i| {
                    let operations = table_operations(SETTINGS[i as usize].operations);
                    candidates
                        .iter()
                        .any(|c| same_group(&c.operations, &operations))
                });
                if !agree {
                    notes.push(Diagnostic::new(
                        DiagnosticKind::SpaceGroupMismatch,
                        format!("{tag} {text:?} and {symbol_tag} {symbol_text:?} name different settings; the atoms decide"),
                    ));
                    candidates.extend(others.iter().map(|&i| Candidate::new(i, symbol_explicit)));
                }
            }
            (Some((symbol_tag, symbol_text)), Some(None)) => notes.push(Diagnostic::new(
                DiagnosticKind::SpaceGroupMismatch,
                format!(
                    "{}; the Hall symbol is read",
                    unknown(symbol_tag, symbol_text)
                ),
            )),
            _ => {}
        }
        (*tag, text.clone(), candidates)
    } else {
        if let Some((tag, text)) = &hall {
            notes.push(Diagnostic::new(
                DiagnosticKind::SpaceGroupMismatch,
                format!("{}; the other names are read", unknown(tag, text)),
            ));
        }
        match (&symbol, &symbol_settings, number_value) {
            (Some((tag, text)), Some(Some(found)), _) => (
                *tag,
                text.clone(),
                found
                    .iter()
                    .map(|&i| Candidate::new(i, symbol_explicit))
                    .collect(),
            ),
            (Some((tag, text)), _, Some(n)) => {
                notes.push(Diagnostic::new(
                    DiagnosticKind::SpaceGroupMismatch,
                    format!("{}; the number {n} is read", unknown(tag, text)),
                ));
                let (number_tag, number_text) = number.clone().expect("a number was read");
                (number_tag, number_text, number_settings(n))
            }
            (Some((tag, text)), _, None) => return Err(Error::cif(unknown(tag, text))),
            (None, _, Some(n)) => {
                let (number_tag, number_text) = number.clone().expect("a number was read");
                (number_tag, number_text, number_settings(n))
            }
            (None, _, None) => match (&hall, &number) {
                (Some((tag, text)), _) => return Err(Error::cif(unknown(tag, text))),
                (None, Some((tag, text))) => {
                    return Err(Error::cif(format!(
                        "{tag} {text:?} is not a space-group number"
                    )))
                }
                (None, None) => return Ok(None),
            },
        }
    };
    if let Some(n) = number_value {
        if !candidates.iter().any(|c| c.setting.number == n) {
            notes.push(Diagnostic::new(
                DiagnosticKind::SpaceGroupMismatch,
                format!(
                    "{tag} {value:?} names space group {}, but the number given is {n}; the atoms decide",
                    candidates[0].setting.number
                ),
            ));
            candidates.extend(number_settings(n));
        }
    }

    // The other origin choice or axes of a named setting stay candidates, for
    // when the atoms contradict the name.
    let siblings: Vec<Candidate> = candidates
        .iter()
        .filter(|c| c.explicit)
        .flat_map(|c| {
            let base = base_name(c.setting.name);
            let number = c.setting.number;
            SETTINGS
                .iter()
                .enumerate()
                .filter(move |(_, s)| s.number == number && base_name(s.name) == base)
        })
        .map(|(i, _)| Candidate::new(i as u16, false))
        .collect();
    candidates.extend(siblings);

    // Settings with the same operations are one choice.
    let mut distinct: Vec<Candidate> = Vec::new();
    for candidate in candidates.drain(..) {
        if !distinct
            .iter()
            .any(|d| same_group(&d.operations, &candidate.operations))
        {
            distinct.push(candidate);
        }
    }
    Ok(Some(Named {
        tag,
        value,
        candidates: distinct,
        notes,
    }))
}

fn lookup(table: &'static [(&'static str, &'static [u16])], key: &str) -> Option<&'static [u16]> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| table[i].1)
}

/// A Hermann-Mauguin symbol as the table indexes it: no whitespace or
/// underscores, lowercase.
fn symbol_key(symbol: &str) -> String {
    symbol
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// A Hall symbol as the table indexes it: whitespace collapsed to single
/// spaces (they separate generators: `P 32` is not `P 3 2`), lowercase.
fn hall_key(symbol: &str) -> String {
    symbol
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The settings a Hermann-Mauguin symbol admits, narrowed by its origin
/// choice or axes when it gives them (`R -3:H`, `R -3 H`, `F d -3 m:2`;
/// `:S` and `:Z` are origin choices 1 and 2). A short monoclinic symbol is
/// on unique axis b where the table knows it so (`P 21/c` is `P 1 21/c 1`);
/// `:a`, `:b` and `:c` put it on that axis (`P 2/m:c` is `P 1 1 2/m`), and
/// one the table does not know on b admits every axis that has it (`P 21/b`
/// is `P 1 1 21/b` or `P 21/b 1 1`).
fn symbol_settings(symbol: &str) -> Option<Vec<u16>> {
    let (base, choice) = split_extension(symbol);
    if let Some(axis) = choice.as_deref().and_then(axis_choice) {
        return monoclinic_settings(base, Some(axis));
    }
    let key = symbol_key(base);
    let found = lookup(&SETTINGS_BY_SYMBOL, &key).or_else(|| {
        // Older tables print the cubic groups m-3 and m-3m without the bar:
        // F d 3 m is F d -3 m.
        let bytes = key.as_bytes();
        let old_cubic = bytes.len() >= 3
            && b"pifabcr".contains(&bytes[0])
            && b"mnda".contains(&bytes[1])
            && bytes[2] == b'3';
        old_cubic
            .then(|| lookup(&SETTINGS_BY_SYMBOL, &format!("{}-{}", &key[..2], &key[2..])))
            .flatten()
    });
    let Some(found) = found else {
        return choice
            .is_none()
            .then(|| monoclinic_settings(base, None))
            .flatten();
    };
    let found: Vec<u16> = match choice {
        Some(choice) => found
            .iter()
            .copied()
            .filter(|&i| extension(SETTINGS[i as usize].name).eq_ignore_ascii_case(&choice))
            .collect(),
        None => found.to_vec(),
    };
    (!found.is_empty()).then_some(found)
}

/// The unique axis `:a`, `:b` or `:c` names, as `split_extension` gives it.
fn axis_choice(choice: &str) -> Option<usize> {
    match choice {
        "A" => Some(0),
        "B" => Some(1),
        "C" => Some(2),
        _ => None,
    }
}

/// The settings of a monoclinic symbol on its unique axis: a short symbol
/// (`P 21/c`) placed on `axis`, or on every axis that has it, b first; a
/// full one (`P 1 21/c 1`) as written, when that is `axis`.
fn monoclinic_settings(symbol: &str, axis: Option<usize>) -> Option<Vec<u16>> {
    let parts: Vec<&str> = symbol.split_whitespace().collect();
    let found: Vec<u16> = match parts.as_slice() {
        &[lattice, element] => {
            let axes: &[usize] = match &axis {
                Some(a) => std::slice::from_ref(a),
                None => &[1, 2, 0],
            };
            axes.iter()
                .flat_map(|&a| {
                    let mut full = [lattice, "1", "1", "1"];
                    full[a + 1] = element;
                    lookup(&SETTINGS_BY_SYMBOL, &symbol_key(&full.join(" ")))
                        .into_iter()
                        .flatten()
                        .copied()
                })
                .collect()
        }
        [_, _, _, _] => lookup(&SETTINGS_BY_SYMBOL, &symbol_key(symbol))?
            .iter()
            .copied()
            .filter(|&i| axis.is_none_or(|a| unique_axis(SETTINGS[i as usize].name) == Some(a)))
            .collect(),
        _ => return None,
    };
    (!found.is_empty()).then_some(found)
}

fn split_extension(symbol: &str) -> (&str, Option<String>) {
    let symbol = symbol.trim();
    let normalize = |e: &str| match e.trim().to_ascii_uppercase().as_str() {
        "S" => "1".to_string(),
        "Z" => "2".to_string(),
        other => other.to_string(),
    };
    if let Some((base, choice)) = symbol.rsplit_once(':') {
        return (base.trim(), Some(normalize(choice)));
    }
    if let Some((base, last)) = symbol.rsplit_once(char::is_whitespace) {
        if matches!(last, "H" | "R" | "h" | "r") && base.contains(char::is_whitespace) {
            return (base.trim(), Some(normalize(last)));
        }
    }
    (symbol, None)
}

/// The settings of number `n` that share its reference setting's symbol:
/// that setting and its other origin choice or axes.
fn number_settings(n: u8) -> Vec<Candidate> {
    let reference = SETTINGS
        .iter()
        .find(|s| s.number == n && s.reference)
        .expect("every number has a reference setting");
    SETTINGS
        .iter()
        .enumerate()
        .filter(|(_, s)| s.number == n && base_name(s.name) == base_name(reference.name))
        .map(|(i, _)| Candidate::new(i as u16, false))
        .collect()
}

/// `op` with translations near a multiple of 1/24 made exact.
fn idealized(op: SymOp) -> SymOp {
    SymOp {
        rotation: op.rotation,
        translation: op.translation.map(|t| {
            let k = (t * 24.0).round();
            if (t * 24.0 - k).abs() <= 24.0 * TRANSLATION_TOLERANCE {
                k / 24.0
            } else {
                t
            }
        }),
    }
}

/// The same operation up to lattice translations.
fn same_op(a: &SymOp, b: &SymOp) -> bool {
    a.rotation == b.rotation
        && (0..3).all(|i| {
            let d = a.translation[i] - b.translation[i];
            (d - d.round()).abs() <= TRANSLATION_TOLERANCE
        })
}

/// The same operations up to order and lattice translations.
pub(crate) fn same_group(a: &[SymOp], b: &[SymOp]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| same_op(x, y)))
}

/// `a` after `b`: x ↦ a(b(x)).
fn compose(a: &SymOp, b: &SymOp) -> SymOp {
    let mut rotation = [[0; 3]; 3];
    for (i, row) in rotation.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a.rotation[i][k] * b.rotation[k][j]).sum();
        }
    }
    SymOp {
        rotation,
        translation: a.apply(b.translation),
    }
}

/// Listed operations with near-1/24 translations made exact and repeats
/// dropped, checked to form a group up to lattice translations.
fn checked_group(
    operations: Vec<SymOp>,
    tag: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<SymOp>, Error> {
    let mut unique: Vec<SymOp> = Vec::with_capacity(operations.len());
    let mut repeats = 0;
    for op in operations.into_iter().map(idealized) {
        if unique.iter().any(|u| same_op(u, &op)) {
            repeats += 1;
        } else {
            unique.push(op);
        }
    }
    if repeats > 0 {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::DuplicateOperation,
            format!("{tag} lists {repeats} operation(s) more than once; each is read once"),
        ));
    }
    let mut by_rotation: FxHashMap<[[i32; 3]; 3], Vec<&SymOp>> = FxHashMap::default();
    for op in &unique {
        by_rotation.entry(op.rotation).or_default().push(op);
    }
    for a in &unique {
        for b in &unique {
            let c = compose(a, b);
            let listed = by_rotation
                .get(&c.rotation)
                .is_some_and(|ops| ops.iter().any(|op| same_op(op, &c)));
            if !listed {
                return Err(Error::cif(format!(
                    "{tag}: the operations do not form a group: {} after {} gives {}, which is not listed",
                    xyz(a),
                    xyz(b),
                    xyz(&c)
                )));
            }
        }
    }
    Ok(unique)
}

/// An operation as CIF writes it, such as `-x+1/2,y,-z`.
pub(crate) fn xyz(op: &SymOp) -> String {
    (0..3)
        .map(|i| {
            let mut part = String::new();
            for (j, axis) in ["x", "y", "z"].iter().enumerate() {
                match op.rotation[i][j] {
                    0 => {}
                    1 => part += &format!("{}{axis}", if part.is_empty() { "" } else { "+" }),
                    -1 => part += &format!("-{axis}"),
                    c => part += &format!("{c:+}{axis}"),
                }
            }
            let t = op.translation[i];
            if t != 0.0 {
                let k = t * 24.0;
                let text = if (k - k.round()).abs() < 1e-9 {
                    let (mut n, mut d) = (k.round() as i64, 24i64);
                    let g = gcd(n.abs(), d);
                    n /= g;
                    d /= g;
                    if d == 1 {
                        format!("{n}")
                    } else {
                        format!("{n}/{d}")
                    }
                } else {
                    format!("{t}")
                };
                if text.starts_with('-') || part.is_empty() {
                    part += &text;
                } else {
                    part += &format!("+{text}");
                }
            }
            if part.is_empty() {
                part.push('0');
            }
            part
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document;

    fn block(text: &str) -> Block<'_> {
        document::parse(text).unwrap().remove(0)
    }

    fn settings(source: &Source) -> Vec<&'static str> {
        match source {
            Source::Named { candidates, .. } => candidates.iter().map(|c| c.setting.name).collect(),
            _ => panic!("not a named group"),
        }
    }

    #[test]
    fn listed_operations_must_form_a_group() {
        let mut diagnostics = Vec::new();
        let text = "data_x\nloop_\n_space_group_symop_operation_xyz\n'x,y,z'\n'-x,-y,-z'\n'x,y,z'\n'x+0.3333,y+0.6667,z+0.6667'\n'x+0.6667,y+0.3333,z+0.3333'\n'-x+0.3333,-y+0.6667,-z+0.6667'\n'-x+0.6667,-y+0.3333,-z+0.3333'\n";
        let Source::Listed { operations, .. } = resolve(&block(text), &mut diagnostics).unwrap()
        else {
            panic!("listed")
        };
        assert_eq!(operations.len(), 6);
        assert_eq!(operations[2].translation, [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::DuplicateOperation);

        let broken = "data_x\nloop_\n_symmetry_equiv_pos_as_xyz\nx,y,z\n-x,y+1/2,-z\nx+1/2,y,z\n";
        let err = resolve(&block(broken), &mut Vec::new())
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("do not form a group"), "{err}");
    }

    #[test]
    fn unreadable_operations_are_an_error() {
        // A name does not stand in for operations that cannot be read.
        let text = "data_x\n_symmetry_space_group_name_H-M 'P -1'\nloop_\n_symmetry_equiv_pos_as_xyz\n'x,y'\n";
        let err = resolve(&block(text), &mut Vec::new())
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.starts_with("CIF error: _symmetry_equiv_pos_as_xyz: "),
            "{err}"
        );
    }

    #[test]
    fn names_resolve_to_settings() {
        let resolve_text = |names: &str| {
            let mut diagnostics = Vec::new();
            let source = resolve(&block(&format!("data_x\n{names}")), &mut diagnostics).unwrap();
            (settings(&source), diagnostics)
        };
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'R -3'").0,
            ["R -3:H", "R -3:R"]
        );
        // An explicit origin choice or axes come first; the other stays a
        // candidate for when the atoms contradict it.
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'R -3 :R'").0,
            ["R -3:R", "R -3:H"]
        );
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'R -3 H'").0,
            ["R -3:H", "R -3:R"]
        );
        assert_eq!(
            resolve_text("_space_group_name_H-M_alt 'F d -3 m:S'").0,
            ["F d -3 m:1", "F d -3 m:2"]
        );
        assert_eq!(
            resolve_text("_space_group_name_H-M_alt 'F d 3 m'").0,
            ["F d -3 m:1", "F d -3 m:2"]
        );
        assert_eq!(
            resolve_text("_space_group_name_H-M_alt 'C m c e'").0,
            ["C m c a"]
        );
        assert_eq!(
            resolve_text("_space_group_name_Hall '-R 3'").0,
            ["R -3:H", "R -3:R"]
        );
        assert_eq!(resolve_text("_space_group_name_Hall 'P 32'").0, ["P 32"]);
        assert_eq!(
            resolve_text("_space_group_name_Hall 'P 3 2'").0,
            ["P 3 1 2"]
        );
        assert_eq!(
            resolve_text("_space_group_IT_number 227").0,
            ["F d -3 m:1", "F d -3 m:2"]
        );
        // Names that disagree are reported, and both stay candidates.
        let (found, notes) =
            resolve_text("_space_group_name_Hall '-P 2ybc'\n_symmetry_space_group_name_H-M 'P -1'");
        assert_eq!(found, ["P 1 21/c 1", "P -1"]);
        assert_eq!(notes[0].kind, DiagnosticKind::SpaceGroupMismatch);
        assert!(
            notes[0].message.ends_with("the atoms decide"),
            "{}",
            notes[0].message
        );
    }

    #[test]
    fn monoclinic_symbols_stand_on_their_unique_axis() {
        let resolve_text = |names: &str| {
            resolve(&block(&format!("data_x\n{names}")), &mut Vec::new())
                .map(|source| settings(&source))
                .map_err(|e| e.to_string())
        };
        // A short symbol the table knows is on axis b; :a, :b and :c place
        // it, and a full symbol, on that axis.
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'P 21/c'").unwrap(),
            ["P 1 21/c 1"]
        );
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'P 2/m :b'").unwrap(),
            ["P 1 2/m 1"]
        );
        assert_eq!(
            resolve_text("_space_group_name_H-M_alt 'P 2/m:c'").unwrap(),
            ["P 1 1 2/m"]
        );
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'P 21/c:a'").unwrap(),
            ["P 21/c 1 1"]
        );
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'P 1 21/c 1:b'").unwrap(),
            ["P 1 21/c 1"]
        );
        // A short symbol unknown on axis b admits every axis that has it.
        assert_eq!(
            resolve_text("_symmetry_space_group_name_H-M 'P 21/b'").unwrap(),
            ["P 1 1 21/b", "P 21/b 1 1"]
        );
        // A placement that does not exist is not in the table.
        for names in [
            "_symmetry_space_group_name_H-M 'P 21/b :b'",
            "_symmetry_space_group_name_H-M 'P 1 21/c 1:c'",
        ] {
            let err = resolve_text(names).unwrap_err();
            assert!(err.contains("not in the table of settings"), "{err}");
        }
    }

    #[test]
    fn unknown_names_are_errors() {
        let err = resolve(
            &block("data_x\n_symmetry_space_group_name_H-M 'Q 9'"),
            &mut Vec::new(),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("not in the table of settings"), "{err}");
    }

    #[test]
    fn operations_print_as_cif_writes_them() {
        assert_eq!(xyz(&SymOp::parse("-x+1/2, y, -z").unwrap()), "-x+1/2,y,-z");
        assert_eq!(xyz(&SymOp::parse("x-y, x, z+5/6").unwrap()), "x-y,x,z+5/6");
    }
}
