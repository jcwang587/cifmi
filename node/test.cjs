const assert = require('node:assert/strict')
const path = require('node:path')
// CIFMI_BINDING can point to an installed package entry point or a compiled .node file.
const cifmi = require(process.env.CIFMI_BINDING || './index.js')

const fixtures = path.resolve(__dirname, '../tests/data/cif')

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

;(async () => {
  // A file named in origin choice 1 with origin choice 2's coordinates:
  // the atoms settle it, and the reading says so.
  const y2ru2o7 = await cifmi.readCifFile(path.join(fixtures, 'cod__2003493.cif'))
  const yttrium = y2ru2o7.atoms.filter((atom) => y2ru2o7.sites[atom.site].species[0].element === 'Y')
  assert.equal(yttrium.length, 16)
  assert.deepEqual(y2ru2o7.diagnostics.map((d) => d.kind), ['setting-chosen'])
  assert.match(y2ru2o7.diagnostics[0].message, /F d -3 m:2 is read/)
  await assert.rejects(cifmi.readCifFile(path.join(fixtures, 'missing.cif')), /IO error/)
  console.log('node: cifmi reads, reports and errs as expected')
})().catch((err) => {
  console.error(err)
  process.exit(1)
})
