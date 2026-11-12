//! Positions in the cell: a periodic index for tolerance lookups, orbits,
//! snapping of thirds, and snapping onto special positions.

use crate::lattice::Lattice;
use crate::symop::SymOp;
use rustc_hash::FxHashMap;

/// Images closer than this (fractional, per axis) are one atom; snapped
/// orbits coincide to rounding.
const IMAGE_TOLERANCE: f64 = 1e-6;

/// Periodic grid of fractional positions for tolerance lookups. Cells evenly
/// partition [0, 1) and are at least 2.5 tolerances wide, so every point
/// within the tolerance of a query lies in the query's cell or in the
/// neighbour on the side of the cell half it falls in: 8 cells to search.
pub(crate) struct Grid {
    cells: FxHashMap<[i64; 3], Vec<usize>>,
    pub(crate) points: Vec<[f64; 3]>,
    n: i64,
    tolerance: f64,
}

impl Grid {
    pub(crate) fn new(tolerance: f64) -> Self {
        let n = ((1.0 / (2.5 * tolerance.max(1e-9))).floor() as i64).clamp(1, 1 << 20);
        Grid {
            cells: FxHashMap::default(),
            points: Vec::new(),
            n,
            tolerance,
        }
    }

    /// Cell index and the neighbouring cell to search on each axis.
    fn locate(&self, x: [f64; 3]) -> ([i64; 3], [i64; 3]) {
        let mut cell = [0; 3];
        let mut other = [0; 3];
        for a in 0..3 {
            let t = (x[a] - x[a].floor()) * self.n as f64;
            let c = t.floor();
            cell[a] = (c as i64).rem_euclid(self.n);
            let step = if t - c < 0.5 { -1 } else { 1 };
            other[a] = (cell[a] + step).rem_euclid(self.n);
        }
        (cell, other)
    }

    pub(crate) fn insert(&mut self, x: [f64; 3]) {
        let (key, _) = self.locate(x);
        self.cells.entry(key).or_default().push(self.points.len());
        self.points.push(x);
    }

    fn close(&self, i: usize, x: [f64; 3]) -> bool {
        (0..3).all(|a| {
            let d = self.points[i][a] - x[a];
            (d - d.round_ties_even()).abs() < self.tolerance
        })
    }

    /// The site of `x` among the points, as rows on one position make one
    /// site: the first point within the tolerance of `x`, else of an image of
    /// `x` under `images`, with whether an image matched.
    pub(crate) fn site_of(&self, images: &[&SymOp], x: [f64; 3]) -> Option<(usize, bool)> {
        if let Some(j) = self.first_match(x) {
            return Some((j, false));
        }
        images
            .iter()
            .find_map(|op| self.first_match(op.apply(x)))
            .map(|j| (j, true))
    }

    /// Lowest index within `tolerance` on every axis.
    pub(crate) fn first_match(&self, x: [f64; 3]) -> Option<usize> {
        if self.n < 3 {
            return (0..self.points.len()).find(|&i| self.close(i, x));
        }
        let (cell, other) = self.locate(x);
        let mut best: Option<usize> = None;
        for corner in 0..8 {
            let key = [0, 1, 2].map(|a| {
                if corner >> a & 1 == 0 {
                    cell[a]
                } else {
                    other[a]
                }
            });
            let Some(list) = self.cells.get(&key) else {
                continue;
            };
            for &i in list {
                if best.is_none_or(|b| i < b) && self.close(i, x) {
                    best = Some(i);
                }
            }
        }
        best
    }
}

/// The operations that move positions: all but the identity.
pub(crate) fn moving(operations: &[SymOp]) -> Vec<&SymOp> {
    operations
        .iter()
        .filter(|op| {
            op.rotation != SymOp::IDENTITY.rotation
                || op.translation.iter().any(|t| (t - t.round()).abs() >= 1e-9)
        })
        .collect()
}

/// The atoms of a site at `position`: its images under `operations`,
/// wrapped into the cell, each once, in the order the operations first reach
/// them.
pub(crate) fn orbit(operations: &[SymOp], position: [f64; 3]) -> Vec<[f64; 3]> {
    let mut orbit = Grid::new(IMAGE_TOLERANCE);
    for op in operations {
        let image = wrap(op.apply(position));
        if orbit.first_match(image).is_none() {
            orbit.insert(image);
        }
    }
    orbit.points
}

