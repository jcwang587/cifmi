# Tests

Run from the repository root:

```sh
cargo test
```

Unit tests sit next to the code they test. [`reading.rs`](reading.rs)
reads the fixtures below, and small constructed files, checking each file
against its declared formula where it gives one. [`writing.rs`](writing.rs)
writes them, reads them back as the same structures, and writes them again
as the same text.

## Fixtures

`data/cif/` holds Crystallography Open Database entries, which COD places in
the public domain, one per case that needs a rule of its own:

| File | Case |
| --- | --- |
| `cod__1000126.cif` | Anisotropic U only: B comes from U_eq |
| `cod__1010642.cif` | A dummy atom (`_atom_site_calc_flag dum`) at occupancy 2 |
| `cod__1100217.cif` | A listing already expanded (rows such as `Br1F*`), read once |
| `cod__1526224.cif` | Se1, listed off the three-fold axis and read onto it, 1e-4 from S1 there: two sites, in every reading of the written text too |
| `cod__1528562.cif` | F1 on (x, ½ − x, ¼) of `R -3 c :R`, listed at (−0.1667, 0.6667): the third stays as the site moves onto its special position |
| `cod__1539664.cif` | Cr2, listed 0.004 Å off (x, 2x, ¼) and read onto it, lands on Fe2 there: one site |
| `cod__2003493.cif` | The Hall symbol names origin choice 1 of F d -3 m, the coordinates are origin choice 2's; the symbol is the old `F d 3 m` |
| `cod__2100690.cif` | A modulated structure (superspace group `X4bm`) |
| `cod__4001733.cif` | Sc on (x, x + ½, 0) to four decimals: its two diagonal-mirror images straddle the merge tolerance |
| `cod__5910103.cif` | Willemite named only `R -3`, in rhombohedral axes |
| `cod__7046814.cif` | SHELX half-occupied H pairs at mirror images, adding up to full atoms |
| `cod__9005920.cif` | AMCSD entry whose site occupancies add up to 1.001 |

## Written texts

`data/written/` holds each fixture cifmi reads, as cifmi writes it.
[`writing.rs`](writing.rs), [`test_python.py`](test_python.py) and
[`node/test.cjs`](../node/test.cjs) compare their writing with these texts
byte for byte, so that Rust, Python and Node.js write one text. After a
deliberate change to the writing, write them anew and review the diff:

```sh
CIFMI_UPDATE_WRITTEN=1 cargo test --test writing
```
