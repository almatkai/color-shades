//! Drives the real executable over stdin/stdout JSONL with a narrow recording
//! fake host. No GUI, clipboard, browser or network effect is performed: the
//! only reverse RPC the extension may issue is `host.clipboard.write`, which this
//! harness records and answers itself.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use tezbar_color_shades::Generator;

const ID: &str = "com.tezbar.native.color-shades";
const GEN: &str = "generate-color-shades";
const HIST: &str = "view-color-shades";

pub fn binary() -> PathBuf {
    std::env::var_os("COLOR_SHADES_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_tezbar-color-shades")))
}

/// What the fake host answers to the next reverse clipboard write.
#[derive(Clone)]
enum Reply {
    Ok,
    Error(&'static str),
    Malformed,
}

struct Host {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<Value>,
    /// Recorded reverse requests: (method, params).
    calls: Vec<(String, Value)>,
    reply: Reply,
}

fn hid() -> String {
    format!("h:{}", uuid::Uuid::new_v4())
}

impl Host {
    fn spawn(dir: &Path) -> (Self, Value) {
        let mut command = Command::new(binary());
        command.current_dir(dir).env_clear();
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let path = if cfg!(windows) {
            PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:/Windows".into()))
                .join("System32")
        } else {
            PathBuf::from("/usr/bin:/bin")
        };
        let mut child = command
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn extension");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, frames) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                let value: Value = serde_json::from_str(&line).expect("JSON frame");
                if tx.send(value).is_err() {
                    return;
                }
            }
        });
        let mut host = Self {
            child,
            stdin,
            frames,
            calls: vec![],
            reply: Reply::Ok,
        };
        let hello = host.next();
        (host, hello)
    }
    fn next(&mut self) -> Value {
        self.frames
            .recv_timeout(Duration::from_secs(10))
            .expect("frame before deadline")
    }
    fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.stdin.write_all(&bytes).unwrap();
        self.stdin.flush().unwrap();
    }
    fn answer(&mut self, frame: &Value) {
        let method = frame["method"].as_str().unwrap().to_owned();
        self.calls.push((method.clone(), frame["params"].clone()));
        assert_eq!(
            method, "host.clipboard.write",
            "only the narrowly granted clipboard write may be requested"
        );
        let id = frame["id"].clone();
        let reply = match self.reply {
            Reply::Ok => json!({"type":"response","id":id,"result":{"ok":true}}),
            Reply::Malformed => json!({"type":"response","id":id,"result":{"ok":false}}),
            Reply::Error(m) => {
                json!({"type":"response","id":id,"error":{"code":"host_error","message":m}})
            }
        };
        self.send(reply);
    }
    /// Sends a request and serves reverse calls until its response arrives.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = hid();
        self.send(json!({"type":"request","id":id,"method":method,"params":params}));
        loop {
            let frame = self.next();
            match frame["type"].as_str() {
                Some("request") => self.answer(&frame),
                Some("event") | Some("cancel") => {}
                Some("response") if frame["id"] == id.as_str() => {
                    return match frame.get("error") {
                        Some(e) => Err(e["message"].as_str().unwrap().to_owned()),
                        None => Ok(frame["result"].clone()),
                    };
                }
                _ => panic!("unexpected frame {frame}"),
            }
        }
    }
    fn initialize(&mut self, dir: &Path, grants: Value, migration: Value) -> Result<Value, String> {
        self.request(
            "initialize",
            json!({"protocol":1,"extensionId":ID,"platform":if cfg!(windows) { "Windows" } else { "macOS" },
                "packageDir":dir.join("package"),"dataDir":dir.join("data"),"tempDir":dir.join("tmp"),
                "grantedCapabilities":grants,"reason":"command","migrationPaths":migration}),
        )
    }
    fn open(&mut self, session: &str, command: &str, args: Value) -> Result<Value, String> {
        self.request(
            "session.open",
            json!({"sessionId":session,"commandName":command,"preferences":{},"argumentValues":args}),
        )
    }
    fn act(&mut self, session: &str, action: &str) -> Result<Value, String> {
        self.call(
            session,
            json!({"actionId":action,"text":"EVIL","colors":{"500":"EVIL"}}),
        )
    }
    fn call(&mut self, session: &str, payload: Value) -> Result<Value, String> {
        self.request(
            "session.call",
            json!({"sessionId":session,"operation":"extension:invoke-action","payload":payload}),
        )
    }
    fn shutdown(mut self) {
        assert_eq!(self.request("shutdown", json!({})).unwrap(), json!({}));
        let status = wait(&mut self.child);
        assert!(status.success(), "clean exit, got {status:?}");
    }
}

