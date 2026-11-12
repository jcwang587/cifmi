//! Node.js bindings: crystal structures from CIF text or files, as plain
//! objects that survive JSON and structured cloning, and plain objects of
//! that shape written as CIF.

use cifmi::{CellParameters, CifOptions, Element, Occupant, SymOp, SymmetrySource as Source};
use napi::bindgen_prelude::AsyncTask;
use napi::{Env, Task};
use napi_derive::napi;

/// How to read a CIF; every field is optional.
#[napi(object)]
#[derive(Default, Clone)]
pub struct ReadOptions {
    /// The data block to read, by name; by default the first block listing
    /// atom positions that yields a structure.
    pub block: Option<String>,
}

/// Cell edge lengths in Å and angles in degrees, as the file gives them.
#[napi(object)]
pub struct Cell {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
}

/// The space group as the file names it; what it leaves out is absent.
#[napi(object)]
pub struct SpaceGroup {
    /// Number in the International Tables, 1 to 230.
    pub number: Option<u32>,
    /// Hermann-Mauguin symbol, as written.
    pub symbol: Option<String>,
    /// Hall symbol, as written.
    pub hall_symbol: Option<String>,
}

/// Where the operations came from: "listed" in the file under `tag`,
/// looked up in a "table" from the name or number `value` given under `tag`,
/// or "p1" when nothing usable was given.
#[napi(object)]
pub struct SymmetrySource {
    pub kind: String,
    pub tag: Option<String>,
    pub value: Option<String>,
}

/// A species on a site.
#[napi(object)]
pub struct Species {
    /// Label of the file's row this species comes from; of the first, when
    /// rows of one species merge.
    pub label: String,
    /// Element symbol, such as "Fe"; "D" for deuterium.
    pub element: String,
    /// Share of the site, 0 to 1.
    pub occupancy: f64,
    /// Oxidation state from the file; 0 when absent.
    pub oxidation: f64,
    /// Isotropic displacement B = 8π²U in Å²; 0 when absent.
    pub b_iso: f64,
}

/// A site of the asymmetric unit.
#[napi(object)]
pub struct Site {
    /// Label of the first row on the site.
    pub label: String,
    /// Fractional coordinates the site's atoms are generated from.
    pub fractional: Vec<f64>,
    pub species: Vec<Species>,
}

/// x' = R x + t on fractional coordinates; `rotation` is row-major.
#[napi(object)]
pub struct Operation {
    pub rotation: Vec<f64>,
    pub translation: Vec<f64>,
}

/// An atom of the unit cell: an image of a site.
#[napi(object)]
pub struct Atom {
    /// Index of its site in `sites`.
    pub site: u32,
    /// Fractional coordinates, wrapped into the cell.
    pub fractional: Vec<f64>,
}

/// Something the file left open or got wrong, reported rather than fixed.
#[napi(object)]
pub struct Diagnostic {
    /// A stable name, such as "setting-chosen".
    pub kind: String,
    pub message: String,
}

/// A crystal structure read from one CIF data block.
#[napi(object)]
pub struct Structure {
    /// Name of the data block, without `data_`.
    pub name: String,
    pub cell: Cell,
    /// Cell vectors a, b, c as rows in Cartesian Å, set from `cell` with c
    /// along z and a in the xz plane.
    pub vectors: Vec<Vec<f64>>,
    pub space_group: SpaceGroup,
    pub symmetry_source: SymmetrySource,
    /// The asymmetric unit: the file's atom rows, those on one position (up
    /// to symmetry) merged into one site.
    pub sites: Vec<Site>,
    /// Operations that generate `atoms` from `sites`.
    pub operations: Vec<Operation>,
    /// Every atom of the unit cell.
    pub atoms: Vec<Atom>,
    pub diagnostics: Vec<Diagnostic>,
}

impl From<cifmi::Structure> for Structure {
    fn from(s: cifmi::Structure) -> Self {
        let (kind, tag, value) = match s.symmetry_source {
            Source::Listed { tag } => ("listed", Some(tag), None),
            Source::Table { tag, value } => ("table", Some(tag), Some(value)),
            Source::P1 => ("p1", None, None),
            Source::Given => ("given", None, None),
        };
        Structure {
            name: s.name,
            cell: Cell {
                a: s.cell.a,
                b: s.cell.b,
                c: s.cell.c,
                alpha: s.cell.alpha,
                beta: s.cell.beta,
                gamma: s.cell.gamma,
            },
            vectors: s.lattice.matrix().iter().map(|row| row.to_vec()).collect(),
            space_group: SpaceGroup {
                number: s.space_group.number.map(u32::from),
                symbol: s.space_group.hermann_mauguin,
                hall_symbol: s.space_group.hall,
            },
            symmetry_source: SymmetrySource {
                kind: kind.to_string(),
                tag,
                value,
            },
            sites: s
                .sites
                .into_iter()
                .map(|site| Site {
                    label: site.label,
                    fractional: site.position.to_vec(),
                    species: site
                        .occupants
                        .iter()
                        .map(|o| Species {
                            label: o.label.clone(),
                            element: o.element.symbol().to_string(),
                            occupancy: o.occupancy,
                            oxidation: o.oxidation,
                            b_iso: o.b_iso,
                        })
                        .collect(),
                })
                .collect(),
            operations: s
                .operations
                .iter()
                .map(|op| Operation {
                    rotation: op
                        .rotation
                        .iter()
                        .flatten()
                        .map(|&r| f64::from(r))
                        .collect(),
                    translation: op.translation.to_vec(),
                })
                .collect(),
            atoms: s
                .atoms
                .iter()
                .map(|atom| Atom {
                    site: atom.site as u32,
                    fractional: atom.position.to_vec(),
                })
                .collect(),
            diagnostics: s
                .diagnostics
                .into_iter()
                .map(|d| Diagnostic {
                    kind: d.kind.code().to_string(),
                    message: d.message,
                })
                .collect(),
        }
    }
}

