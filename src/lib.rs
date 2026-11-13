//! Reads CIF files into crystal structures, and writes crystal structures
//! as CIF.
//!
//! A [`Structure`] keeps the sites the file lists (rows on one position
//! merged into a site), the symmetry operations, and every atom of the unit
//! cell those operations generate.
//!
//! The reading is crystallographic: standard CIF syntax; listed symmetry
//! operations checked to form a group, or the settings a space-group name
//! admits settled by the atoms; sites snapped onto their special positions
//! so every orbit has its crystallographic multiplicity. What a file leaves
//! open or gets wrong is reported in [`Structure::diagnostics`];
//! [`CifOptions`] holds the tolerances.
//!
//! [`write_cif`] and [`write_cif_str`] write a structure, read or built with
//! [`Structure::new`], as a CIF 1.1 data block in the core dictionary's
//! names, in the structure's own setting; cifmi reads it back as the same
//! structure.
//!
//! Units: lengths in Å, angles in degrees, displacement parameters U in Å²,
//! occupancies as site fractions.
//!
//! ```
//! let text = "data_Cu
//! _cell_length_a 3.615
//! _cell_length_b 3.615
//! _cell_length_c 3.615
//! _cell_angle_alpha 90
//! _cell_angle_beta 90
//! _cell_angle_gamma 90
//! _symmetry_space_group_name_H-M 'F m -3 m'
//! loop_
//! _atom_site_label
//! _atom_site_fract_x
//! _atom_site_fract_y
//! _atom_site_fract_z
//! Cu1 0 0 0
//! ";
//! let structure = cifmi::read_cif_str(text, &cifmi::CifOptions::default())?;
//! assert_eq!(structure.space_group.hermann_mauguin.as_deref(), Some("F m -3 m"));
//! assert_eq!(structure.sites.len(), 1);
//! assert_eq!(structure.operations.len(), 192);
//! assert_eq!(structure.atoms.len(), 4); // face-centred cubic
//!
//! let written = cifmi::write_cif_str(&structure)?;
//! assert!(written.contains("_space_group_name_H-M_alt   'F m -3 m'"));
//! let again = cifmi::read_cif_str(&written, &cifmi::CifOptions::default())?;
//! assert_eq!(cifmi::write_cif_str(&again)?, written);
//! # Ok::<(), cifmi::Error>(())
//! ```

// Validation rejects NaN by writing `!(x > 0.0)`.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

mod crystallographic;
mod document;
mod element;
mod error;
mod lattice;
mod lexer;
mod options;
#[cfg(feature = "python")]
mod python;
mod sites;
mod spacegroups;
mod structure;
mod symmetry;
mod symop;
mod writer;

pub use element::{parse_species, Element};
pub use error::Error;
pub use lattice::Lattice;
pub use options::CifOptions;
pub use structure::{
    Atom, CellParameters, Diagnostic, DiagnosticKind, Occupant, Site, SpaceGroupNames, Structure,
    SymmetrySource,
};
pub use symop::SymOp;

use std::path::Path;

/// Read the first crystal structure in a CIF file. Bytes that are not
/// UTF-8 are read as U+FFFD.
pub fn read_cif<P: AsRef<Path>>(path: P, options: &CifOptions) -> Result<Structure, Error> {
    let bytes = std::fs::read(path)?;
    read_cif_str(&String::from_utf8_lossy(&bytes), options)
}

/// Read the first crystal structure in CIF text.
pub fn read_cif_str(text: &str, options: &CifOptions) -> Result<Structure, Error> {
    crystallographic::read(text, options)
}

/// Write a structure to a CIF file, as [`write_cif_str`] writes it.
pub fn write_cif<P: AsRef<Path>>(path: P, structure: &Structure) -> Result<(), Error> {
    let text = write_cif_str(structure)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// A structure as CIF 1.1 text: one data block in the core dictionary's
/// current names, in the structure's own setting, axes and origin, with its
/// sites as they are. The space group is named where the table of settings
/// has its operations' setting; every operation is listed; each species of
/// each site is a row of the atom-site loop with its site's multiplicity.
/// A structure cifmi read reads back from the text as the same structure,
/// with repeated labels made unique, and writing that again gives the same
/// text; one built from its parts is read by the reading's rules. A
/// structure that cannot be written as a CIF that cifmi reads is an error
/// with the reason (see [`Structure::new`]).
pub fn write_cif_str(structure: &Structure) -> Result<String, Error> {
    writer::write(structure)
}
