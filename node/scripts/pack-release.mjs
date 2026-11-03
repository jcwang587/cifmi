import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { NapiCli, parseTriple } from '@napi-rs/cli'

const nodeDir = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const [artifactArg, outputArg, localTarget] = process.argv.slice(2)
assert(artifactArg && outputArg, 'Usage: npm run pack:release -- ARTIFACT_DIR OUTPUT_DIR [LOCAL_TARGET]')
assert(process.env.npm_execpath, 'Run this script through npm run pack:release')
const artifactDir = resolve(artifactArg)
const outputDir = resolve(outputArg)
const pkg = JSON.parse(readFileSync(join(nodeDir, 'package.json'), 'utf8'))
const targets = localTarget ? [localTarget] : pkg.napi.targets
assert(targets.every(target => pkg.napi.targets.includes(target)), 'Unknown native target')
const staging = mkdtempSync(join(tmpdir(), 'cifmi-pack-'))
const notices = ['LICENSE', 'THIRD_PARTY_NOTICES.md']
mkdirSync(outputDir, { recursive: true })

try {
  pkg.optionalDependencies = Object.fromEntries(pkg.napi.targets.map(target => [
    `${pkg.name}-${parseTriple(target).platformArchABI}`, pkg.version,
  ]))
  pkg.files = [...new Set([...pkg.files, ...notices])]
  writeFileSync(join(staging, 'package.json'), JSON.stringify(pkg, null, 2) + '\n')
  copyFileSync(join(nodeDir, '..', 'README.md'), join(staging, 'README.md'))
  for (const notice of notices) {
    copyFileSync(join(nodeDir, '..', notice), join(staging, notice))
  }
  await new NapiCli().createNpmDirs({ cwd: staging })

  const packages = []
  let wrapper
  let types
  function pack(directory, target) {
    const result = JSON.parse(execFileSync(process.execPath, [process.env.npm_execpath,
      'pack', '--json', '--ignore-scripts', '--pack-destination', outputDir,
    ], { cwd: directory, encoding: 'utf8' }))[0]
    for (const notice of notices) {
      assert(result.files.some(file => file.path === notice), `Package is missing ${notice}`)
    }
    packages.push({ target, name: result.name, version: result.version, filename: result.filename })
  }

  for (const target of targets) {
    const abi = parseTriple(target).platformArchABI
    const built = join(artifactDir, `bindings-${target}`)
    const generatedWrapper = readFileSync(join(built, 'index.js'), 'utf8')
    const generatedTypes = readFileSync(join(built, 'index.d.ts'), 'utf8')
    if (wrapper !== undefined) {
      assert.equal(generatedWrapper, wrapper, `Generated loader differs for ${target}`)
      assert.equal(generatedTypes, types, `Generated types differ for ${target}`)
    }
    wrapper = generatedWrapper
    types = generatedTypes
    const nativeDir = join(staging, 'npm', abi)
    const binary = `${pkg.napi.binaryName}.${abi}.node`
    copyFileSync(join(built, binary), join(nativeDir, binary))
    const nativeManifestPath = join(nativeDir, 'package.json')
    const nativeManifest = JSON.parse(readFileSync(nativeManifestPath, 'utf8'))
    nativeManifest.files = [...new Set([...nativeManifest.files, ...notices])]
    writeFileSync(nativeManifestPath, JSON.stringify(nativeManifest, null, 2) + '\n')
    for (const notice of notices) {
      copyFileSync(join(staging, notice), join(nativeDir, notice))
    }
    pack(nativeDir, target)
  }

  writeFileSync(join(staging, 'index.js'), wrapper)
  writeFileSync(join(staging, 'index.d.ts'), types)
  pack(staging, null)
  writeFileSync(join(outputDir, 'packages.json'), JSON.stringify(packages, null, 2) + '\n')
  console.log(`Packed ${packages.length} release packages in ${outputDir}`)
} finally {
  rmSync(staging, { recursive: true, force: true })
}
