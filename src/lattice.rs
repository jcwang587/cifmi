//! Direct and reciprocal lattices.

use crate::Error;

/// A crystal lattice: rows of `matrix` are the a, b, c vectors in Cartesian Å.
///
/// Fractional coordinates map to Cartesian ones as `x_cart = x_frac · matrix`.
/// [`Lattice::from_parameters`] puts c along z and a in the xz plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
    matrix: [[f64; 3]; 3],
}

impl Lattice {
    /// Lattice from three row vectors. The vectors must be finite and span a
    /// nonzero volume; left-handed sets are accepted.
    pub fn new(matrix: [[f64; 3]; 3]) -> Result<Self, Error> {
        if matrix.iter().flatten().any(|v| !v.is_finite()) {
            return Err(Error::invalid("lattice vectors must be finite"));
        }
        let lattice = Lattice { matrix };
        if !(lattice.determinant().abs() > 1e-12) {
            return Err(Error::invalid("lattice vectors must span a nonzero volume"));
        }
        Ok(lattice)
    }

    /// Lattice from lengths (Å) and angles (degrees), with c along z and a in
    /// the xz plane.
    pub fn from_parameters(
        a: f64,
        b: f64,
        c: f64,
        alpha: f64,
        beta: f64,
        gamma: f64,
    ) -> Result<Self, Error> {
        let params = [a, b, c, alpha, beta, gamma];
        if params.iter().any(|v| !v.is_finite()) || a <= 0.0 || b <= 0.0 || c <= 0.0 {
            return Err(Error::invalid("cell lengths must be positive and finite"));
        }
        if [alpha, beta, gamma]
            .iter()
            .any(|v| *v <= 0.0 || *v >= 180.0)
        {
            return Err(Error::invalid(
                "cell angles must lie strictly between 0 and 180 degrees",
            ));
        }
        let (alpha, beta, gamma) = (alpha.to_radians(), beta.to_radians(), gamma.to_radians());
        let (cos_alpha, cos_beta, cos_gamma) = (alpha.cos(), beta.cos(), gamma.cos());
        let (sin_alpha, sin_beta) = (alpha.sin(), beta.sin());
        let val = ((cos_alpha * cos_beta - cos_gamma) / (sin_alpha * sin_beta)).clamp(-1.0, 1.0);
        let gamma_star = val.acos();
        Lattice::new([
            [a * sin_beta, 0.0, a * cos_beta],
            [
                -b * sin_alpha * gamma_star.cos(),
                b * sin_alpha * gamma_star.sin(),
                b * cos_alpha,
            ],
            [0.0, 0.0, c],
        ])
    }

    /// Row vectors a, b, c in Cartesian Å.
    pub fn matrix(&self) -> &[[f64; 3]; 3] {
        &self.matrix
    }

    fn determinant(&self) -> f64 {
        let m = &self.matrix;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            + m[0][1] * (m[1][2] * m[2][0] - m[1][0] * m[2][2])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// Cell volume in Å³.
    pub fn volume(&self) -> f64 {
        self.determinant().abs()
    }

    /// Lengths |a|, |b|, |c| in Å.
    pub fn lengths(&self) -> [f64; 3] {
        self.matrix
            .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
    }

    /// Angles alpha, beta, gamma in degrees.
    pub fn angles(&self) -> [f64; 3] {
        let m = &self.matrix;
        let lengths = self.lengths();
        let mut out = [0.0; 3];
        for (dim, angle) in out.iter_mut().enumerate() {
            let (j, k) = ((dim + 1) % 3, (dim + 2) % 3);
            let dot = m[j][0] * m[k][0] + m[j][1] * m[k][1] + m[j][2] * m[k][2];
            *angle = (dot / (lengths[j] * lengths[k]))
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
        }
        out
    }

    /// Crystallographic reciprocal lattice (no 2π): rows b1, b2, b3 with
    /// a_i · b_j = δ_ij, in Å⁻¹.
    pub fn reciprocal(&self) -> [[f64; 3]; 3] {
        let m = &self.matrix;
        let cof = [
            [
                m[1][1] * m[2][2] - m[1][2] * m[2][1],
                m[1][2] * m[2][0] - m[1][0] * m[2][2],
                m[1][0] * m[2][1] - m[1][1] * m[2][0],
            ],
            [
                m[0][2] * m[2][1] - m[0][1] * m[2][2],
                m[0][0] * m[2][2] - m[0][2] * m[2][0],
                m[0][1] * m[2][0] - m[0][0] * m[2][1],
            ],
            [
                m[0][1] * m[1][2] - m[0][2] * m[1][1],
                m[0][2] * m[1][0] - m[0][0] * m[1][2],
                m[0][0] * m[1][1] - m[0][1] * m[1][0],
            ],
        ];
        let inv_det = 1.0 / self.determinant();
        cof.map(|row| row.map(|v| v * inv_det))
    }

    /// Interplanar spacing d(hkl) in Å.
    pub fn d_spacing(&self, hkl: [i32; 3]) -> f64 {
        1.0 / norm(reciprocal_vector(&self.reciprocal(), hkl))
    }
}

/// Cartesian reciprocal-lattice vector h·b1 + k·b2 + l·b3.
#[inline]
fn reciprocal_vector(b: &[[f64; 3]; 3], hkl: [i32; 3]) -> [f64; 3] {
    let (h, k, l) = (hkl[0] as f64, hkl[1] as f64, hkl[2] as f64);
    [
        h * b[0][0] + k * b[1][0] + l * b[2][0],
        h * b[0][1] + k * b[1][1] + l * b[2][1],
        h * b[0][2] + k * b[1][2] + l * b[2][2],
    ]
}

#[inline]
fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_cell() {
        let l = Lattice::from_parameters(4.0, 4.0, 4.0, 90.0, 90.0, 90.0).unwrap();
        assert!((l.volume() - 64.0).abs() < 1e-10);
        assert!((l.d_spacing([1, 1, 1]) - 4.0 / 3f64.sqrt()).abs() < 1e-12);
        for a in l.angles() {
            assert!((a - 90.0).abs() < 1e-10);
        }
    }

    #[test]
    fn reciprocal_is_dual_basis() {
        let l = Lattice::from_parameters(5.1, 6.2, 7.3, 81.0, 95.0, 102.0).unwrap();
        let b = l.reciprocal();
        for i in 0..3 {
            for (j, bj) in b.iter().enumerate() {
                let a = l.matrix()[i];
                let dot = a[0] * bj[0] + a[1] * bj[1] + a[2] * bj[2];
                assert!((dot - if i == j { 1.0 } else { 0.0 }).abs() < 1e-12);
            }
        }
        let p = [l.lengths()[0], l.lengths()[1], l.lengths()[2]];
        assert!((p[0] - 5.1).abs() < 1e-12 && (p[2] - 7.3).abs() < 1e-12);
        let ang = l.angles();
        assert!((ang[0] - 81.0).abs() < 1e-10 && (ang[2] - 102.0).abs() < 1e-10);
    }

    #[test]
    fn rejects_degenerate_cells() {
        assert!(Lattice::new([[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 1.0]]).is_err());
        assert!(Lattice::from_parameters(1.0, 1.0, 1.0, 0.0, 90.0, 90.0).is_err());
    }
}
