//! Python bindings (`cifmi._core`): structures as read-only objects with
//! the fields of the Node addon's. The package in `python/cifmi` re-exports
//! them.

use crate::{CifOptions, Error, SymmetrySource as Source};
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use std::path::PathBuf;

fn to_py_err(err: Error) -> PyErr {
    match err {
        Error::Io(err) => PyIOError::new_err(err.to_string()),
        other => PyValueError::new_err(other.to_string()),
    }
}

fn cif_options(block: Option<String>) -> CifOptions {
    CifOptions {
        block,
        ..CifOptions::default()
    }
}

/// Cell edge lengths in Å and angles in degrees, as the file gives them.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct Cell {
    a: f64,
    b: f64,
    c: f64,
    alpha: f64,
    beta: f64,
    gamma: f64,
}

/// The space group as the file names it; what it leaves out is None.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct SpaceGroup {
    /// Number in the International Tables, 1 to 230.
    number: Option<u8>,
    /// Hermann-Mauguin symbol, as written.
    symbol: Option<String>,
    /// Hall symbol, as written.
    hall_symbol: Option<String>,
}

/// Where the operations came from: "listed" in the file under `tag`,
/// looked up in a "table" from the name or number `value` given under `tag`,
/// or "p1" when nothing usable was given.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct SymmetrySource {
    kind: &'static str,
    tag: Option<String>,
    value: Option<String>,
}

/// A species on a site.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct Species {
    /// Label of the file's row this species comes from; of the first, when
    /// rows of one species merge.
    label: String,
    /// Element symbol, such as "Fe"; "D" for deuterium.
    element: &'static str,
    /// Share of the site, 0 to 1.
    occupancy: f64,
    /// Oxidation state from the file; 0 when absent.
    oxidation: f64,
    /// Isotropic displacement B = 8π²U in Å²; 0 when absent.
    b_iso: f64,
}

/// A site of the asymmetric unit.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct Site {
    /// Label of the first row on the site.
    label: String,
    /// Fractional coordinates the site's atoms are generated from.
    fractional: [f64; 3],
    species: Vec<Species>,
}

/// x' = R x + t on fractional coordinates.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct Operation {
    rotation: [[i32; 3]; 3],
    translation: [f64; 3],
}

/// An atom of the unit cell: an image of a site.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct Atom {
    /// Index of its site in `sites`.
    site: usize,
    /// Fractional coordinates, wrapped into the cell.
    fractional: [f64; 3],
}

/// Something the file left open or got wrong, reported rather than fixed.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct Diagnostic {
    /// A stable name, such as "setting-chosen".
    kind: &'static str,
    message: String,
}

/// A crystal structure read from one CIF data block.
#[pyclass(module = "cifmi._core", frozen, get_all)]
pub struct Structure {
    /// Name of the data block, without `data_`.
    name: String,
    cell: Cell,
    /// Cell vectors a, b, c as rows in Cartesian Å, set from `cell` with c
    /// along z and a in the xz plane.
    vectors: [[f64; 3]; 3],
    space_group: SpaceGroup,
    symmetry_source: SymmetrySource,
    /// The asymmetric unit: the file's atom rows, those on one position (up
    /// to symmetry) merged into one site.
    sites: Vec<Site>,
    /// Operations that generate `atoms` from `sites`.
    operations: Vec<Operation>,
    /// Every atom of the unit cell.
    atoms: Vec<Atom>,
    diagnostics: Vec<Diagnostic>,
}

#[pymethods]
impl Structure {
    fn __repr__(&self) -> String {
        format!(
            "<cifmi.Structure {:?}: {} sites, {} atoms, {} operations>",
            self.name,
            self.sites.len(),
            self.atoms.len(),
            self.operations.len(),
        )
    }
}

impl From<crate::Structure> for Structure {
    fn from(s: crate::Structure) -> Self {
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
            vectors: *s.lattice.matrix(),
            space_group: SpaceGroup {
                number: s.space_group.number,
                symbol: s.space_group.hermann_mauguin,
                hall_symbol: s.space_group.hall,
            },
            symmetry_source: SymmetrySource { kind, tag, value },
            sites: s
                .sites
                .into_iter()
                .map(|site| Site {
                    label: site.label,
                    fractional: site.position,
                    species: site
                        .occupants
                        .iter()
                        .map(|o| Species {
                            label: o.label.clone(),
                            element: o.element.symbol(),
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
                    rotation: op.rotation,
                    translation: op.translation,
                })
                .collect(),
            atoms: s
                .atoms
                .iter()
                .map(|atom| Atom {
                    site: atom.site,
                    fractional: atom.position,
                })
                .collect(),
            diagnostics: s
                .diagnostics
                .into_iter()
                .map(|d| Diagnostic {
                    kind: d.kind.code(),
                    message: d.message,
                })
                .collect(),
        }
    }
}

/// Read the first structure in a CIF file; `block` names the data block to
/// read, by default the first that lists atom positions and yields a
/// structure.
#[pyfunction]
#[pyo3(signature = (path, *, block=None))]
fn read_cif(path: PathBuf, block: Option<String>) -> PyResult<Structure> {
    let options = cif_options(block);
    crate::read_cif(&path, &options)
        .map(Structure::from)
        .map_err(to_py_err)
}

/// Read the first structure in CIF text (see `read_cif`).
#[pyfunction]
#[pyo3(signature = (text, *, block=None))]
fn read_cif_str(text: &str, block: Option<String>) -> PyResult<Structure> {
    let options = cif_options(block);
    crate::read_cif_str(text, &options)
        .map(Structure::from)
        .map_err(to_py_err)
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Structure>()?;
    m.add_class::<Cell>()?;
    m.add_class::<SpaceGroup>()?;
    m.add_class::<SymmetrySource>()?;
    m.add_class::<Site>()?;
    m.add_class::<Species>()?;
    m.add_class::<Operation>()?;
    m.add_class::<Atom>()?;
    m.add_class::<Diagnostic>()?;
    m.add_function(wrap_pyfunction!(read_cif, m)?)?;
    m.add_function(wrap_pyfunction!(read_cif_str, m)?)?;
    Ok(())
}