/// Wrap a fractional position into the cell, a hair below 1 to 0.
fn wrap(x: [f64; 3]) -> [f64; 3] {
    x.map(|v| {
        let w = v - v.floor();
        if w >= 1.0 - 1e-12 {
            0.0
        } else {
            w
        }
    })
}

const TWO_THIRDS: f64 = 2.0 / 3.0;
const ONE_THIRD: f64 = 1.0 / 3.0;

/// A value within `tolerance` (relative) of 1/3 or 2/3 becomes exactly
/// that: a CIF writes 0.3333 for a third.
pub(crate) fn snap_thirds(v: f64, tolerance: f64) -> f64 {
    for ideal in [ONE_THIRD, TWO_THIRDS] {
        if (v / ideal - 1.0).abs() <= tolerance {
            return ideal;
        }
    }
    v
}

/// Move `x` to the mean of its images under the operations that map it
/// within `tolerance` Å of itself (its site symmetry), so those operations
/// fix it exactly and its orbit has the crystallographic multiplicity.
pub(crate) fn snap_to_special_position(
    lattice: &Lattice,
    ops: &[SymOp],
    x: [f64; 3],
    tolerance: f64,
) -> [f64; 3] {
    let m = lattice.matrix();
    mean_of_close_images(ops, x, |d| {
        let cart = [0, 1, 2].map(|j| d[0] * m[0][j] + d[1] * m[1][j] + d[2] * m[2][j]);
        (cart[0] * cart[0] + cart[1] * cart[1] + cart[2] * cart[2]).sqrt() < tolerance
    })
}

/// [`snap_to_special_position`] with images within `tolerance` in
/// fractional coordinates on every axis: what rounding leaves of a special
/// position.
pub(crate) fn snap_within(ops: &[SymOp], x: [f64; 3], tolerance: f64) -> [f64; 3] {
    mean_of_close_images(ops, x, |d| d.iter().all(|v| v.abs() < tolerance))
}

/// The mean of the images of `x` under the subgroup generated by the
/// operations whose image of `x`, across cell boundaries, is `close`.
///
/// Averaging over a whole subgroup puts the mean on a point each of its
/// operations fixes. The close operations alone need not form one: when
/// rounding sets two images of a site either side of the tolerance (two
/// mirrors through it, 1e-4 away, one taken and one not), their mean is fixed
/// by neither, and the site's orbit would double.
fn mean_of_close_images(ops: &[SymOp], x: [f64; 3], close: impl Fn([f64; 3]) -> bool) -> [f64; 3] {
    // Each operation with its translation taken to the image nearest `x`.
    let nearest = |op: SymOp| {
        let image = op.apply(x);
        SymOp {
            rotation: op.rotation,
            translation: [0, 1, 2].map(|a| op.translation[a] - (image[a] - x[a]).round()),
        }
    };
    let same = |a: &SymOp, b: &SymOp| {
        a.rotation == b.rotation
            && (0..3).all(|i| (a.translation[i] - b.translation[i]).abs() < 1e-6)
    };
    let mut group: Vec<SymOp> = Vec::new();
    for op in ops {
        let op = nearest(*op);
        let image = op.apply(x);
        if close([0, 1, 2].map(|a| image[a] - x[a])) && !group.iter().any(|g| same(g, &op)) {
            group.push(op);
        }
    }
    if group.is_empty() {
        return x;
    }
    // Close under composition. A point's stabilizer is at most 48 strong.
    let mut size = 0;
    while size != group.len() {
        size = group.len();
        for i in 0..size {
            for j in 0..size {
                let (a, b) = (group[i], group[j]);
                let rotation = [0, 1, 2].map(|r| {
                    [0, 1, 2].map(|c| (0..3).map(|k| a.rotation[r][k] * b.rotation[k][c]).sum())
                });
                let translation = a.apply(b.translation);
                let product = nearest(SymOp {
                    rotation,
                    translation,
                });
                if !group.iter().any(|g| same(g, &product)) {
                    group.push(product);
                }
            }
        }
    }
    let mut sum = [0.0; 3];
    for op in &group {
        let image = op.apply(x);
        for a in 0..3 {
            sum[a] += image[a];
        }
    }
    sum.map(|v| v / group.len() as f64)
}
