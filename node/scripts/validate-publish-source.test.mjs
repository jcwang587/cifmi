import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { expectedSourceChecks, validatePublishSource } from './validate-publish-source.mjs'

test('recovery install checks match the workflow matrix, including exclusions and additions', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/check-node.yml', import.meta.url), 'utf8').replace(/\r\n/g, '\n')
  const matrix = workflow.split('\n  install-test:\n')[1]?.split('\n    steps:\n')[0]
  assert(matrix, 'Expected the install-test workflow job')
  // Read the simple inline lists and host/version entries used by this workflow.
  // Fail if their syntax changes rather than silently ignoring matrix entries.
  function values(key) {
    const match = matrix.match(new RegExp(`^        ${key}: \\[([^\\]]+)\\]$`, 'm'))
    assert(match, `Expected an inline ${key} matrix`)
    return match[1].split(',').map(value => value.trim().replace(/^"|"$/g, ''))
  }
  function entries(key) {
    const section = matrix.match(new RegExp(`^        ${key}:\\n([\\s\\S]*?)(?=^        \\w|$(?![\\s\\S]))`, 'm'))?.[1]
    assert(section, `Expected matrix ${key}`)
    const pairs = [...section.matchAll(/^          - host: ([^\n]+)\n            node-version: "([^"]+)"$/gm)]
      .map(([, host, version]) => [host, version])
    assert.equal(pairs.length, (section.match(/^          - /gm) ?? []).length,
      `Unsupported ${key} entry syntax`)
    return pairs
  }
  const excluded = entries('exclude')
  const jobs = values('host').flatMap(host => values('node-version').map(version => [host, version]))
    .filter(([host, version]) => !excluded.some(([otherHost, otherVersion]) => host === otherHost && version === otherVersion))
  jobs.push(...entries('include'))
  const expected = jobs.map(([host, version]) => `check / install-test (${host}, ${version})`)
  assert.deepEqual(expectedSourceChecks.filter(name => name.startsWith('check / install-test ')).sort(), expected.sort())
})

function source() {
  return {
    repository: 'jcwang587/cifmi', version: '1.0.1', tagSha: 'release-commit',
    run: { id: 123, path: '.github/workflows/publish-npm.yml',
      repository: { full_name: 'jcwang587/cifmi' }, head_repository: { full_name: 'jcwang587/cifmi' },
      event: 'push', head_branch: 'v1.0.1', head_sha: 'release-commit', status: 'completed' },
    jobs: [...expectedSourceChecks.map(name => ({ name, conclusion: 'success' })),
      { name: 'publish', conclusion: 'failure' }],
    artifact: { name: 'node-packages', expired: false, digest: `sha256:${'a'.repeat(64)}`,
      workflow_run: { id: 123, head_sha: 'release-commit' } },
  }
}

test('accepts a failed publication after all original package checks succeeded', () => {
  assert.doesNotThrow(() => validatePublishSource(source()))
})

for (const [name, mutate] of [
  ['another workflow', data => { data.run.path = '.github/workflows/other.yml' }],
  ['a fork', data => { data.run.head_repository.full_name = 'other/cifmi' }],
  ['another release tag', data => { data.run.head_branch = 'v1.0.0' }],
  ['a moved release tag', data => { data.tagSha = 'different-commit' }],
  ['an incomplete run', data => { data.run.status = 'in_progress' }],
  ['failed package checks', data => { data.jobs[1].conclusion = 'failure' }],
  ['skipped package checks', data => { data.jobs[1].conclusion = 'skipped' }],
  ['missing package checks', data => { data.jobs = [{ name: 'publish', conclusion: 'failure' }] }],
  ['a partial package-check matrix', data => { data.jobs.splice(3, 1) }],
  ['an expired artifact', data => { data.artifact.expired = true }],
  ['an artifact from another run', data => { data.artifact.workflow_run.id = 456 }],
  ['an artifact from another commit', data => { data.artifact.workflow_run.head_sha = 'different-commit' }],
]) {
  test(`rejects ${name}`, () => {
    const data = source()
    mutate(data)
    assert.throws(() => validatePublishSource(data))
  })
}