fn to_napi_error(err: cifmi::Error) -> napi::Error {
    napi::Error::from_reason(err.to_string())
}

fn cif_options(options: Option<ReadOptions>) -> CifOptions {
    let options = options.unwrap_or_default();
    CifOptions {
        block: options.block,
        ..CifOptions::default()
    }
}

/// Read the first structure in CIF text (synchronous; a typical file takes
/// well under a millisecond).
#[napi]
pub fn read_cif_str(text: String, options: Option<ReadOptions>) -> napi::Result<Structure> {
    let options = cif_options(options);
    cifmi::read_cif_str(&text, &options)
        .map(Structure::from)
        .map_err(to_napi_error)
}

pub struct ReadFile {
    path: String,
    options: CifOptions,
}

impl Task for ReadFile {
    type Output = cifmi::Structure;
    type JsValue = Structure;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        cifmi::read_cif(&self.path, &self.options).map_err(to_napi_error)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(Structure::from(output))
    }
}

/// Read the first structure in a CIF file, off the main thread.
#[napi(ts_return_type = "Promise<Structure>")]
pub fn read_cif_file(
    path: String,
    options: Option<ReadOptions>,
) -> napi::Result<AsyncTask<ReadFile>> {
    Ok(AsyncTask::new(ReadFile {
        path,
        options: cif_options(options),
    }))
}

/// The parts of a structure the writer reads: any object of the shape
/// `readCifStr` returns, whose other fields are not read.
#[napi(object)]
pub struct StructureParts {
    /// Name of the data block, without `data_`.
    pub name: String,
    pub cell: Cell,
    pub operations: Vec<Operation>,
    /// Each site's label is its first species' label.
    pub sites: Vec<Site>,
}

/// Three numbers of a plain object, or the reason they are not.
fn triple(values: &[f64], what: &str) -> napi::Result<[f64; 3]> {
    values.try_into().map_err(|_| {
        napi::Error::from_reason(format!("{what} has {} numbers, not 3", values.len()))
    })
}

/// The structure of plain parts, checked as `cifmi::Structure::new` checks
/// them.
fn structure(parts: StructureParts) -> napi::Result<cifmi::Structure> {
    let operations = parts
        .operations
        .iter()
        .map(|op| {
            let integers = op
                .rotation
                .iter()
                .map(|&r| (r.fract() == 0.0 && r.abs() <= f64::from(i32::MAX)).then_some(r as i32))
                .collect::<Option<Vec<i32>>>();
            let r: [i32; 9] = integers.and_then(|r| r.try_into().ok()).ok_or_else(|| {
                napi::Error::from_reason(format!(
                    "an operation's rotation {:?} is not 9 integers",
                    op.rotation
                ))
            })?;
            Ok(SymOp {
                rotation: [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]],
                translation: triple(&op.translation, "an operation's translation")?,
            })
        })
        .collect::<napi::Result<Vec<_>>>()?;
    let sites = parts
        .sites
        .into_iter()
        .map(|site| {
            let position = triple(
                &site.fractional,
                &format!("site {}'s fractional coordinates", site.label),
            )?;
            let occupants = site
                .species
                .into_iter()
                .map(|s| {
                    Ok(Occupant {
                        element: Element::from_symbol(&s.element).map_err(to_napi_error)?,
                        label: s.label,
                        oxidation: s.oxidation,
                        occupancy: s.occupancy,
                        b_iso: s.b_iso,
                    })
                })
                .collect::<napi::Result<_>>()?;
            Ok(cifmi::Site {
                label: site.label,
                position,
                occupants,
            })
        })
        .collect::<napi::Result<Vec<_>>>()?;
    let cell = parts.cell;
    cifmi::Structure::new(
        parts.name,
        CellParameters {
            a: cell.a,
            b: cell.b,
            c: cell.c,
            alpha: cell.alpha,
            beta: cell.beta,
            gamma: cell.gamma,
        },
        operations,
        sites,
    )
    .map_err(to_napi_error)
}

/// A structure as CIF 1.1 text (synchronous): one data block in the core
/// dictionary's names, in the structure's own setting, which `readCifStr`
/// reads back as the same structure. A structure that cannot be written so
/// is an error with the reason.
#[napi]
pub fn write_cif_str(structure: StructureParts) -> napi::Result<String> {
    cifmi::write_cif_str(&self::structure(structure)?).map_err(to_napi_error)
}

pub struct WriteFile {
    path: String,
    structure: cifmi::Structure,
}

impl Task for WriteFile {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> napi::Result<Self::Output> {
        cifmi::write_cif(&self.path, &self.structure).map_err(to_napi_error)
    }

    fn resolve(&mut self, _env: Env, _output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(())
    }
}

/// Write a structure to a CIF file off the main thread, as `writeCifStr`
/// writes it.
#[napi(ts_return_type = "Promise<void>")]
pub fn write_cif_file(
    path: String,
    structure: StructureParts,
) -> napi::Result<AsyncTask<WriteFile>> {
    Ok(AsyncTask::new(WriteFile {
        path,
        structure: self::structure(structure)?,
    }))
}
