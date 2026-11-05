//! Import only explicit host-granted files; never discover paths or modify source
//! bytes. Native/private records win, then Native V2, then JS sources in the
//! documented order below. Each source is marked in the same atomic store write.
use crate::native_core::extensions::{color_shades, v2_sdk::STORE};
use serde_json::{json, Value};
use tezbar_extension_sdk::{read_store, update_store, CoreResult, ExtensionContext};

pub const MIGRATION_KEY: &str = "nativeStore";
const MARKER: &str = "_nativeStoreImported";
const JS_MARKER: &str = "_jsStoresImported";
// Managed canonical ID, managed slug, registry ID, registry slug, old extension
// slug, old extension ID. At each root the current store precedes localStorage.
pub const JS_SOURCES: &[(&str, &str)] = &[
    (
        "jsManagedIdCurrent",
        "extensions/packages/raycast.color-shades/.tezbar-local-storage.json",
    ),
    (
        "jsManagedIdOlder",
        "extensions/packages/raycast.color-shades/localStorage.json",
    ),
    (
        "jsManagedSlugCurrent",
        "extensions/packages/color-shades/.tezbar-local-storage.json",
    ),
    (
        "jsManagedSlugOlder",
        "extensions/packages/color-shades/localStorage.json",
    ),
    (
        "jsRegistryIdCurrent",
        "extension-registry/packages/raycast.color-shades/.tezbar-local-storage.json",
    ),
    (
        "jsRegistryIdOlder",
        "extension-registry/packages/raycast.color-shades/localStorage.json",
    ),
    (
        "jsRegistrySlugCurrent",
        "extension-registry/packages/color-shades/.tezbar-local-storage.json",
    ),
    (
        "jsRegistrySlugOlder",
        "extension-registry/packages/color-shades/localStorage.json",
    ),
    (
        "jsExtensionsSlugCurrent",
        "extensions/color-shades/.tezbar-local-storage.json",
    ),
    (
        "jsExtensionsSlugOlder",
        "extensions/color-shades/localStorage.json",
    ),
    (
        "jsExtensionsIdCurrent",
        "extensions/raycast.color-shades/.tezbar-local-storage.json",
    ),
    (
        "jsExtensionsIdOlder",
        "extensions/raycast.color-shades/localStorage.json",
    ),
];

async fn source(path: &std::path::Path) -> Result<Option<Value>, String> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Invalid Color Shades migration store: {e}"))?;
            if !value.is_object() {
                return Err("Color Shades migration store must be an object".into());
            }
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Color Shades migration store: {e}")),
    }
}

