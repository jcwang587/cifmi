//! Space-group operations.

use crate::spacegroups::OPS;
use crate::Error;

/// A space-group operation x' = R x + t on fractional column vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymOp {
    pub rotation: [[i32; 3]; 3],
    pub translation: [f64; 3],
}

impl SymOp {
    pub const IDENTITY: SymOp = SymOp {
        rotation: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        translation: [0.0; 3],
    };

    /// Parse a CIF-style operation such as `"-x+1/2, y, -z"` or `"x-y,x,z+1/6"`.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let bad = |why: &str| Error::invalid(format!("symmetry operation {text:?}: {why}"));
        let parts: Vec<&str> = text.trim().trim_matches(['\'', '"']).split(',').collect();
        if parts.len() != 3 {
            return Err(bad("expected three comma-separated components"));
        }
        let mut op = SymOp {
            rotation: [[0; 3]; 3],
            translation: [0.0; 3],
        };
        for (row, part) in parts.iter().enumerate() {
            let expr: String = part.chars().filter(|c| !c.is_whitespace()).collect();
            let expr = expr.to_ascii_lowercase();
            if expr.is_empty() {
                return Err(bad("empty component"));
            }
            let bytes = expr.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let mut sign = 1.0;
                if bytes[i] == b'+' || bytes[i] == b'-' {
                    if bytes[i] == b'-' {
                        sign = -1.0;
                    }
                    i += 1;
                }
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'/')
                {
                    i += 1;
                }
                let number = &expr[start..i];
                let coefficient = if number.is_empty() {
                    None
                } else {
                    Some(parse_number(number).ok_or_else(|| bad("bad number"))?)
                };
                if i < bytes.len() && bytes[i] == b'*' {
                    i += 1;
                }
                if i < bytes.len() && matches!(bytes[i], b'x' | b'y' | b'z') {
                    let col = (bytes[i] - b'x') as usize;
                    let c = sign * coefficient.unwrap_or(1.0);
                    if c.fract() != 0.0 {
                        return Err(bad("non-integer rotation coefficient"));
                    }
                    op.rotation[row][col] += c as i32;
                    i += 1;
                    // A trailing constant written as "x/2" is not crystallographic.
                } else {
                    let c = coefficient.ok_or_else(|| bad("dangling sign"))?;
                    op.translation[row] += sign * c;
                }
            }
        }
        let det = determinant(&op.rotation);
        if det != 1 && det != -1 {
            return Err(bad("rotation part is not unimodular"));
        }
        Ok(op)
    }

    /// Image of a fractional position.
    pub fn apply(&self, x: [f64; 3]) -> [f64; 3] {
        let r = &self.rotation;
        let mut out = self.translation;
        for (i, o) in out.iter_mut().enumerate() {
            *o += r[i][0] as f64 * x[0] + r[i][1] as f64 * x[1] + r[i][2] as f64 * x[2];
        }
        out
    }

    /// Image of Miller indices under the reciprocal action h' = h R.
    #[inline]
    pub fn apply_hkl(&self, h: [i32; 3]) -> [i32; 3] {
        let r = &self.rotation;
        [
            h[0] * r[0][0] + h[1] * r[1][0] + h[2] * r[2][0],
            h[0] * r[0][1] + h[1] * r[1][1] + h[2] * r[2][1],
            h[0] * r[0][2] + h[1] * r[1][2] + h[2] * r[2][2],
        ]
    }
}

/// Operations of a generated table: indices into `OPS`, whose translations
/// are in 1/24ths.
pub(crate) fn table_operations(indices: &[u16]) -> Vec<SymOp> {
    indices
        .iter()
        .map(|&i| {
            let (r, t) = OPS[i as usize];
            SymOp {
                rotation: [
                    [r[0] as i32, r[1] as i32, r[2] as i32],
                    [r[3] as i32, r[4] as i32, r[5] as i32],
                    [r[6] as i32, r[7] as i32, r[8] as i32],
                ],
                translation: t.map(|v| v as f64 / 24.0),
            }
        })
        .collect()
}

fn parse_number(text: &str) -> Option<f64> {
    match text.split_once('/') {
        Some((n, d)) => {
            let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
            (d != 0.0).then_some(n / d)
        }
        None => text.parse().ok(),
    }
}

fn determinant(r: &[[i32; 3]; 3]) -> i32 {
    r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
        - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_operations() {
        let op = SymOp::parse("-x+1/2, y, -z").unwrap();
        assert_eq!(op.rotation, [[-1, 0, 0], [0, 1, 0], [0, 0, -1]]);
        assert_eq!(op.translation, [0.5, 0.0, 0.0]);
        let op = SymOp::parse("x-y,x,z+1/6").unwrap();
        assert_eq!(op.rotation, [[1, -1, 0], [1, 0, 0], [0, 0, 1]]);
        assert!((op.translation[2] - 1.0 / 6.0).abs() < 1e-15);
        let op = SymOp::parse("'1/2+X,0.5-Y,Z'").unwrap();
        assert_eq!(op.rotation, [[1, 0, 0], [0, -1, 0], [0, 0, 1]]);
        assert_eq!(op.translation, [0.5, 0.5, 0.0]);
        assert!(SymOp::parse("x,y").is_err());
        assert!(SymOp::parse("x,x,z").is_err());
    }

    #[test]
    fn hkl_action_preserves_structure_factor_magnitude() {
        // h' = h R: for the 4-fold x,y,z -> -y,x,z the (1,2,0) maps to (2,-1,0).
        let op = SymOp::parse("-y,x,z").unwrap();
        assert_eq!(op.apply_hkl([1, 2, 0]), [2, -1, 0]);
    }
}
