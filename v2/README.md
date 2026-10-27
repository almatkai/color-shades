# Color Shades — Native V2

Rust library and concrete compiled-in Tezbar adapter for the original **Color Shades** extension by **qeude** (MIT). Generates and saves 11 recommended shades; history supports View Palette, Delete Entry and Delete All Entries. The base shade at level 500 retains normalized input spelling. All other shades, nearest NTC names and exports follow the locked source libraries, including stateful stable-sort hue ties.

The two commands are `generate-color-shades` (required text argument `color`, passed in `argumentValues`) and `view-color-shades`. The manifest is `extension.json`, schema 2, ID `com.tezbar.native.color-shades`, replacing `raycast.color-shades`. macOS and Windows verification remain **not-run**: tests are headless; live UI, actual system clipboard and real Windows execution have not been tested.

## Host integration

Target API: Tezbar beta `fd300e276356587726566026f072563eb9c34cb1`. This is source for a compiled adapter. Installing a manifest alone cannot add native dispatch.

1. Vendor this directory at **`src-tauri/native-extensions/color-shades`**. Keep Cargo.lock, data, notices and license files with it. Add `tezbar-color-shades = { path = "native-extensions/color-shades" }` to the host's Cargo dependencies. The adapter also uses host dependencies already present in beta: serde, serde_json, chrono and tokio.
2. In `src-tauri/src/native_core/extensions/mod.rs`, declare the module beside `v2_sdk`:

   ```rust
   #[path = "../../../native-extensions/color-shades/src/tezbar_adapter.rs"]
   mod v2_color_shades;
   ```

3. Register `"color-shades"` in `v2_runtime::slug`; add `"color-shades" => super::v2_color_shades::commands()` in `commands`; add its dispatch arm in `handle`:

   ```rust
   "color-shades" => {
       super::v2_color_shades::handle(ctx, command, operation, payload, state).await
   }
   ```

4. Add the `extension.json` object to `src-tauri/src/native_core/extensions/v2_catalog.json`. Copy `assets/color-shades.png` **unchanged** to **`src/renderer/public/extensions/icons/color-shades.png`**, serving `/extensions/icons/color-shades.png`. Rebuild the host. The icon is the original 212,670-byte PNG; SHA-256 `7e51fa91832fe621a9c95ecfa70bce5cbcf7aea75288481bbe76d152506512f8`.

There is no external Rust loader, subprocess dispatch, JSON executable registration, invented HUD API or public RPC route. Clipboard writes go through the actual beta SDK's `clipboard:write` route.

## Behavior and deliberate adaptations

- `normalize` removes the first hash anywhere, doubles exactly three UTF-16 units (excluding JS line terminators), otherwise pads to at least six units. It preserves case and accepts the source's padded and 8-digit forms; no new strict six-digit-only gate is imposed. Invalid colors fail using the original `unknown color` error. Error text containing invalid unpaired UTF-16 surrogates is converted to Unicode replacement characters for Rust/UTF-8 transport; acceptance/rejection remains identical.
- The full 1,566-entry NTC dataset and exact chroma-js 1.4.1 Lab distance are bundled. Equal-distance names retain dataset order. Exports use generated change-case-all slugs for every finite NTC name. No approximate naming service is used.
- Both source hue arrays are stably sorted and mutated between successful generations, including repeated palette changes. The adapter serializes this process-wide generator lane through the save. Failed saves keep the previous hue state and view so retrying has the same result. Saved palettes include a native revision and their original hue ordering, allowing exact recomputation for every copy/base action without trusting event payload colors or text.
- Generate, View Palette and Use as Base Color save before presenting the new palette. Name collisions overwrite the entry. View Palette regenerates from the original level-500 spelling and updates creationDate. History sorts newest first. Equal millisecond dates use deterministic name order, correcting the source comparator's inconsistent `-1` result for equal dates.
- State and action IDs are session-scoped. Header Back uses beta's actual `backActionId` to restore the retained draft or reload prior history. Successful generation, navigation and deletion invalidate prior actions. Dispose/relaunch invalidates the old session. Copy errors leave actions retryable. Clipboard replies must acknowledge success with `ok: true` and no error or failure flag; malformed replies fail. Effects always derive from a recomputed committed palette.
- SDK `update_store` provides atomic read/validate/update/write transactions across contexts. Native storage is only `native-extensions-v2/color-shades/storage.json`, with palettes in the `palettes` key. Delete All empties only that key and preserves unrelated keys. Corrupt storage is rejected without replacing it. History mutations check their saved entry/revision or full snapshot inside the transaction (including identical-color overwrites), rejecting stale cross-session actions. No V1 storage or preference migration/read occurs.
- The source's stray `)` child and duplicate revalidation/generation writes are intentionally omitted.