pub async fn initialize(ctx: &ExtensionContext) -> CoreResult {
    let current = read_store(ctx, STORE).await?;
    if current.get(JS_MARKER).is_some_and(|v| !v.is_object()) {
        return Err("Invalid JavaScript migration markers".into());
    }
    let native = if current[MARKER] != true {
        if let Some(path) = ctx.migration_paths.get(MIGRATION_KEY) {
            Some(source(path).await?.unwrap_or_else(|| json!({})))
        } else {
            None
        }
    } else {
        None
    };
    let mut js = Vec::new();
    for (key, _) in JS_SOURCES {
        if current[JS_MARKER][*key] == true {
            continue;
        }
        if let Some(path) = ctx.migration_paths.get(*key) {
            if let Some(value) = source(path).await? {
                js.push(((*key).to_owned(), value));
            }
        }
    }
    if native.is_none() && js.is_empty() {
        return Ok(current);
    }
    // SDK update_store validates in memory, syncs a temporary file, atomically
    // replaces storage.json and syncs its directory. Rejection writes no marker
    // and no partial palettes; later initialization can retry the same sources.
    update_store(ctx, STORE, move |store| {
        if store[MARKER] != true {
            if let Some(legacy) = native {
                color_shades::import_palettes(store, &legacy)
                    .map_err(|e| format!("Color Shades migration rejected: {e}"))?;
                store[MARKER] = json!(true);
            }
        }
        for (key, legacy) in js {
            if store[JS_MARKER][&key] == true {
                continue;
            }
            color_shades::import_js_palettes(store, &legacy)
                .map_err(|e| format!("Color Shades JS migration rejected ({key}): {e}"))?;
            if store.get(JS_MARKER).is_none() {
                store[JS_MARKER] = json!({});
            }
            store[JS_MARKER][key] = json!(true);
        }
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
    fn javascript(base: &str, date: &str) -> (String, Value) {
        let (name, committed) = committed(base, date);
        let mut palette = committed["palette"].clone();
        palette["colors"]["50"] = json!("#aBcDeF");
        (
            name.clone(),
            json!({name: serde_json::to_string(&palette).unwrap()}),
        )
    }

    #[tokio::test]
    async fn every_manifest_js_file_grant_imports_exact_snapshots() {
        let manifest: Value = serde_json::from_str(include_str!("../extension.json")).unwrap();
        assert_eq!(
            manifest["migrationPaths"].as_object().unwrap().len(),
            JS_SOURCES.len() + 1
        );
        for (key, relative) in JS_SOURCES {
            assert_eq!(manifest["migrationPaths"][*key], *relative);
            let dir = tempfile::tempdir().unwrap();
            let mut f = fixture(dir.path());
            let source = dir.path().join(relative);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            let (name, legacy) = javascript("#82123d", "2024-02-03T04:05:06.007Z");
            let bytes = serde_json::to_vec(&legacy).unwrap();
            std::fs::write(&source, &bytes).unwrap();
            f.context
                .migration_paths
                .insert((*key).into(), source.clone());
            let saved = initialize(&f.context).await.unwrap();
            let original: Value = serde_json::from_str(legacy[&name].as_str().unwrap()).unwrap();
            assert_eq!(saved["palettes"][&name]["palette"], original);
            assert_eq!(saved["palettes"][&name]["javascript_snapshot"], 1);
            assert_eq!(saved[JS_MARKER][*key], true);
            assert_eq!(std::fs::read(source).unwrap(), bytes);
            assert_eq!(initialize(&f.context).await.unwrap(), saved);
        }
    }

    #[tokio::test]
    async fn native_ownership_source_precedence_and_late_grants() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path());
        let (native_name, native) = committed("#00ff00", "2026-01-01T00:00:00.000Z");
        update_store(&f.context, STORE, {
            let native_name = native_name.clone();
            let native = native.clone();
            move |s| {
                s["palettes"] = json!({native_name:native});
                s[MARKER] = json!(true);
                s["other"] = json!(7);
                Ok(())
            }
        })
        .await
        .unwrap();
        // An old Native V2 marker must not disable new JS grants.
        assert!(initialize(&f.context)
            .await
            .unwrap()
            .get(JS_MARKER)
            .is_none());
        let (js_name, newer) = javascript("#82123d", "2025-01-01T00:00:00.000Z");
        let (_, older) = javascript("#82123d", "2020-01-01T00:00:00.000Z");
        let (_, conflict) = javascript("#00ff00", "2027-01-01T00:00:00.000Z");
        // Reverse grant insertion order must not determine collision ownership.
        for (i, value) in [older, conflict, newer.clone()].into_iter().enumerate() {
            let key = [JS_SOURCES[2].0, JS_SOURCES[1].0, JS_SOURCES[0].0][i];
            let path = dir.path().join(format!("{i}.json"));
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            f.context.migration_paths.insert(key.into(), path);
        }
        let saved = initialize(&f.context).await.unwrap();
        assert_eq!(saved["palettes"][&native_name], native);
        let expected: Value = serde_json::from_str(newer[&js_name].as_str().unwrap()).unwrap();
        assert_eq!(saved["palettes"][&js_name]["palette"], expected);
        assert_eq!(saved["other"], 7);
        // Simulate user deletion. Changing old sources later cannot resurrect it.
        update_store(&f.context, STORE, move |s| {
            s["palettes"] = json!({});
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(initialize(&f.context).await.unwrap()["palettes"], json!({}));
        assert!(f.calls().is_empty());
    }

    #[tokio::test]
    async fn js_rejection_is_atomic_and_corrected_source_retries() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path());
        let (name, good) = javascript("#82123d", "2024-01-01T00:00:00.000Z");
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        std::fs::write(&a, serde_json::to_vec(&good).unwrap()).unwrap();
        f.context
            .migration_paths
            .insert(JS_SOURCES[0].0.into(), a.clone());
        f.context
            .migration_paths
            .insert(JS_SOURCES[1].0.into(), b.clone());
        update_store(&f.context, STORE, |s| {
            s["unrelated"] = json!(42);
            Ok(())
        })
        .await
        .unwrap();
        let destination = dir.path().join("private/color-shades/storage.json");
        let original = std::fs::read(&destination).unwrap();
        let palette: Value = serde_json::from_str(good[&name].as_str().unwrap()).unwrap();
        let mut bad_date = palette.clone();
        bad_date["creationDate"] = json!("yesterday");
        let mut missing = palette.clone();
        missing["colors"].as_object_mut().unwrap().remove("950");
        let mut extra = palette.clone();
        extra["colors"]["999"] = json!("#123456");
        let mut invalid_color = palette.clone();
        invalid_color["colors"]["50"] = json!("red");
        let mut unknown_name = palette.clone();
        unknown_name["name"] = json!("arbitrary");
        let mut bad = vec![
            b"{broken".to_vec(),
            b"[]".to_vec(),
            serde_json::to_vec(&json!({name.clone():palette})).unwrap(),
            serde_json::to_vec(&json!({"wrong":good[&name]})).unwrap(),
        ];
        for p in [bad_date, missing, extra, invalid_color, unknown_name] {
            bad.push(
                serde_json::to_vec(&json!({name.clone():serde_json::to_string(&p).unwrap()}))
                    .unwrap(),
            );
        }
        for bytes in bad {
            std::fs::write(&b, &bytes).unwrap();
            assert!(initialize(&f.context).await.is_err());
            assert_eq!(
                std::fs::read(&destination).unwrap(),
                original,
                "no partial import or markers"
            );
            assert_eq!(std::fs::read(&b).unwrap(), bytes);
        }
        std::fs::write(&b, b"{}").unwrap();
        let saved = initialize(&f.context).await.unwrap();
        assert_eq!(saved["palettes"][&name]["palette"], palette);
        assert_eq!(saved[JS_MARKER][JS_SOURCES[0].0], true);
        assert_eq!(saved[JS_MARKER][JS_SOURCES[1].0], true);
        // Imported empty sources also remain idempotent, without reopening bytes.
        std::fs::write(&a, b"{broken").unwrap();
        std::fs::write(&b, b"{broken").unwrap();
        assert_eq!(initialize(&f.context).await.unwrap(), saved);
    }
}
