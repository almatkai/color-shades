use super::*;
use crate::native_core::common::{CorePaths, CoreRouter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
const GEN: &str = "generate-color-shades";
const HIST: &str = "view-color-shades";
const RUN: &str = "extension:run-command";
const ACT: &str = "extension:invoke-action";
type Calls = Arc<Mutex<Vec<(String, Vec<Value>)>>>;
fn context(path: &Path) -> (CoreContext, Calls, Arc<AtomicU64>) {
    let ctx = CoreContext::headless(CorePaths {
        data_dir: path.join("data"),
        config_dir: path.join("config"),
        resource_dir: path.join("resource"),
        temp_dir: path.join("tmp"),
        home_dir: path.into(),
    });
    let calls = Arc::new(Mutex::new(vec![]));
    let fail = Arc::new(AtomicU64::new(0));
    let (c, f) = (calls.clone(), fail.clone());
    let router: CoreRouter = Arc::new(move |channel, args| {
        let (c, f) = (c.clone(), f.clone());
        Box::pin(async move {
            c.lock().unwrap().push((channel.clone(), args));
            assert_eq!(channel, "clipboard:write", "No unapproved effect route");
            match f.load(Ordering::Relaxed) {
                1 => Err("clipboard unavailable".into()),
                2 => Ok(json!({"ok":false,"message":"clipboard unavailable"})),
                3 => Ok(json!(true)),
                4 => Ok(json!({"ok":true,"error":"clipboard unavailable"})),
                5 => Ok(json!({"ok":true,"success":false})),
                _ => Ok(json!({"ok":true})),
            }
        })
    });
    ctx.set_router(router).unwrap();
    (ctx, calls, fail)
}
fn path(ctx: &CoreContext) -> PathBuf {
    ctx.paths
        .data_dir
        .join("native-extensions-v2/color-shades/storage.json")
}
fn id(view: &Value, title: &str) -> String {
    view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == title)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
async fn invoke(ctx: &CoreContext, command: &str, s: &mut Value, id: &str) -> CoreResult {
    handle(ctx,command,ACT,&json!({"actionId":id,"text":"EVIL","color":"#BADBAD","url":"https://invalid.invalid","colors":{"500":"EVIL"}}),s).await
}
async fn launch(ctx: &CoreContext, c: &str, s: &mut Value, color: &str) -> Value {
    handle(ctx, c, RUN, &json!({"argumentValues":{"color":color}}), s)
        .await
        .unwrap()
}

#[tokio::test]
async fn every_copy_action_rejects_failed_and_malformed_acknowledgements() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, calls, fail) = context(d.path());
    let mut s = Value::Null;
    let view = launch(&ctx, GEN, &mut s, "#abc").await;
    let before = s.clone();
    for title in [
        "Copy Shade",
        "Copy All Colors as Variables",
        "Copy All Colors as JSON",
    ] {
        let action = id(&view, title);
        for mode in 1..=5 {
            fail.store(mode, Ordering::Relaxed);
            let count = calls.lock().unwrap().len();
            assert!(invoke(&ctx, GEN, &mut s, &action).await.is_err());
            assert_eq!(calls.lock().unwrap().len(), count + 1);
            assert_eq!(s, before);
        }
        fail.store(0, Ordering::Relaxed);
        assert_eq!(
            invoke(&ctx, GEN, &mut s, &action).await.unwrap()["mode"],
            "unchanged"
        );
        assert_eq!(s, before);
    }
}

