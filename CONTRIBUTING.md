# Contributing to cifmi

Run development commands from the repository root.

## Tests

### Rust

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features python -- -D warnings
```

The `python` feature turns on the PyO3 bindings, which only maturin can
link, so use it for `clippy` and `check` but not for `cargo test`.

The tests compare what cifmi writes with the texts in `tests/data/written`;
[tests/README.md](tests/README.md) says how to write them anew after a
change to the writing.

### Python

Build the extension into an activated environment, then run the suite:

```sh
python -m pip install -e ".[test]"
python -m pytest tests
```

### Node.js

```sh
npm --prefix node install
npm --prefix node run build
npm --prefix node test
```

To test a separately built binding, set `CIFMI_BINDING` to an installed
package entry point or a compiled `.node` file before the test command.

### Everything a release checks

The `Check packages` workflow packages the crate and tests the packaged copy,
builds and tests one abi3 wheel per platform and the source distribution, and
builds the Node addon for every target, packs the npm packages and installs
them. The wheels on PyPI and the binaries on npm cover the same five
platforms: Windows x64 and Arm64, macOS Arm64, and Linux x64 and Arm64 with
glibc. Elsewhere pip builds the source distribution, which needs Rust. The
same checks run before a release is tagged. Locally,
`python tests/check_release_version.py` checks that every manifest and lock
file carries the version in `Cargo.toml`, and `pre-commit run --all-files`
runs the formatters.

## The space-group table

`src/spacegroups.rs` is generated from gemmi's table of settings. Regenerate
it with an interpreter that has gemmi and numpy, then review the diff:

```sh
python -m pip install numpy gemmi
python tools/generate_spacegroups.py
```

## macOS builds

Every macOS link goes through `tools/macos-link`, set in `.cargo/config.toml`:
the Xcode 27 linker can leave a library's symbol string pool where the macOS 27
loader refuses it, and the wrapper realigns the output. Aligned outputs are
left as they are, so other Xcode versions are unaffected. Release profiles are
not stripped for the same reason.

## Releases

The version lives in `Cargo.toml`. Python derives it through maturin, and the
Node manifests are synced from it.

1. Set `[package].version` in `Cargo.toml` and add the version's section to
   `CHANGELOG.md`.
2. Sync the Node manifests and refresh both lock files:

   ```sh
   npm --prefix node run sync:version
   cargo check
   cargo check --manifest-path node/Cargo.toml
   ```

3. Regenerate the Node loader, which embeds the version:

   ```sh
   npm --prefix node run build
   ```

4. Check the agreement and open a pull request:

   ```sh
   python tests/check_release_version.py --version X.Y.Z
   ```

5. After the merge, run the `release` workflow with the version
   (`gh workflow run release.yml -f version=X.Y.Z`). It checks every package
   again, tags `vX.Y.Z`, and the tag publishes the crate to crates.io, the
   wheels to PyPI and the Node packages to npm.

### Registry setup and the first release

Before running the release workflow:

- Add `CARGO_REGISTRY_TOKEN` and `RELEASE_TRIGGER_TOKEN` as **repository
  secrets** in GitHub Actions settings. The crates.io token must allow the
  first publication of `cifmi`; the release token must be able to push tags
  and trigger workflows. These workflows do not use an environment, so
  secrets stored only in the `pypi` environment are unavailable to them.
- Create the GitHub environment `pypi`. On PyPI, add a pending trusted
  publisher for project `cifmi`, owner `jcwang587`, repository `cifmi`,
  workflow `publish-pypi.yml`, and environment `pypi`.
- Wait for the complete `Check packages` run on the release commit to pass.

On npm, users install `cifmi` alone. Its binaries are five packages under
the `@cifmi` scope, `@cifmi/cifmi-win32-x64-msvc`,
`@cifmi/cifmi-win32-arm64-msvc`, `@cifmi/cifmi-darwin-arm64`,
`@cifmi/cifmi-linux-x64-gnu` and `@cifmi/cifmi-linux-arm64-gnu`, named by
`napi.packageName` in `node/package.json`. `cifmi` lists them as optional
dependencies, so npm installs the one for the user's system, and its loader
requires that one.

npm trusted publishing requires the six packages to exist first. The
initial tag-triggered `publish npm` run builds and tests all six tarballs,
then its publish step refuses to continue while registry entries are
missing. Bootstrap from that run's `node-packages` artifact:

1. On npm, create the organization `cifmi`, which owns the `@cifmi` scope,
   with the account that publishes.
2. Download the artifact from the **tag-triggered `publish npm` run** whose
   package checks all passed. Keep its run ID and original tarballs.
3. Log in to npm locally and publish the five platform tarballs in the order
   listed in `packages.json`, then `cifmi` last. For each tarball, run
   `npm publish PATH_TO_TARBALL --ignore-scripts --access public`: a scoped
   package is private unless it is published with public access.
4. For every package name in `packages.json`, the scoped ones in full,
   configure its trusted publisher:

   ```sh
   npm trust github PACKAGE_NAME --repo jcwang587/cifmi --file publish-npm.yml --allow-publish
   ```

5. Verify the original artifacts through the recovery workflow, from the
   release's own tag (`v0.2.0` for the first npm release):

   ```sh
   gh workflow run publish-npm.yml --ref vX.Y.Z -f publishToNPM=yes -f sourceRunId=RUN_ID
   ```

Recovery validates the source run and downloads its original tested
tarballs. It skips a published version only when its integrity and npm tag
match exactly. Do not substitute tarballs from an earlier `Check packages`
run or rebuild them locally: a different binary or archive can have a
different integrity even at the same version. Use this same recovery path
after a partial npm publication. Subsequent releases publish automatically
through the configured trusted publishers.

### Recovering a partial Python publication

Keep the original tag and checked artifacts. The PyPI workflow accepts a
`sourceRunId` from a completed, tag-triggered `publish pypi` run whose package
checks passed. Dispatch recovery from the release's own tag, whose workflow
expects the checks and artifacts that release ran:

```sh
gh workflow run publish-pypi.yml --ref vX.Y.Z -f publishToPyPI=yes -f sourceRunId=RUN_ID
```

Recovery verifies the original repository, tag, commit, checks and artifact
metadata. It downloads each artifact into its own directory and requires one
intact wheel per platform and the source distribution. It excludes an
existing PyPI file only when its SHA256 matches its original checked
artifact, and uploads only the missing files.
