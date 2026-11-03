//! How a CIF is read: the block to read and the reading's tolerances.

/// Options for reading a structure from CIF data.
#[derive(Clone, Debug)]
pub struct CifOptions {
    /// Data block name (after `data_`); the first block that yields a
    /// structure when `None`.
    pub block: Option<String>,
    /// Rows closer than this (fractional, per axis), directly or through
    /// symmetry, are on one site.
    pub site_tolerance: f64,
    /// Coordinates within this relative distance of 1/3 or 2/3 snap to them:
    /// a CIF writes 0.3333 for a third.
    pub frac_tolerance: f64,
    /// Sites whose total occupancy exceeds this are an error; totals between
    /// 1 and this are scaled to a full site and reported.
    pub occupancy_tolerance: f64,
    /// Operations that move a site by less than this many Å form its site
    /// symmetry; the site moves to the mean of those images, so its orbit is
    /// expanded exactly.
    pub special_position_tolerance: f64,
}

impl CifOptions {
    /// The default tolerances, reading the first block that yields a
    /// structure.
    pub fn new() -> Self {
        CifOptions {
            block: None,
            site_tolerance: 1e-4,
            frac_tolerance: 1e-4,
            occupancy_tolerance: 1.1,
            special_position_tolerance: 0.05,
        }
    }
}

impl Default for CifOptions {
    fn default() -> Self {
        Self::new()
    }
}