#[tokio::test]
async fn all_copy_routes_payload_rejection_retry_and_stale_generation() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, calls, fail) = context(d.path());
    let mut s = Value::Null;
    let v = launch(&ctx, GEN, &mut s, "#aBc").await;
    assert_eq!(v["root"]["type"], "Grid");
    assert_eq!(v["root"]["props"]["columns"], 11);
    assert_eq!(v["root"]["children"].as_array().unwrap().len(), 11);
    for row in v["root"]["children"].as_array().unwrap() {
        let a = row["props"]["actionIds"][0].as_str().unwrap();
        invoke(&ctx, GEN, &mut s, a).await.unwrap();
        assert_eq!(
            calls.lock().unwrap().last().unwrap().1,
            vec![row["props"]["subtitle"].clone()]
        );
    }
    let saved = s.clone();
    let copy = id(&v, "Copy All Colors as Variables");
    fail.store(1, Ordering::Relaxed);
    assert!(invoke(&ctx, GEN, &mut s, &copy).await.is_err());
    assert_eq!(s, saved);
    fail.store(0, Ordering::Relaxed);
    invoke(&ctx, GEN, &mut s, &copy).await.unwrap();
    let current = session(&s, GEN).unwrap();
    let Screen::Palette(c) = current.screen else {
        panic!()
    };
    assert_eq!(
        calls.lock().unwrap().last().unwrap().1,
        vec![json!(c.recompute().unwrap().variables().unwrap())]
    );
    invoke(&ctx, GEN, &mut s, &id(&v, "Copy All Colors as JSON"))
        .await
        .unwrap();
    assert_eq!(
        calls.lock().unwrap().last().unwrap().1,
        vec![json!(c.recompute().unwrap().json().unwrap())]
    );
    assert_eq!(s, saved);
    let before = calls.lock().unwrap().len();
    for (command, op, payload) in [
        ("wrong", ACT, json!({"actionId":copy})),
        (GEN, "wrong", json!({"actionId":copy})),
        (GEN, ACT, json!({"actionId":"color-shades:0:copy:50"})),
        (HIST, ACT, json!({"actionId":copy})),
    ] {
        assert!(handle(&ctx, command, op, &payload, &mut s).await.is_err());
        assert_eq!(s, saved);
    }
    assert_eq!(calls.lock().unwrap().len(), before);
    let next = invoke(&ctx, GEN, &mut s, &id(&v, "Use as Base Color"))
        .await
        .unwrap();
    assert_ne!(s, saved);
    assert!(invoke(&ctx, GEN, &mut s, &copy).await.is_err());
    let p = session(&s, GEN).unwrap();
    let Screen::Palette(c) = p.screen else {
        panic!()
    };
    assert_eq!(
        c.palette.colors[&500],
        v["root"]["children"][0]["props"]["subtitle"]
    );
    let old = id(&next, "Copy Shade");
    handle(&ctx, GEN, "extension:dispose-session", &json!({}), &mut s)
        .await
        .unwrap();
    assert!(invoke(&ctx, GEN, &mut s, &old).await.is_err());
    let v = launch(&ctx, GEN, &mut s, "#abc").await;
    let old = id(&v, "Copy Shade");
    launch(&ctx, GEN, &mut s, "#abc").await;
    assert!(invoke(&ctx, GEN, &mut s, &old).await.is_err());
    assert!(!ctx.paths.config_dir.exists());
    assert!(!ctx.paths.resource_dir.exists());
}
#[tokio::test]
async fn every_use_as_base_action_recomputes_and_saves() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, calls, _) = context(d.path());
    for i in 0..11 {
        let mut s = Value::Null;
        let v = launch(&ctx, GEN, &mut s, "#82123d").await;
        let row = &v["root"]["children"][i];
        let a = row["props"]["actionIds"][1].as_str().unwrap();
        invoke(&ctx, GEN, &mut s, a).await.unwrap();
        let current = session(&s, GEN).unwrap();
        let Screen::Palette(c) = current.screen else {
            panic!()
        };
        assert_eq!(c.palette.colors[&500], row["props"]["subtitle"]);
        assert_eq!(
            entries(&sdk::read_store(&ctx, SLUG).await.unwrap()).unwrap()[&c.palette.name],
            *c
        );
    }
    assert!(calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn draft_back_current_palette_and_failure_preserve_retryable_state() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, _, _) = context(d.path());
    let mut s = Value::Null;
    let draft = handle(&ctx, GEN, RUN, &json!({}), &mut s).await.unwrap();
    assert_eq!(draft["root"]["type"], "Form");
    let submit = id(&draft, "Generate Color Shades");
    assert!(handle(
        &ctx,
        GEN,
        ACT,
        &json!({"actionId":submit,"formValues":{"color":"bogus"}}),
        &mut s
    )
    .await
    .is_err());
    assert_eq!(session(&s, GEN).unwrap().draft, "bogus");
    let view = handle(
        &ctx,
        GEN,
        ACT,
        &json!({"actionId":submit,"formValues":{"color":"#AbC"}}),
        &mut s,
    )
    .await
    .unwrap();
    let old = s.clone();
    let base = id(&view, "Use as Base Color");
    let storage = std::fs::read(path(&ctx)).unwrap();
    std::fs::remove_file(path(&ctx)).unwrap();
    std::fs::create_dir(path(&ctx)).unwrap();
    assert!(invoke(&ctx, GEN, &mut s, &base).await.is_err());
    assert_eq!(s, old);
    std::fs::remove_dir(path(&ctx)).unwrap();
    std::fs::write(path(&ctx), storage).unwrap();
    let view = invoke(&ctx, GEN, &mut s, &base).await.unwrap();
    let updated = s.clone();
    for op in [
        "extension:refresh-session",
        "extension:load-more",
        "extension:search-text-changed",
    ] {
        assert_eq!(
            handle(&ctx, GEN, op, &json!({"searchText":"test"}), &mut s)
                .await
                .unwrap()["mode"],
            "unchanged"
        );
        assert_eq!(s, updated);
    }
    let draft = invoke(&ctx, GEN, &mut s, &id(&view, "Go Back"))
        .await
        .unwrap();
    assert_eq!(draft["root"]["children"][0]["props"]["value"], "#AbC");
    assert!(invoke(&ctx, GEN, &mut s, &base).await.is_err());
}
#[tokio::test]
async fn history_resaves_sorts_collides_deletes_preserves_unrelated_and_checks_stale() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, calls, _) = context(d.path());
    let mut gen = Value::Null;
    sdk::update_store(&ctx, SLUG, |v| {
        v["unrelated"] = json!({"keep":true});
        Ok(())
    })
    .await
    .unwrap();
    launch(&ctx, GEN, &mut gen, "#abc").await;
    launch(&ctx, GEN, &mut gen, "#aBc").await;
    assert_eq!(
        entries(&sdk::read_store(&ctx, SLUG).await.unwrap())
            .unwrap()
            .len(),
        1
    );
    launch(&ctx, GEN, &mut gen, "#ff0000").await;
    let mut s = Value::Null;
    let list = launch(&ctx, HIST, &mut s, "").await;
    assert_eq!(list["root"]["type"], "List");
    assert_eq!(list["root"]["children"].as_array().unwrap().len(), 2);
    let rows = list["root"]["children"].as_array().unwrap();
    assert!(
        rows[0]["props"]["accessories"][0]["date"].as_str().unwrap()
            >= rows[1]["props"]["accessories"][0]["date"].as_str().unwrap()
    );
    assert_eq!(rows[0]["props"]["icon"]["source"], "CircleFilled");
    let view_id = rows[0]["props"]["actionIds"][0].as_str().unwrap();
    let p = invoke(&ctx, HIST, &mut s, view_id).await.unwrap();
    assert_eq!(p["root"]["type"], "Grid");
    assert!(invoke(&ctx, HIST, &mut s, view_id).await.is_err());
    let back = invoke(&ctx, HIST, &mut s, &id(&p, "Go Back"))
        .await
        .unwrap();
    assert_eq!(back["root"]["type"], "List");
    let del = id(&back, "Delete Entry");
    let cleared = invoke(&ctx, HIST, &mut s, &del).await.unwrap();
    assert!(invoke(&ctx, HIST, &mut s, &del).await.is_err());
    let empty = invoke(&ctx, HIST, &mut s, &id(&cleared, "Delete All Entries"))
        .await
        .unwrap();
    assert!(empty["root"]["children"].as_array().unwrap().is_empty());
    assert_eq!(
        sdk::read_store(&ctx, SLUG).await.unwrap(),
        json!({"unrelated":{"keep":true},"palettes":{}})
    );
    launch(&ctx, GEN, &mut gen, "abc").await;
    let old = launch(&ctx, HIST, &mut s, "").await;
    launch(&ctx, GEN, &mut gen, "#aBc").await; // same name, different preserved spelling
    for title in ["View Palette", "Delete Entry", "Delete All Entries"] {
        assert!(invoke(&ctx, HIST, &mut s, &id(&old, title)).await.is_err());
    }
    assert!(calls.lock().unwrap().is_empty());
    if let Ok(out) = std::env::var("COLOR_SHADES_WIRE_PATH") {
        let mut draft = Value::Null;
        let form = handle(&ctx, GEN, RUN, &json!({}), &mut draft)
            .await
            .unwrap();
        std::fs::write(
            out,
            serde_json::to_vec_pretty(&json!({"grid":p,"list":back,"form":form})).unwrap(),
        )
        .unwrap();
    }
}
#[tokio::test]
async fn corruption_never_overwrites_existing_data_or_consumes_actions() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, _, _) = context(d.path());
    let mut s = Value::Null;
    let view = launch(&ctx, GEN, &mut s, "abc").await;
    let old = s.clone();
    let good = std::fs::read(path(&ctx)).unwrap();
    for bad in [
        b"{broken".as_slice(),
        b"[]",
        b"{\"palettes\":null}",
        b"{\"palettes\":{\"bad\":{}}}",
    ] {
        std::fs::write(path(&ctx), bad).unwrap();
        assert!(invoke(&ctx, GEN, &mut s, &id(&view, "Use as Base Color"))
            .await
            .is_err());
        assert_eq!(s, old);
        assert_eq!(std::fs::read(path(&ctx)).unwrap(), bad);
        let mut hist = Value::Null;
        assert!(handle(&ctx, HIST, RUN, &json!({}), &mut hist)
            .await
            .is_err());
        assert_eq!(std::fs::read(path(&ctx)).unwrap(), bad);
    }
    std::fs::write(path(&ctx), good).unwrap();
    invoke(&ctx, GEN, &mut s, &id(&view, "Use as Base Color"))
        .await
        .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sessions_do_not_lose_entries_or_unknown_keys() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, _, _) = context(d.path());
    sdk::update_store(&ctx, SLUG, |v| {
        v["unknown"] = json!(17);
        Ok(())
    })
    .await
    .unwrap();
    let data: Vec<Value> = serde_json::from_str(include_str!("../vendor/ntc.json")).unwrap();
    let mut tasks = vec![];
    for item in data.iter().take(20) {
        let ctx = ctx.clone();
        let color = item["hex"].as_str().unwrap().to_string();
        tasks.push(tokio::spawn(async move {
            let mut s = Value::Null;
            launch(&ctx, GEN, &mut s, &color).await;
            let Screen::Palette(c) = session(&s, GEN).unwrap().screen else {
                panic!()
            };
            c.palette.name
        }));
    }
    let mut expected = std::collections::BTreeSet::new();
    for t in tasks {
        expected.insert(t.await.unwrap());
    }
    let store = sdk::read_store(&ctx, SLUG).await.unwrap();
    let data = entries(&store).unwrap();
    assert_eq!(
        data.keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert_eq!(store["unknown"], 17);
}

