//! Node.js bindings: crystal structures from CIF text or files, as plain
//! objects that survive JSON and structured cloning.

use cifmi::{CifOptions, SymmetrySource as Source};
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
    pub rotation: Vec<i32>,
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
                    rotation: op.rotation.iter().flatten().copied().collect(),
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
