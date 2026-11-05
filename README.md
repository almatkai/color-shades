# Color shades generator

Generate color shades from a base color and manage your generated color shades palettes.

[Native V2 Rust port](v2/README.md)

## Installable native Rust extension for Tezbar

The root Cargo project builds the standalone **Color Shades 1.0.0** executable,
`com.tezbar.native.color-shades` (schema 3, process execution, protocol 1), replacing
`raycast.color-shades`. It reuses the mature [v2 engine and adapter](v2/README.md)
with the published Rust SDK pinned to commit
`aaec571673ab64a85a136cf6bd054ef660c4d93c`. No Tezbar/Tauri crate, beta fixture,
JavaScript, Bun, npm installation or network service is required at runtime.
The original Raycast TypeScript implementation, package metadata and assets remain
in their original locations; its development commands remain in `package.json`.

Both original commands are ported:

| Command | Behavior |
| --- | --- |
| `generate-color-shades` | Required `color` argument; generates and saves 11 shades, exact NTC names, Copy Shade, Use as Base Color, variable and JSON exports. Back returns to an editable color form. |
| `view-color-shades` | Saved palettes, newest first; View Palette regenerates and saves, Delete Entry, Delete All Entries, and Back navigation. |

There are no extension or command preferences in the original package. Only the
narrow `clipboard.write` host capability is requested. Action IDs are bound to
the committed session and palette; payload text/colors cannot replace computed
values. Clipboard failures are reported and actions remain retryable. Sessions,
transactional state, cancellation and shutdown use the SDK `serve()` lifecycle.

The process stores palettes under `<dataDir>/color-shades/storage.json`. On first
initialize, a host-granted `nativeStore` path may import the compiled-in Native V2
store at `native-extensions-v2/color-shades/storage.json`. Entries are validated
by exact recomputation, existing process entries win collisions, and the original
file is never changed. Invalid stores fail explicitly. Raycast/V1 LocalStorage
palettes are **not imported**: their format lacks the saved hue ordering needed
for exact recomputation. The existing V1 store remains untouched.

### Build, package and verify

Run on Apple Silicon macOS with Rust 1.85 or newer and Python 3.9 or newer:

```sh
export CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR="$PWD/target-process"
cargo test --locked -j2
cargo test --manifest-path v2/Cargo.toml --locked -j2
cargo fmt --all -- --check
cargo clippy --all-targets --locked -j2 -- -D warnings
python3 scripts/third-party.py --check
cargo build --release --target aarch64-apple-darwin --locked -j2
python3 scripts/package.py --target aarch64-apple-darwin \
  --binary target-process/aarch64-apple-darwin/release/tezbar-color-shades
python3 scripts/smoke-package.py dist/color-shades-aarch64-apple-darwin.zip \
  --report dist/color-shades-aarch64-apple-darwin.smoke.json
```

`scripts/third-party.py` regenerates the Rust dependency license inventory and
copies each dependency's original license/notice files from the locked Cargo
sources. `--check` verifies the checked-in copies. Packaging executes
`--protocol-info`, verifies the immutable SDK pin and command inventory, and emits
a root-layout ZIP containing `extension.json`, `bin/tezbar-color-shades`,
`resources/`, `licenses/`, `LICENSE`, and `NOTICE`. Its manifest has the exact
SHA256 file inventory. Sibling `.zip.sha256`, `.executable.sha256`, `.files.json`
and `.artifact.json` files carry archive and executable hashes. Smoke verification
checks that inventory and drives the extracted executable through hello,
initialize, both command sessions, a clipboard action, dispose and shutdown using
a recording fake host, without changing the actual desktop clipboard.

For Windows x64, build with `--target x86_64-pc-windows-msvc` on Windows and pass
`target-process/x86_64-pc-windows-msvc/release/tezbar-color-shades.exe` to the same
packaging/smoke scripts. Windows is
declared as a supported target, but its artifact and execution verification are
pending central coordination; no Windows coverage is claimed here.

### Installation and verification limits

This version is an unpublished review candidate. No push, tag, release or trusted
host catalog update has been performed. After independent review, the publisher
must provide the ZIP and a matching trusted release manifest with real target
artifact URL, archive SHA256, executable path and executable SHA256. The ZIP's
`artifacts: {}` deliberately avoids embedding its own hash. The root source
manifest alone is not an installable release reference.

With reviewed artifacts, the host's local import entry point is:

```sh
tezbar --extensions-v3-import /absolute/trusted-release-manifest.json \
  /absolute/color-shades-aarch64-apple-darwin.zip
```

Close and reopen Tezbar after import. Desktop installation, live rendering and
actual clipboard integration have not been exercised. Offline tests cover the
Mac binary and preserved v2 behavior. The inspected host renderer clamps the
requested 11 grid columns to eight, displays localized calendar dates, ignores
swatch/date tooltips and does not execute custom action shortcut descriptors;
all actions remain available through its action panel. No command implementation
is missing, but these renderer differences and absent V1 palette migration remain
explicit parity limits. See [NOTICE](NOTICE) and [licenses](licenses/) for source,
data and dependency attribution.