#[tokio::test]
async fn history_effect_failures_retry_and_identical_overwrites_are_stale() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, _, _) = context(d.path());
    let mut gen = Value::Null;
    for title in ["View Palette", "Delete Entry", "Delete All Entries"] {
        launch(&ctx, GEN, &mut gen, "#abc").await;
        let mut s = Value::Null;
        let list = launch(&ctx, HIST, &mut s, "").await;
        let action = id(&list, title);
        let old = s.clone();
        let bytes = std::fs::read(path(&ctx)).unwrap();
        std::fs::remove_file(path(&ctx)).unwrap();
        std::fs::create_dir(path(&ctx)).unwrap();
        assert!(invoke(&ctx, HIST, &mut s, &action).await.is_err());
        assert_eq!(s, old);
        std::fs::remove_dir(path(&ctx)).unwrap();
        std::fs::write(path(&ctx), &bytes).unwrap();
        invoke(&ctx, HIST, &mut s, &action).await.unwrap();
    }
    launch(&ctx, GEN, &mut gen, "#abc").await;
    let mut s = Value::Null;
    let list = launch(&ctx, HIST, &mut s, "").await;
    // Same base/colors/name/date/hue order, differing only in native revision.
    sdk::update_store(&ctx, SLUG, |store| {
        let mut data = entries(store)?;
        data.values_mut().next().unwrap().revision += 1;
        store[KEY] = serde_json::to_value(data).unwrap();
        Ok(())
    })
    .await
    .unwrap();
    for title in ["View Palette", "Delete Entry", "Delete All Entries"] {
        assert!(invoke(&ctx, HIST, &mut s, &id(&list, title)).await.is_err());
    }
}

#[tokio::test]
async fn malformed_persisted_hue_state_is_an_error_not_a_panic_or_replacement() {
    let d = tempfile::tempdir().unwrap();
    let (ctx, _, _) = context(d.path());
    let mut s = Value::Null;
    let grid = launch(&ctx, GEN, &mut s, "abc").await;
    let mut store: Value = serde_json::from_slice(&std::fs::read(path(&ctx)).unwrap()).unwrap();
    let data = store[KEY]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    data["before"]["cmy"] = json!([-2000000, 0, 1]);
    let bytes = serde_json::to_vec(&store).unwrap();
    std::fs::write(path(&ctx), &bytes).unwrap();
    let old = s.clone();
    assert!(invoke(&ctx, GEN, &mut s, &id(&grid, "Use as Base Color"))
        .await
        .is_err());
    assert_eq!(s, old);
    assert_eq!(std::fs::read(path(&ctx)).unwrap(), bytes);
}
