//! Python bindings (`cifmi._core`): structures as read-only objects with
//! the fields of the Node addon's, read from CIF or built from their parts,
//! and written as CIF. The package in `python/cifmi` re-exports them.

use crate::{
    CellParameters, CifOptions, Element, Error, Occupant, SymOp, SymmetrySource as Source,
};
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
#[pyclass(module = "cifmi._core", frozen, get_all, from_py_object)]
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
/// "p1" when nothing usable was given, or "given" with a structure built from
/// its parts.
#[pyclass(module = "cifmi._core", frozen, get_all)]
#[derive(Clone)]
pub struct SymmetrySource {
    kind: &'static str,
    tag: Option<String>,
    value: Option<String>,
}

/// A species on a site.
#[pyclass(module = "cifmi._core", frozen, get_all, from_py_object)]
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
    /// Isotropic displacement U in Å²; 0 when absent.
    u_iso: f64,
}

/// A site of the asymmetric unit.
#[pyclass(module = "cifmi._core", frozen, get_all, from_py_object)]
#[derive(Clone)]
pub struct Site {
    /// Label of the first row on the site, its first species' label.
    label: String,
    /// Fractional coordinates the site's atoms are generated from.
    fractional: [f64; 3],
    species: Vec<Species>,
}

/// x' = R x + t on fractional coordinates.
#[pyclass(module = "cifmi._core", frozen, get_all, from_py_object)]
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

/// A crystal structure read from one CIF data block, or built from its parts
/// as they are by `Structure(name, cell, operations, sites)`: the vectors set
/// from the cell and the atoms as the sites' orbits, with no space-group
/// names, the symmetry source "given" and no diagnostics. Parts that cannot
/// be written as a CIF that cifmi reads raise ValueError with the reason.
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
impl Cell {
    #[new]
    fn new(a: f64, b: f64, c: f64, alpha: f64, beta: f64, gamma: f64) -> Self {
        Cell {
            a,
            b,
            c,
            alpha,
            beta,
            gamma,
        }
    }
}

#[pymethods]
impl Species {
    /// `element` is a symbol such as "Fe", or "D" for deuterium.
    #[new]
    #[pyo3(signature = (label, element, occupancy, oxidation=0.0, u_iso=0.0))]
    fn new(
        label: String,
        element: &str,
        occupancy: f64,
        oxidation: f64,
        u_iso: f64,
    ) -> PyResult<Self> {
        Ok(Species {
            label,
            element: Element::from_symbol(element).map_err(to_py_err)?.symbol(),
            occupancy,
            oxidation,
            u_iso,
        })
    }
}

#[pymethods]
impl Site {
    #[new]
    fn new(label: String, fractional: [f64; 3], species: Vec<Species>) -> Self {
        Site {
            label,
            fractional,
            species,
        }
    }
}

#[pymethods]
impl Operation {
    #[new]
    fn new(rotation: [[i32; 3]; 3], translation: [f64; 3]) -> Self {
        Operation {
            rotation,
            translation,
        }
    }
}

#[pymethods]
impl Structure {
    #[new]
    fn new(
        name: String,
        cell: Cell,
        operations: Vec<Operation>,
        sites: Vec<Site>,
    ) -> PyResult<Self> {
        core_structure(name, &cell, &operations, &sites)
            .map(Structure::from)
            .map_err(to_py_err)
    }

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

/// The core structure of a Python one's parts, checked as
/// `crate::Structure::new` checks them.
fn core_structure(
    name: String,
    cell: &Cell,
    operations: &[Operation],
    sites: &[Site],
) -> Result<crate::Structure, Error> {
    let sites = sites
        .iter()
        .map(|site| {
            let occupants = site
                .species
                .iter()
                .map(|s| {
                    Ok(Occupant {
                        label: s.label.clone(),
                        element: Element::from_symbol(s.element)?,
                        oxidation: s.oxidation,
                        occupancy: s.occupancy,
                        u_iso: s.u_iso,
                    })
                })
                .collect::<Result<_, Error>>()?;
            Ok(crate::Site {
                label: site.label.clone(),
                position: site.fractional,
                occupants,
            })
        })
        .collect::<Result<_, Error>>()?;
    crate::Structure::new(
        name,
        CellParameters {
            a: cell.a,
            b: cell.b,
            c: cell.c,
            alpha: cell.alpha,
            beta: cell.beta,
            gamma: cell.gamma,
        },
        operations
            .iter()
            .map(|op| SymOp {
                rotation: op.rotation,
                translation: op.translation,
            })
            .collect(),
        sites,
    )
}

impl From<crate::Structure> for Structure {
    fn from(s: crate::Structure) -> Self {
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
                            u_iso: o.u_iso,
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

/// The core structure of `structure`, to write.
fn to_write(structure: &Structure) -> PyResult<crate::Structure> {
    core_structure(
        structure.name.clone(),
        &structure.cell,
        &structure.operations,
        &structure.sites,
    )
    .map_err(to_py_err)
}

/// Write a structure to a CIF file, as `write_cif_str` writes it.
#[pyfunction]
fn write_cif(path: PathBuf, structure: PyRef<'_, Structure>) -> PyResult<()> {
    crate::write_cif(&path, &to_write(&structure)?).map_err(to_py_err)
}

/// A structure, read or built, as CIF 1.1 text: one data block in the core
/// dictionary's names, in the structure's own setting, which `read_cif_str`
/// reads back as the same structure for one cifmi read. A structure that
/// cannot be written as a CIF that cifmi reads raises ValueError with the
/// reason.
#[pyfunction]
fn write_cif_str(structure: PyRef<'_, Structure>) -> PyResult<String> {
    crate::write_cif_str(&to_write(&structure)?).map_err(to_py_err)
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
    m.add_function(wrap_pyfunction!(write_cif, m)?)?;
    m.add_function(wrap_pyfunction!(write_cif_str, m)?)?;
    Ok(())
}
