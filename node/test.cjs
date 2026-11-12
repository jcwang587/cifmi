const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
// CIFMI_BINDING can point to an installed package entry point or a compiled .node file.
const cifmi = require(process.env.CIFMI_BINDING || './index.js')

const fixtures = path.resolve(__dirname, '../tests/data/cif')
// Each fixture cifmi reads, as it writes it: the Rust and Python tests compare
// their writing with the same bytes.
const written = path.resolve(__dirname, '../tests/data/written')

const cscl = `data_CsCl
_cell_length_a 4.123
_cell_length_b 4.123
_cell_length_c 4.123
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
_space_group_name_H-M_alt 'P m -3 m'
loop_
_atom_site_label
_atom_site_type_symbol
_atom_site_fract_x
_atom_site_fract_y
_atom_site_fract_z
_atom_site_U_iso_or_equiv
Cs1 Cs 0 0 0 0.02
Cl1 Cl 0.5 0.5 0.5 0.02
`

// The reading, as plain data.
const s = cifmi.readCifStr(cscl)
assert.equal(s.name, 'CsCl')
assert.deepEqual(s.cell, { a: 4.123, b: 4.123, c: 4.123, alpha: 90, beta: 90, gamma: 90 })
assert.equal(s.vectors.length, 3)
assert.ok(Math.abs(s.vectors[2][2] - 4.123) < 1e-12)
// What the file leaves out is absent.
assert.deepEqual(s.spaceGroup, { symbol: 'P m -3 m' })
assert.deepEqual(s.symmetrySource, { kind: 'table', tag: '_space_group_name_h-m_alt', value: 'P m -3 m' })
assert.equal(s.operations.length, 48)
assert.deepEqual(s.operations[0], { rotation: [1, 0, 0, 0, 1, 0, 0, 0, 1], translation: [0, 0, 0] })
assert.deepEqual(s.sites.map((site) => site.label), ['Cs1', 'Cl1'])
assert.deepEqual(s.sites[1].fractional, [0.5, 0.5, 0.5])
const [cs] = s.sites[0].species
assert.equal(cs.element, 'Cs')
assert.equal(cs.occupancy, 1)
assert.ok(Math.abs(cs.bIso - 8 * Math.PI ** 2 * 0.02) < 1e-12)
assert.equal(s.atoms.length, 2)
assert.deepEqual(s.diagnostics, [])
assert.deepEqual(JSON.parse(JSON.stringify(s)), s, 'survives JSON')
assert.deepEqual(structuredClone(s), s, 'survives structured cloning')

// Errors.
assert.throws(() => cifmi.readCifStr('data_x\n_cell_length_a 1\n'), /CIF error/)
assert.throws(() => cifmi.readCifStr(cscl, { block: 'other' }), /no data block named "other"/)

// The writing: every fixture byte for byte as the other runtimes write it,
// from the reading or from a plain object of its shape, and read back.
for (const file of fs.readdirSync(written).sort()) {
  const expected = fs.readFileSync(path.join(written, file), 'latin1')
  const structure = cifmi.readCifStr(fs.readFileSync(path.join(fixtures, file), 'utf8'))
  assert.equal(cifmi.writeCifStr(structure), expected, file)
  const { name, cell, operations, sites } = JSON.parse(JSON.stringify(structure))
  assert.equal(cifmi.writeCifStr({ name, cell, operations, sites }), expected, file)
  assert.equal(cifmi.writeCifStr(cifmi.readCifStr(expected)), expected, file)
}
const csclText = cifmi.writeCifStr(s)
assert.match(csclText, /^_space_group_name_H-M_alt {3}'P m -3 m'$/m)
assert.match(csclText, /^ {2}Cs1 {2}Cs {2}1 {2}0\.00000000 {2}0\.00000000 {2}0\.00000000 {2}1\.00000000 {2}1\.57913670$/m)
// What cannot be written is an error with the reason.
const changed = (change) => {
  const copy = structuredClone(s)
  change(copy)
  return () => cifmi.writeCifStr(copy)
}
assert.throws(changed((c) => { c.sites[0].species[0].element = 'Xx' }), /unknown element symbol "Xx"/)
assert.throws(changed((c) => { c.sites[0].species[0].occupancy = 1.5 }), /Cs1: occupancy 1.5 is not above 0 and at most 1/)
assert.throws(changed((c) => { c.operations[1].rotation[0] = 0.5 }), /rotation .* is not 9 integers/)
assert.throws(changed((c) => { c.sites[1].fractional.pop() }), /site Cl1's fractional coordinates has 2 numbers, not 3/)
assert.throws(changed((c) => { c.name = 'Cs Cl' }), /cannot be a data block code/)

;(async () => {
  // A file named in origin choice 1 with origin choice 2's coordinates:
  // the atoms settle it, and the reading says so.
  const y2ru2o7 = await cifmi.readCifFile(path.join(fixtures, 'cod__2003493.cif'))
  const yttrium = y2ru2o7.atoms.filter((atom) => y2ru2o7.sites[atom.site].species[0].element === 'Y')
  assert.equal(yttrium.length, 16)
  assert.deepEqual(y2ru2o7.diagnostics.map((d) => d.kind), ['setting-chosen'])
  assert.match(y2ru2o7.diagnostics[0].message, /F d -3 m:2 is read/)
  await assert.rejects(cifmi.readCifFile(path.join(fixtures, 'missing.cif')), /IO error/)
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'cifmi-'))
  try {
    const target = path.join(directory, 'written.cif')
    assert.equal(await cifmi.writeCifFile(target, y2ru2o7), undefined)
    assert.equal(fs.readFileSync(target, 'latin1'), cifmi.writeCifStr(y2ru2o7))
    assert.equal((await cifmi.readCifFile(target)).atoms.length, 88)
    await assert.rejects(cifmi.writeCifFile(path.join(directory, 'missing', 'x.cif'), y2ru2o7), /IO error/)
    // What cannot be written rejects the promise, as a file that cannot be read does.
    const unnamed = { ...y2ru2o7, name: '' }
    await assert.rejects(cifmi.writeCifFile(target, unnamed), /cannot be a data block code: it is empty/)
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
  console.log('node: cifmi reads, writes, reports and errs as expected')
})().catch((err) => {
  console.error(err)
  process.exit(1)
})
