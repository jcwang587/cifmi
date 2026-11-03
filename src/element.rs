//! Chemical elements and species labels.

use crate::Error;

/// Element symbols indexed by atomic number minus one.
static SYMBOLS: [&str; 118] = [
    "H", "He", "Li", "Be", "B", "C", "N", "O", "F", "Ne", "Na", "Mg", "Al", "Si", "P", "S", "Cl",
    "Ar", "K", "Ca", "Sc", "Ti", "V", "Cr", "Mn", "Fe", "Co", "Ni", "Cu", "Zn", "Ga", "Ge", "As",
    "Se", "Br", "Kr", "Rb", "Sr", "Y", "Zr", "Nb", "Mo", "Tc", "Ru", "Rh", "Pd", "Ag", "Cd", "In",
    "Sn", "Sb", "Te", "I", "Xe", "Cs", "Ba", "La", "Ce", "Pr", "Nd", "Pm", "Sm", "Eu", "Gd", "Tb",
    "Dy", "Ho", "Er", "Tm", "Yb", "Lu", "Hf", "Ta", "W", "Re", "Os", "Ir", "Pt", "Au", "Hg", "Tl",
    "Pb", "Bi", "Po", "At", "Rn", "Fr", "Ra", "Ac", "Th", "Pa", "U", "Np", "Pu", "Am", "Cm", "Bk",
    "Cf", "Es", "Fm", "Md", "No", "Lr", "Rf", "Db", "Sg", "Bh", "Hs", "Mt", "Ds", "Rg", "Cn", "Nh",
    "Fl", "Mc", "Lv", "Ts", "Og",
];

/// A chemical element, or deuterium, which keeps its own symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Element(u8);

const DEUTERIUM: u8 = 0;

impl Element {
    /// Deuterium: Z = 1 under its own symbol.
    pub const D: Element = Element(DEUTERIUM);

    /// Element with atomic number `z` (1..=118).
    pub fn from_z(z: u8) -> Result<Self, Error> {
        if (1..=118).contains(&z) {
            Ok(Element(z))
        } else {
            Err(Error::invalid(format!(
                "atomic number {z} is outside 1..=118"
            )))
        }
    }

    /// Element from an exact, case-sensitive symbol such as `"Fe"` or `"D"`.
    pub fn from_symbol(symbol: &str) -> Result<Self, Error> {
        if symbol == "D" {
            return Ok(Element::D);
        }
        SYMBOLS
            .iter()
            .position(|s| *s == symbol)
            .map(|i| Element(i as u8 + 1))
            .ok_or_else(|| Error::invalid(format!("unknown element symbol {symbol:?}")))
    }

    /// Atomic number (1 for deuterium).
    pub fn z(self) -> u8 {
        if self.0 == DEUTERIUM {
            1
        } else {
            self.0
        }
    }

    /// Element symbol (`"D"` for deuterium).
    pub fn symbol(self) -> &'static str {
        if self.0 == DEUTERIUM {
            "D"
        } else {
            SYMBOLS[self.0 as usize - 1]
        }
    }
}

/// Parse a species label: an element symbol with an optional oxidation state,
/// such as `"Fe"`, `"Fe3+"`, `"O2-"`, `"Na+"` or `"Fe+3"`. Returns the element
/// and the signed charge (0 when absent).
pub fn parse_species(label: &str) -> Result<(Element, i8), Error> {
    let label = label.trim();
    let split = label
        .char_indices()
        .find(|(i, c)| *i > 0 && !c.is_ascii_lowercase())
        .map_or(label.len(), |(i, _)| i);
    let element = Element::from_symbol(&label[..split])?;
    let rest = &label[split..];
    if rest.is_empty() {
        return Ok((element, 0));
    }
    let bad = || Error::invalid(format!("unrecognized species label {label:?}"));
    let (digits, sign) = if let Some(d) = rest.strip_suffix('+') {
        (d, 1)
    } else if let Some(d) = rest.strip_suffix('-') {
        (d, -1)
    } else if let Some(d) = rest.strip_prefix('+') {
        (d, 1)
    } else if let Some(d) = rest.strip_prefix('-') {
        (d, -1)
    } else {
        return Err(bad());
    };
    let magnitude: i8 = if digits.is_empty() {
        1
    } else {
        digits.parse().map_err(|_| bad())?
    };
    Ok((element, sign * magnitude))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip() {
        for z in 1..=118 {
            let e = Element::from_z(z).unwrap();
            assert_eq!(Element::from_symbol(e.symbol()).unwrap(), e);
        }
        assert_eq!(Element::from_symbol("D").unwrap().z(), 1);
        assert!(Element::from_symbol("Xx").is_err());
        assert!(Element::from_symbol("fe").is_err());
    }

    #[test]
    fn species_labels() {
        let fe = Element::from_symbol("Fe").unwrap();
        let o = Element::from_symbol("O").unwrap();
        assert_eq!(parse_species("Fe").unwrap(), (fe, 0));
        assert_eq!(parse_species("Fe3+").unwrap(), (fe, 3));
        assert_eq!(parse_species("Fe+3").unwrap(), (fe, 3));
        assert_eq!(parse_species("O2-").unwrap(), (o, -2));
        assert_eq!(parse_species("Na+").unwrap().1, 1);
        assert!(parse_species("Fe3").is_err());
    }
}
