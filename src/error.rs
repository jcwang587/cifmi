use std::io;
use thiserror::Error as ThisError;

/// Errors raised while reading or writing a structure or validating its
/// parts.
#[derive(ThisError, Debug)]
pub enum Error {
    /// An input/output error occurred while reading or writing a structure
    /// file.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// A structure file could not be parsed, or a structure cannot be
    /// written as one.
    #[error("CIF error: {0}")]
    Cif(String),

    /// A lattice, element, species label or symmetry operation is invalid.
    #[error("{0}")]
    Invalid(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) fn cif(message: impl Into<String>) -> Self {
        Self::Cif(message.into())
    }
}
