//! Explicit import of the compiled-in Native V2 store, only when the host grants
//! the manifest-declared `nativeStore` migration path. Never searches app data and
//! never modifies the legacy file. V1/Raycast LocalStorage palettes are not
//! imported: they lack the hue-ordering state needed for exact recomputation.
use crate::native_core::extensions::{color_shades, v2_sdk::STORE};
use serde_json::{json, Value};
use tezbar_extension_sdk::{read_store, update_store, CoreResult, ExtensionContext};

pub const MIGRATION_KEY: &str = "nativeStore";
const MARKER: &str = "_nativeStoreImported";

pub async fn initialize(ctx: &ExtensionContext) -> CoreResult {
    let current = read_store(ctx, STORE).await?;
    if current[MARKER] == true {
        return Ok(current);
    }
    let Some(path) = ctx.migration_paths.get(MIGRATION_KEY) else {
        // Nothing granted: nothing to import. Do not write a marker so a later
        // grant (e.g. after reinstall from a host that has the file) still imports.
        return Ok(current);
    };
    let legacy = match tokio::fs::read(path).await {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Invalid Color Shades migration store: {e}"))?;
            if !value.is_object() {
                return Err("Color Shades migration store must be an object".into());
            }
            value
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("Color Shades migration store: {e}")),
    };
    update_store(ctx, STORE, move |store| {
        color_shades::import_palettes(store, &legacy)
            .map_err(|e| format!("Color Shades migration rejected: {e}"))?;
        store[MARKER] = json!(true);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tezbar_extension_sdk::{ExtensionPaths, TestContext};

    fn fixture(dir: &std::path::Path) -> TestContext {
        TestContext::new(
            ExtensionPaths {
                package_dir: dir.join("package"),
                data_dir: dir.join("private"),
                temp_dir: dir.join("tmp"),
            },
            &["clipboard.write"],
        )
    }

    #[tokio::test]
    async fn absent_grant_imports_nothing_and_writes_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        let f = fixture(dir.path());
        assert_eq!(initialize(&f.context).await.unwrap(), json!({}));
        assert!(!dir
            .path()
            .join("private/color-shades/storage.json")
            .exists());
        assert!(f.calls().is_empty());
    }

    #[tokio::test]
    async fn missing_granted_file_marks_import_done() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path());
        f.context
            .migration_paths
            .insert(MIGRATION_KEY.into(), dir.path().join("absent.json"));
        let store = initialize(&f.context).await.unwrap();
        assert_eq!(store[MARKER], true);
        assert_eq!(store["palettes"], json!({}));
    }

    #[tokio::test]
    async fn malformed_or_corrupt_legacy_is_rejected_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        for (i, bytes) in [
            &b"{broken"[..],
            &b"[]"[..],
            &br##"{"palettes":{"X":{"revision":1,"base":"#000","before":{"cmy":[180,300,60],"rgb":[360,240,120,0]},"palette":{"creationDate":"2026-01-01T00:00:00.000Z","name":"X","colors":{}}}}}"##[..],
        ]
        .into_iter()
        .enumerate()
        {
            let legacy = dir.path().join(format!("bad{i}.json"));
            std::fs::write(&legacy, bytes).unwrap();
            let mut f = fixture(&dir.path().join(i.to_string()));
            f.context
                .migration_paths
                .insert(MIGRATION_KEY.into(), legacy.clone());
            assert!(initialize(&f.context).await.is_err());
            assert_eq!(read_store(&f.context, STORE).await.unwrap(), json!({}));
            assert_eq!(std::fs::read(&legacy).unwrap(), bytes);
        }
    }
    fn committed(base: &str, date: &str) -> (String, Value) {
        let before = tezbar_color_shades::Generator::default();
        let palette = before.clone().generate(base, date).unwrap();
        let name = palette.name.clone();
        (
            name,
            json!({"revision":7,"base":base,"before":before,"palette":palette}),
        )
    }

    #[tokio::test]
    async fn imports_validated_palettes_existing_wins_and_legacy_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let (a, a_value) = committed("#82123d", "2026-01-01T00:00:00.000Z");
        let (b, b_value) = committed("#00ff00", "2026-01-02T00:00:00.000Z");
        let (_, newer_a) = committed("#82123d", "2026-02-01T00:00:00.000Z");
        let legacy = dir.path().join("legacy.json");
        let bytes = serde_json::to_vec(
            &json!({"palettes":{a.clone():a_value,b.clone():b_value.clone()},"other":1}),
        )
        .unwrap();
        std::fs::write(&legacy, &bytes).unwrap();
        let mut f = fixture(dir.path());
        f.context
            .migration_paths
            .insert(MIGRATION_KEY.into(), legacy.clone());
        update_store(&f.context, STORE, {
            let (a, newer_a) = (a.clone(), newer_a.clone());
            move |s| {
                s["palettes"] = json!({ a: newer_a });
                s["future"] = json!({"keep":true});
                Ok(())
            }
        })
        .await
        .unwrap();
        let store = initialize(&f.context).await.unwrap();
        assert_eq!(store["palettes"][&a], newer_a);
        assert_eq!(store["palettes"][&b], b_value);
        assert_eq!(store["future"]["keep"], true);
        assert!(store.get("other").is_none(), "only palettes are imported");
        assert_eq!(store[MARKER], true);
        assert_eq!(std::fs::read(&legacy).unwrap(), bytes);
        // Idempotent: a second start (or a later legacy change) does not re-import.
        std::fs::write(&legacy, b"{broken").unwrap();
        assert_eq!(initialize(&f.context).await.unwrap(), store);
        assert!(f.calls().is_empty());
    }
}
