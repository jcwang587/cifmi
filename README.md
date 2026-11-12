# cifmi

[![Crates.io](https://img.shields.io/crates/v/cifmi?logo=rust&color=264323)](https://crates.io/crates/cifmi)
[![PyPI](https://img.shields.io/pypi/v/cifmi?logo=pypi&logoColor=white&color=3775A9)](https://pypi.org/project/cifmi/)
[![npm](https://img.shields.io/npm/v/cifmi?logo=npm&color=CB3837)](https://www.npmjs.com/package/cifmi)

cifmi reads CIF 1.1 and 2.0 files into crystal structures, with the cell,
the space group, the sites, the symmetry operations and every atom of the
unit cell, and writes crystal structures as standard CIF 1.1. It is written
in Rust, with Python and Node.js bindings.

## Quick start

### Python

```python
import cifmi

structure = cifmi.read_cif("sample.cif")
for atom in structure.atoms:
    print(structure.sites[atom.site].label, atom.fractional)

cifmi.write_cif("copy.cif", structure)
```

### Rust

```rust
use cifmi::{read_cif, write_cif, CifOptions};

fn main() -> Result<(), cifmi::Error> {
    let structure = read_cif("sample.cif", &CifOptions::default())?;
    for atom in &structure.atoms {
        println!("{} {:?}", structure.sites[atom.site].label, atom.position);
    }
    write_cif("copy.cif", &structure)
}
```

### Node.js

```javascript
const cifmi = require('cifmi')

cifmi.readCifFile('sample.cif').then((structure) => {
  for (const atom of structure.atoms) {
    console.log(structure.sites[atom.site].label, atom.fractional)
  }
  return cifmi.writeCifFile('copy.cif', structure)
})
```

A structure from another crystal model is written the same way: build it
from its cell, operations and sites with `Structure::new` in Rust or
`cifmi.Structure` in Python; in Node.js, `writeCifStr` and `writeCifFile`
take any object of the shape `readCifStr` returns.

Every file is read one way, by crystallographic rules, and a file cifmi
cannot read is an error with the reason. A structure is written as it is,
in its own setting with the standard names. One cifmi reads writes as a CIF
that reads back as the same structure, with repeated labels made unique; the
design's §7 lists the few that read back otherwise or are refused. See the
[design](https://github.com/jcwang587/cifmi/blob/main/docs/design.md) for
the rules and the evidence behind them, and
[CONTRIBUTING.md](https://github.com/jcwang587/cifmi/blob/main/CONTRIBUTING.md)
for development and releases.
