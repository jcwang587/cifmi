//! Crystal structures as read from a CIF data block, or built from their
//! parts.

use crate::element::Element;
use crate::lattice::Lattice;
use crate::symop::SymOp;
use crate::writer;
use crate::Error;

/// A crystal structure read from one CIF data block: the sites the file
/// lists, the symmetry operations, and every atom of the unit cell those
/// operations generate from the sites. [`Structure::new`] builds one from
/// its parts.
#[derive(Clone, Debug)]
pub struct Structure {
    /// Name of the data block, without `data_`.
    pub name: String,
    /// The cell as the file gives it.
    pub cell: CellParameters,
    /// The cell's vectors, set from `cell` with c along z and a in the xz plane.
    pub lattice: Lattice,
    /// The space group as the file names it.
    pub space_group: SpaceGroupNames,
    /// Where `operations` came from.
    pub symmetry_source: SymmetrySource,
    /// The asymmetric unit: the file's atom rows, those on one position (up
    /// to symmetry) merged into one site, in the order the file first lists
    /// them.
    pub sites: Vec<Site>,
    /// Space-group operations that generate `atoms` from `sites`.
    pub operations: Vec<SymOp>,
    /// Every atom of the unit cell: the images of the sites, coincident
    /// images kept once, site by site in the order of `sites`.
    pub atoms: Vec<Atom>,
    /// What the reader found that the file left open or got wrong, in the
    /// order found.
    pub diagnostics: Vec<Diagnostic>,
}

impl Structure {
    /// A structure from the parts a crystal model holds: the cell, the
    /// space-group operations and the asymmetric unit, as they are. Nothing
    /// is merged or moved: the lattice is set from the cell and the atoms
    /// are the sites' orbits. The structure names no space group, its
    /// symmetry source is [`SymmetrySource::Given`] and it has no
    /// diagnostics.
    ///
    /// The parts must make a CIF that cifmi reads (see
    /// [`write_cif_str`](crate::write_cif_str)): `name` a data block code;
    /// the operations a group, each once, of which the cell has the
    /// symmetry; each site labelled as its first species, on a position of
    /// its own up to symmetry, at finite coordinates, holding each species
    /// once, with occupancies above 0 adding up to at most 1, U at least 0
    /// and finite oxidation states. Anything else is an error with the
    /// reason. The CIF is read by the reading's rules, as any file is: a
    /// translation within 0.001 of a multiple of 1/24 is read as that
    /// multiple, and a site within the site tolerance of a special position,
    /// or a coordinate near a third, moves onto it.
    pub fn new(
        name: impl Into<String>,
        cell: CellParameters,
        operations: Vec<SymOp>,
        sites: Vec<Site>,
    ) -> Result<Self, Error> {
        let name = name.into();
        let parts = writer::check(&name, &cell, &operations, &sites)?;
        let atoms = parts
            .orbits
            .into_iter()
            .enumerate()
            .flat_map(|(site, orbit)| {
                orbit
                    .into_iter()
                    .map(move |position| Atom { site, position })
            })
            .collect();
        Ok(Structure {
            name,
            cell,
            lattice: parts.lattice,
            space_group: SpaceGroupNames::default(),
            symmetry_source: SymmetrySource::Given,
            sites,
            operations,
            atoms,
            diagnostics: Vec::new(),
        })
    }
}

/// Cell edge lengths in Å and angles in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellParameters {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
}

/// The space group as a file names it; `None` for what it leaves out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpaceGroupNames {
    /// Number in the International Tables, 1 to 230.
    pub number: Option<u8>,
    /// Hermann-Mauguin symbol, as written.
    pub hermann_mauguin: Option<String>,
    /// Hall symbol, as written.
    pub hall: Option<String>,
}

/// Where a structure's symmetry operations came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymmetrySource {
    /// Listed in the file under this data name.
    Listed { tag: String },
    /// Looked up in a table from the space-group name or number the file
    /// gives under this data name.
    Table { tag: String, value: String },
    /// Nothing the file gives: the identity alone (P1). A diagnostic says
    /// why.
    P1,
    /// Given with the structure's other parts, to [`Structure::new`]; not
    /// read from a file.
    Given,
}

/// A site of the asymmetric unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Site {
    /// Label of the first row on the site.
    pub label: String,
    /// Fractional coordinates its atoms are generated from.
    pub position: [f64; 3],
    /// Species on the site, in the order the rows list them. Rows of one
    /// species add their occupancies and keep the first row's displacement.
    pub occupants: Vec<Occupant>,
}

