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
builds and tests the wheels and the source distribution, and builds the Node
addon for every target, packs the npm packages and installs them. The same
checks run before a release is tagged. Locally,
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