## Supported beta UI and known gaps

Actual Grid is used: every item has color content, level title and hex subtitle, with primary Copy Shade, Use as Base Color, Copy All Colors as Variables and Copy All Colors as JSON. History uses actual List rows with a tinted CircleFilled base swatch and created-at date accessory. All source actions are available through the beta action panel.

The pinned beta clamps Grid columns to **8**, even though the adapter preserves `columns: 11`. It formats accessory dates as localized calendar dates and ignores accessory/icon tooltips. Source shortcuts are preserved in descriptors (`ctrl+c`, `ctrl+opt+c`, `ctrl+x`, `ctrl+shift+x`), but this beta's action overlay does **not** execute custom shortcut descriptors. Actions remain selectable using its standard action-panel controls. These renderer limitations require host changes for full visual/keyboard parity; this package does not modify the renderer.

## Verification and reproduction

Rust MSRV is **1.85**. Direct dependencies are pinned and Cargo.lock is included. Builds and node_modules are ignored.

```sh
cargo test --locked
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo +1.85.0 test --locked
cargo +1.85.0 clippy --all-targets --locked -- -D warnings
cargo check --target x86_64-pc-windows-gnu --all-targets --locked
cargo clippy --target x86_64-pc-windows-gnu --all-targets --locked -- -D warnings
cargo package --allow-dirty
```

Cargo tests are self-contained, including in the extracted `.crate`: they compile the adapter unchanged beside byte-exact beta common/SDK fixtures, with only a Tauri symbol stub and captured router. They exercise all 11 primary copy/base actions, both exports, draft/back/current palette, malformed payload rejection, unknown commands/operations, storage failures/retries, collisions, date ordering, history view/resave/deletes, concurrent sessions, corruption and stale actions. The unchanged beta SDK/common tests also run. Windows GNU checks compile test code but do not execute Windows binaries.

The checked-in oracle has **4,777 sequential inputs**, including random RGB and short hex, case/hash/padding/error forms, all 361 integer hue boundaries, both directions of tie sequences and every NTC entry. It compares all 11 shade values, exact names, hue ordering before each call, JSON and variable exports. It was generated by executing the original TS generator/export/storage services with locked npm libraries; only Raycast LocalStorage is intercepted. It also checks all 1,566 generated slugs against change-case-all and exercises the original storage collision/delete behavior.

To regenerate the oracle (Node 26; development network access for npm only):

```sh
node tools/prepare-oracle.cjs
npm ci --prefix .work/original --ignore-scripts --no-audit --no-fund
node tools/oracle.cjs
cargo test --locked
```

To validate actual beta parsers, swatch rendering, Back routing, full schema, icon and source/data provenance:

```sh
npm ci --prefix tools --ignore-scripts --no-audit --no-fund
node tools/renderer-check.cjs
node tools/package-check.cjs
```

`renderer-check` runs the original Grid/List/Form parser functions and real React server rendering of their untouched beta files. Only unrelated imported UI helpers are stubbed. It confirms all 11 swatches and the eight-column clamp. `fixtures/wire.json` is an actual adapter result; regenerate it with `COLOR_SHADES_WIRE_PATH="$PWD/fixtures/wire.json" cargo test --lib history_resaves`.

Own V1 smoke tests and typecheck were run with pnpm in a disposable copy; the root package files/lock and all V1 source/assets remain byte-exact. Only the root README gains a link here. See `NOTICE.md` and `vendor/provenance.json` for licensing and exact inputs. Packaging SPDX: **MIT AND Apache-2.0 AND BSD-3-Clause AND CC-BY-2.5**.