/// A species on a site.
#[derive(Clone, Debug, PartialEq)]
pub struct Occupant {
    /// Label of the file's row this species comes from; of the first, when
    /// rows of one species merge.
    pub label: String,
    pub element: Element,
    /// Oxidation state from `_atom_type_oxidation_number`; 0 when absent.
    pub oxidation: f64,
    /// Fraction of the site this species occupies.
    pub occupancy: f64,
    /// Isotropic displacement U in Å²; 0 when absent.
    pub u_iso: f64,
}

/// An atom of the unit cell: one image of a site.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atom {
    /// Index of its site in [`Structure::sites`].
    pub site: usize,
    /// Fractional coordinates, wrapped into the cell.
    pub position: [f64; 3],
}

/// Something the reader found that the file left open or got wrong:
/// reported, never fixed quietly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    /// What was found, naming the data, rows or values involved.
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Diagnostic {
            kind,
            message: message.into(),
        }
    }
}

/// The kinds of [`Diagnostic`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// The file gives no symmetry operations, space-group name or number; it
    /// is read as P1.
    NoSymmetry,
    /// A space-group name admits several settings (origin choices, or
    /// hexagonal and rhombohedral axes); the message says which was read and
    /// why.
    SettingChosen,
    /// Names or listed operations that disagree with each other.
    SpaceGroupMismatch,
    /// A cell whose lengths or angles depart from the symmetry of its
    /// operations by less than 1 % (an angle of 90.06° under a two-fold
    /// axis), read as given; a larger departure is an error.
    CellSymmetryMismatch,
    /// A symmetry operation listed more than once.
    DuplicateOperation,
    /// A data name given more than once; the last value is read.
    DuplicateDataName,
    /// A save frame, which belongs in dictionaries, skipped.
    SkippedSaveFrame,
    /// A value the file leaves out, read as the CIF dictionary's default.
    DefaultValue,
    /// Atom rows listing only placeholder coordinates, skipped.
    PlaceholderRow,
    /// Dummy atoms (`_atom_site_calc_flag dum`), skipped.
    DummyAtom,
    /// Pseudo-atoms standing for groups (`Wat`, `OH`), read as one atom.
    PseudoAtom,
    /// Atom rows with zero occupancy, skipped.
    ZeroOccupancy,
    /// Rows repeating atoms at symmetry images of earlier rows (a listing
    /// already expanded), each atom read once.
    RepeatedAtom,
    /// Sites whose occupancies add up to more than 1, within the tolerance,
    /// scaled to a full site.
    OverfullSite,
    /// Partly occupied sites near a special position, which could be one
    /// atom on it or an atom split around it, and which the file does not
    /// place by site symmetry or multiplicity: read by their distance from
    /// it.
    AmbiguousSpecialPosition,
    /// Negative or unreadable displacement parameters, read as none.
    IgnoredDisplacement,
    /// Oxidation numbers that cannot be read; their atom types are read
    /// without one and the others keep theirs.
    IgnoredOxidationState,
    /// Listed multiplicities that differ from the orbits.
    MultiplicityMismatch,
    /// Atoms closer than 0.5 Å whose sites hold more than one atom between
    /// them.
    CollidingAtoms,
}

impl DiagnosticKind {
    /// A stable name, such as `"no-symmetry"`.
    pub fn code(self) -> &'static str {
        match self {
            DiagnosticKind::NoSymmetry => "no-symmetry",
            DiagnosticKind::SettingChosen => "setting-chosen",
            DiagnosticKind::SpaceGroupMismatch => "space-group-mismatch",
            DiagnosticKind::CellSymmetryMismatch => "cell-symmetry-mismatch",
            DiagnosticKind::DuplicateOperation => "duplicate-operation",
            DiagnosticKind::DuplicateDataName => "duplicate-data-name",
            DiagnosticKind::SkippedSaveFrame => "skipped-save-frame",
            DiagnosticKind::DefaultValue => "default-value",
            DiagnosticKind::PlaceholderRow => "placeholder-row",
            DiagnosticKind::DummyAtom => "dummy-atom",
            DiagnosticKind::PseudoAtom => "pseudo-atom",
            DiagnosticKind::ZeroOccupancy => "zero-occupancy",
            DiagnosticKind::RepeatedAtom => "repeated-atom",
            DiagnosticKind::OverfullSite => "overfull-site",
            DiagnosticKind::AmbiguousSpecialPosition => "ambiguous-special-position",
            DiagnosticKind::IgnoredDisplacement => "ignored-displacement",
            DiagnosticKind::IgnoredOxidationState => "ignored-oxidation-state",
            DiagnosticKind::MultiplicityMismatch => "multiplicity-mismatch",
            DiagnosticKind::CollidingAtoms => "colliding-atoms",
        }
    }
}