fn wait(child: &mut Child) -> std::process::ExitStatus {
    for _ in 0..100 {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    panic!("extension did not exit");
}

fn action(view: &Value, title: &str, nth: usize) -> String {
    view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["title"] == title)
        .nth(nth)
        .unwrap_or_else(|| panic!("action {title}"))["id"]
        .as_str()
        .unwrap()
        .into()
}

fn store(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("data/color-shades/storage.json")).unwrap())
        .unwrap()
}

fn legacy_entry(base: &str, date: &str) -> (String, Value) {
    let before = Generator::default();
    let palette = before.clone().generate(base, date).unwrap();
    (
        palette.name.clone(),
        json!({"revision":3,"base":base,"before":before,"palette":palette}),
    )
}

#[test]
fn protocol_info_matches_manifest() {
    let out = Command::new(binary())
        .arg("--protocol-info")
        .output()
        .unwrap();
    assert!(out.status.success());
    let info: Value = serde_json::from_slice(&out.stdout).unwrap();
    let manifest: Value =
        serde_json::from_str(include_str!("../extension.json")).expect("root manifest");
    assert_eq!(info["protocol"], 1);
    assert_eq!(info["id"], manifest["id"]);
    assert_eq!(info["version"], manifest["version"]);
    let names: Vec<_> = manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].clone())
        .collect();
    assert_eq!(info["commands"], json!(names));
}

#[test]
fn generate_copy_base_back_and_history_flow_over_real_pipes() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    // A store left by the compiled-in Native V2 adapter, granted explicitly.
    let (old_name, old_entry) = legacy_entry("#00ff00", "2026-01-02T03:04:05.006Z");
    let legacy = d.join("legacy-storage.json");
    std::fs::write(
        &legacy,
        serde_json::to_vec(&json!({"palettes":{old_name.clone():old_entry}})).unwrap(),
    )
    .unwrap();
    let legacy_bytes = std::fs::read(&legacy).unwrap();

    let (mut host, hello) = Host::spawn(d);
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["id"], ID);
    assert_eq!(hello["protocol"], 1);
    assert_eq!(hello["version"], env!("CARGO_PKG_VERSION"));
    // Sessions are refused before initialize completes.
    assert!(host.open("early", GEN, json!({"color":"#abc"})).is_err());
    let ready = host
        .initialize(d, json!(["clipboard.write"]), json!({"nativeStore":legacy}))
        .unwrap();
    assert_eq!(ready["ready"], true);
    assert_eq!(
        std::fs::read(&legacy).unwrap(),
        legacy_bytes,
        "legacy untouched"
    );
    assert_eq!(store(d)["_nativeStoreImported"], true);

    // History shows the migrated palette.
    let history = host.open("hist-1", HIST, json!({})).unwrap();
    assert_eq!(history["root"]["type"], "List");
    assert_eq!(history["root"]["children"][0]["props"]["title"], old_name);

    // Generate: an 11-shade Grid with level 500 keeping the input spelling.
    let view = host.open("gen", GEN, json!({"color":"#82123d"})).unwrap();
    assert_eq!(view["mode"], "view");
    assert_eq!(view["root"]["type"], "Grid");
    let items = view["root"]["children"].as_array().unwrap().clone();
    assert_eq!(items.len(), 11);
    assert_eq!(items[5]["props"]["title"], "500");
    assert_eq!(items[5]["props"]["subtitle"], "#82123d");
    let expected = Generator::default()
        .generate("#82123d", "2026-01-01T00:00:00.000Z")
        .unwrap();
    for item in &items {
        let level: u16 = item["props"]["id"].as_str().unwrap().parse().unwrap();
        assert_eq!(item["props"]["subtitle"], expected.colors[&level]);
    }
    assert_eq!(view["title"], expected.name);
    assert!(view["root"]["props"]["backActionId"].is_string());

    // Copy Shade derives text from the committed palette, never the payload.
    let copy_500 = items[5]["props"]["actionIds"][0]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(host.act("gen", &copy_500).unwrap()["mode"], "unchanged");
    assert_eq!(
        host.calls.last().unwrap(),
        &("host.clipboard.write".to_owned(), json!({"text":"#82123d"}))
    );
    // Host failures and unacknowledged writes surface as errors and stay retryable.
    host.reply = Reply::Error("clipboard unavailable");
    assert_eq!(
        host.act("gen", &copy_500).unwrap_err(),
        "clipboard unavailable"
    );
    host.reply = Reply::Malformed;
    assert!(host.act("gen", &copy_500).is_err());
    host.reply = Reply::Ok;
    let variables = action(&view, "Copy All Colors as Variables", 0);
    host.act("gen", &variables).unwrap();
    assert_eq!(
        host.calls.last().unwrap().1["text"],
        expected.variables().unwrap()
    );
    host.act("gen", &action(&view, "Copy All Colors as JSON", 3))
        .unwrap();
    assert_eq!(
        host.calls.last().unwrap().1["text"],
        expected.json().unwrap()
    );

    // Use as Base Color saves and presents a new palette; older IDs go stale.
    let base_50 = items[0]["props"]["actionIds"][1]
        .as_str()
        .unwrap()
        .to_owned();
    let next = host.act("gen", &base_50).unwrap();
    assert_eq!(next["root"]["type"], "Grid");
    assert_eq!(
        next["root"]["children"][5]["props"]["subtitle"],
        expected.colors[&50]
    );
    let calls = host.calls.len();
    assert!(host.act("gen", &copy_500).is_err(), "stale action rejected");
    assert!(host.act("gen", "color-shades:999999:copy:50").is_err());
    assert!(host.call("gen", json!({})).is_err(), "missing action ID");
    assert_eq!(
        host.calls.len(),
        calls,
        "rejected actions perform no effect"
    );

    // Header Back returns to the retained draft form.
    let back = next["root"]["props"]["backActionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let draft = host.act("gen", &back).unwrap();
    assert_eq!(draft["root"]["type"], "Form");
    assert_eq!(draft["root"]["children"][0]["props"]["value"], "#82123d");
    // Submitting an invalid colour fails, keeps the draft and the action retryable.
    let submit = action(&draft, "Generate Color Shades", 0);
    let bad = host.call(
        "gen",
        json!({"actionId":submit,"formValues":{"color":"nope"}}),
    );
    assert!(bad.unwrap_err().contains("unknown color"));
    let ok = host
        .call(
            "gen",
            json!({"actionId":submit,"formValues":{"color":"#abc"}}),
        )
        .unwrap();
    assert_eq!(ok["root"]["children"][5]["props"]["subtitle"], "#aabbcc");

    // Persisted history: three generated palettes plus the migrated one.
    let saved = store(d);
    let names: Vec<_> = saved["palettes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert!(names.contains(&old_name) && names.contains(&expected.name));
    assert_eq!(names.len(), 4);

    // A stale history session cannot delete after the store changed.
    let delete_stale = action(&history, "Delete All Entries", 0);
    assert!(host.act("hist-1", &delete_stale).is_err());
    assert_eq!(store(d)["palettes"].as_object().unwrap().len(), 4);

    let history = host.open("hist-2", HIST, json!({})).unwrap();
    let rows = history["root"]["children"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["props"]["title"], ok["title"], "newest first");
    let index = rows
        .iter()
        .position(|r| r["props"]["title"] == old_name)
        .unwrap();
    // View Palette regenerates from the saved base and resaves it.
    let view_old = history["root"]["children"][index]["props"]["actionIds"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let palette = host.act("hist-2", &view_old).unwrap();
    assert_eq!(palette["title"], old_name);
    assert_eq!(
        palette["root"]["children"][5]["props"]["subtitle"],
        "#00ff00"
    );
    let back = palette["root"]["props"]["backActionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let history = host.act("hist-2", &back).unwrap();
    assert_eq!(history["root"]["children"][0]["props"]["title"], old_name);
    // Delete Entry, then Delete All Entries preserving unrelated store keys.
    let delete = history["root"]["children"][0]["props"]["actionIds"][1]
        .as_str()
        .unwrap()
        .to_owned();
    let history = host.act("hist-2", &delete).unwrap();
    assert_eq!(history["root"]["children"].as_array().unwrap().len(), 3);
    assert!(store(d)["palettes"].get(&old_name).is_none());
    let clear = action(&history, "Delete All Entries", 0);
    let empty = host.act("hist-2", &clear).unwrap();
    assert_eq!(empty["root"]["children"], json!([]));
    let saved = store(d);
    assert_eq!(saved["palettes"], json!({}));
    assert_eq!(saved["_nativeStoreImported"], true);

    // Dispose invalidates the session; unknown operations are rejected.
    assert!(host
        .request(
            "session.call",
            json!({"sessionId":"gen","operation":"extension:frobnicate","payload":{}})
        )
        .is_err());
    host.request("session.dispose", json!({"sessionId":"gen"}))
        .unwrap();
    assert!(host.act("gen", &back).is_err());
    assert!(host.open("x", "not-a-command", json!({})).is_err());
    host.shutdown();
}

#[test]
fn ungranted_clipboard_is_refused_without_reverse_request() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, _) = Host::spawn(dir.path());
    host.initialize(dir.path(), json!([]), json!({})).unwrap();
    let view = host.open("s", GEN, json!({"color":"#123456"})).unwrap();
    let copy = action(&view, "Copy Shade", 0);
    let err = host.act("s", &copy).unwrap_err();
    assert!(err.contains("not granted"), "{err}");
    assert!(host.calls.is_empty());
    // No migration path was granted, so no import marker is written.
    assert!(store(dir.path()).get("_nativeStoreImported").is_none());
    host.shutdown();
}

#[test]
fn cancelled_copy_drops_reverse_request_and_keeps_action_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let (mut host, _) = Host::spawn(dir.path());
    host.initialize(dir.path(), json!(["clipboard.write"]), json!({}))
        .unwrap();
    let view = host.open("s", GEN, json!({"color":"#82123d"})).unwrap();
    let copy = action(&view, "Copy Shade", 2);
    let id = hid();
    host.send(json!({"type":"request","id":id,"method":"session.call",
        "params":{"sessionId":"s","operation":"extension:invoke-action","payload":{"actionId":copy}}}));
    let reverse = host.next();
    assert_eq!(reverse["type"], "request");
    assert_eq!(reverse["method"], "host.clipboard.write");
    host.send(json!({"type":"cancel","requestId":id,"reason":"user closed view"}));
    let mut saw_cancel = false;
    let response = loop {
        let frame = host.next();
        match frame["type"].as_str() {
            Some("cancel") => {
                assert_eq!(frame["requestId"], reverse["id"]);
                saw_cancel = true;
            }
            Some("response") => break frame,
            _ => panic!("unexpected {frame}"),
        }
    };
    assert_eq!(response["id"], id.as_str());
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("cancel"));
    // A late host reply for the cancelled reverse request is tolerated.
    host.send(json!({"type":"response","id":reverse["id"],"result":{"ok":true}}));
    assert!(saw_cancel, "SDK cancels the dropped reverse request");
    // The same action ID still works after cancellation (state not advanced).
    assert_eq!(host.act("s", &copy).unwrap()["mode"], "unchanged");
    assert_eq!(
        host.calls.last().unwrap().1["text"],
        view["root"]["children"][2]["props"]["subtitle"]
    );
    host.shutdown();
}

#[test]
fn corrupt_migration_store_fails_initialize_without_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy.json");
    std::fs::write(&legacy, b"{broken").unwrap();
    let (mut host, _) = Host::spawn(dir.path());
    let err = host
        .initialize(
            dir.path(),
            json!(["clipboard.write"]),
            json!({"nativeStore":legacy}),
        )
        .unwrap_err();
    assert!(err.contains("migration"), "{err}");
    assert_eq!(std::fs::read(&legacy).unwrap(), b"{broken");
    assert!(!dir.path().join("data/color-shades/storage.json").exists());
    // Sessions stay unavailable; EOF ends the process cleanly.
    assert!(host.open("s", GEN, json!({"color":"#abc"})).is_err());
    let Host {
        mut child, stdin, ..
    } = host;
    drop(stdin);
    assert!(wait(&mut child).success());
}
