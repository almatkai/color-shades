//! Compile unchanged as a sibling of Tezbar's real v2_sdk; see README integration.
use super::v2_sdk as sdk;
use crate::native_core::common::{CoreContext, CoreResult};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tezbar_color_shades::{Generator, Palette, LEVELS};

const SLUG: &str = "color-shades";
const KEY: &str = "palettes";
static EPOCH: AtomicU64 = AtomicU64::new(1);
// The source module's two static arrays are shared by all generations. Hold this
// lane through persistence so effects and sorted hue state commit in call order.
static GENERATOR: tokio::sync::Mutex<Option<Generator>> = tokio::sync::Mutex::const_new(None);

pub fn commands() -> &'static [&'static str] {
    &["generate-color-shades", "view-color-shades"]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Committed {
    revision: u64,
    base: String,
    before: Generator,
    palette: Palette,
}
impl Committed {
    fn recompute(&self) -> Result<Palette, String> {
        if self.revision == 0
            || chrono::DateTime::parse_from_rfc3339(&self.palette.creation_date).is_err()
        {
            return Err("Invalid palette date".into());
        }
        let p = self
            .before
            .clone()
            .generate(&self.base, &self.palette.creation_date)?;
        if p != self.palette {
            return Err("Corrupt native palette".into());
        }
        Ok(p)
    }
}
type Entries = BTreeMap<String, Committed>;
fn entries(store: &Value) -> Result<Entries, String> {
    let result: Entries = match store.get(KEY) {
        None => BTreeMap::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("Invalid native palettes: {e}"))?,
    };
    for (name, p) in &result {
        if name != &p.recompute()?.name {
            return Err("Corrupt palette key".into());
        }
    }
    Ok(result)
}
#[derive(Clone, Serialize, Deserialize)]
enum Screen {
    Draft,
    History(Entries),
    Palette(Box<Committed>),
}
#[derive(Clone, Serialize, Deserialize)]
struct Session {
    command: String,
    epoch: u64,
    draft: String,
    screen: Screen,
    back: Vec<Screen>,
}
fn bump(s: &mut Session) {
    s.epoch = EPOCH.fetch_add(1, Ordering::Relaxed);
}
#[derive(Clone)]
enum Intent {
    Shade(u16),
    Base(u16),
    Variables,
    Json,
    View(String),
    Delete(String),
    Clear,
    Back,
    Submit,
}
struct Action {
    wire: Value,
    intent: Intent,
}
fn action(s: &Session, suffix: &str, title: &str, intent: Intent) -> Action {
    Action {
        wire: json!({"id":format!("color-shades:{}:{suffix}",s.epoch),"title":title,"kind":"action"}),
        intent,
    }
}
fn actions(s: &Session) -> Vec<Action> {
    let mut out = vec![];
    match &s.screen {
        Screen::Draft => {
            let mut a = action(s, "submit", "Generate Color Shades", Intent::Submit);
            a.wire["kind"] = json!("submit-form");
            out.push(a);
        }
        Screen::Palette(_) => {
            for level in LEVELS {
                let mut a = action(
                    s,
                    &format!("copy:{level}"),
                    "Copy Shade",
                    Intent::Shade(level),
                );
                a.wire["kind"] = json!("copy");
                out.push(a);
                out.push(action(
                    s,
                    &format!("base:{level}"),
                    "Use as Base Color",
                    Intent::Base(level),
                ));
                let mut a = action(
                    s,
                    &format!("variables:{level}"),
                    "Copy All Colors as Variables",
                    Intent::Variables,
                );
                a.wire["kind"] = json!("copy");
                a.wire["shortcut"] = json!({"modifiers":["ctrl"],"key":"c"});
                out.push(a);
                let mut a = action(
                    s,
                    &format!("json:{level}"),
                    "Copy All Colors as JSON",
                    Intent::Json,
                );
                a.wire["kind"] = json!("copy");
                a.wire["shortcut"] = json!({"modifiers":["ctrl","opt"],"key":"c"});
                out.push(a);
            }
        }
        Screen::History(items) => {
            for (i, name) in items.keys().enumerate() {
                out.push(action(
                    s,
                    &format!("view:{i}"),
                    "View Palette",
                    Intent::View(name.clone()),
                ));
                let mut a = action(
                    s,
                    &format!("delete:{i}"),
                    "Delete Entry",
                    Intent::Delete(name.clone()),
                );
                a.wire["style"] = json!("destructive");
                a.wire["shortcut"] = json!({"modifiers":["ctrl"],"key":"x"});
                out.push(a);
                let mut a = action(
                    s,
                    &format!("clear:{i}"),
                    "Delete All Entries",
                    Intent::Clear,
                );
                a.wire["style"] = json!("destructive");
                a.wire["shortcut"] = json!({"modifiers":["ctrl","shift"],"key":"x"});
                out.push(a);
            }
        }
    }
    if !s.back.is_empty() {
        out.push(action(s, "back", "Go Back", Intent::Back));
    }
    out
}
fn render(s: &Session) -> CoreResult {
    let actions = actions(s);
    let wire: Vec<_> = actions.iter().map(|a| a.wire.clone()).collect();
    let ids: Vec<_> = wire.iter().map(|a| a["id"].clone()).collect();
    let (title, root) = match &s.screen {
        Screen::Draft => (
            "Generate Color Shades".into(),
            json!({"type":"Form","children":[{"type":"Form.TextField","props":{"id":"color","title":"Color","placeholder":"#82123d","value":s.draft}}]}),
        ),
        Screen::Palette(c) => {
            let p = c.recompute()?;
            let children:Vec<_>=LEVELS.iter().enumerate().map(|(i,level)|json!({"type":"Grid.Item","props":{"id":level.to_string(),"title":level.to_string(),"subtitle":p.colors[level],"content":{"color":{"light":p.colors[level],"dark":p.colors[level],"adjustContrast":false}},"actionIds":ids[i*4..i*4+4]}})).collect();
            (
                p.name.clone(),
                json!({"type":"Grid","props":{"columns":11,"navigationTitle":p.name},"children":children}),
            )
        }
        Screen::History(items) => {
            let mut ordered: Vec<_> = items.iter().enumerate().collect();
            // Source comparator reverses ties; stable native ordering by name is
            // deliberate for equal millisecond timestamps (documented).
            ordered.sort_by(|(_, (_, a)), (_, (_, b))| {
                b.palette.creation_date.cmp(&a.palette.creation_date)
            });
            let children:Vec<_>=ordered.into_iter().map(|(i,(name,c))|json!({"type":"List.Item","props":{"id":name,"title":name,"icon":{"source":"CircleFilled","tintColor":c.palette.colors[&500],"tooltip":c.palette.colors[&500]},"accessories":[{"date":c.palette.creation_date,"tooltip":"Created at"}],"actionIds":ids[i*3..i*3+3]}})).collect();
            (
                "View Color Shades".into(),
                json!({"type":"List","children":children}),
            )
        }
    };
    let mut root = root;
    // beta index.tsx routes header Back through this supported action ID.
    // Also expose it in each row's action panel.
    if !s.back.is_empty() {
        root["props"]["backActionId"] = wire.last().unwrap()["id"].clone();
        if let Some(children) = root["children"].as_array_mut() {
            for child in children {
                if let Some(ids) = child["props"]["actionIds"].as_array_mut() {
                    ids.push(wire.last().unwrap()["id"].clone());
                }
            }
        }
    }
    Ok(sdk::view(&title, root, wire))
}
fn session(state: &Value, command: &str) -> Result<Session, String> {
    let s: Session =
        serde_json::from_value(state.clone()).map_err(|_| "No active Color Shades session")?;
    if s.command != command {
        return Err("Command does not match session".into());
    }
    Ok(s)
}
fn commit_state(state: &mut Value, s: &Session) -> CoreResult {
    let view = render(s)?;
    *state = serde_json::to_value(s).map_err(|e| e.to_string())?;
    Ok(view)
}
async fn generate(
    ctx: &CoreContext,
    base: String,
    expected: Option<(String, Committed)>,
) -> Result<Committed, String> {
    let mut lane = GENERATOR.lock().await;
    let before = lane.get_or_insert_with(Generator::default).clone();
    let mut next = before.clone();
    let date = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let palette = next.generate(&base, &date)?;
    let c = Committed {
        revision: EPOCH.fetch_add(1, Ordering::Relaxed),
        base,
        before,
        palette,
    };
    let saved = c.clone();
    sdk::update_store(ctx, SLUG, move |store| {
        let mut data = entries(store)?;
        if let Some((name, old)) = expected {
            if data.get(&name) != Some(&old) {
                return Err("Stale history action; reopen history".into());
            }
        }
        data.insert(saved.palette.name.clone(), saved);
        store[KEY] = serde_json::to_value(data).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await?;
    *lane = Some(next);
    Ok(c)
}

async fn copy(ctx: &CoreContext, text: &str) -> Result<(), String> {
    let reply = sdk::copy(ctx, text).await?;
    if reply["ok"] != true
        || reply["success"] == false
        || reply.get("error").is_some_and(|e| !e.is_null())
    {
        return Err(reply["error"]
            .as_str()
            .or_else(|| reply["message"].as_str())
            .unwrap_or("Clipboard write was not acknowledged")
            .into());
    }
    Ok(())
}

pub async fn handle(
    ctx: &CoreContext,
    command: &str,
    operation: &str,
    payload: &Value,
    state: &mut Value,
) -> CoreResult {
    if !commands().contains(&command) {
        return Err("Unknown Color Shades command".into());
    }
    if !matches!(
        operation,
        "extension:run-command"
            | "extension:invoke-action"
            | "extension:search-text-changed"
            | "extension:refresh-session"
            | "extension:load-more"
            | "extension:dispose-session"
    ) {
        return Err("Unsupported Color Shades operation".into());
    }
    if operation == "extension:dispose-session" {
        *state = Value::Null;
        return Ok(sdk::unchanged());
    }
    if operation == "extension:run-command" {
        // Invalidate actions from any prior launch, including when a new launch fails.
        *state = Value::Null;
        let mut s = Session {
            command: command.into(),
            epoch: 0,
            draft: String::new(),
            screen: Screen::Draft,
            back: vec![],
        };
        bump(&mut s);
        if command == "view-color-shades" {
            s.screen = Screen::History(entries(&sdk::read_store(ctx, SLUG).await?)?);
        } else if let Some(base) = payload["argumentValues"]["color"].as_str() {
            s.draft = base.into();
            s.back.push(Screen::Draft);
            s.screen = Screen::Palette(Box::new(generate(ctx, base.into(), None).await?));
        }
        return commit_state(state, &s);
    }
    let mut s = session(state, command)?;
    match operation {
        "extension:search-text-changed" => {
            // Search is local in Grid/List; never invalidate copy/menu IDs on a keystroke.
            if matches!(s.screen, Screen::Draft) {
                if let Some(v) = payload["formValues"]["color"].as_str() {
                    s.draft = v.into();
                }
                commit_state(state, &s)
            } else {
                Ok(sdk::unchanged())
            }
        }
        "extension:load-more" => Ok(sdk::unchanged()),
        "extension:refresh-session" => {
            // Beta refresh uses a cloned state and discards mutations. Changing
            // IDs here would issue actions the committed session cannot accept.
            Ok(sdk::unchanged())
        }
        "extension:invoke-action" => {
            let id = payload["actionId"].as_str().ok_or("Missing action ID")?;
            let intent = actions(&s)
                .into_iter()
                .find(|a| a.wire["id"] == id)
                .ok_or("Stale or unknown Color Shades action")?
                .intent;
            match intent {
                Intent::Shade(level) | Intent::Base(level) => {
                    let Screen::Palette(c) = &s.screen else {
                        return Err("Palette required".into());
                    };
                    let p = c.recompute()?;
                    let text = p.colors[&level].clone();
                    if matches!(intent, Intent::Shade(_)) {
                        copy(ctx, &text).await?;
                        return Ok(sdk::unchanged());
                    }
                    let c = generate(ctx, text, None).await?;
                    s.screen = Screen::Palette(Box::new(c));
                }
                Intent::Variables | Intent::Json => {
                    let Screen::Palette(c) = &s.screen else {
                        return Err("Palette required".into());
                    };
                    let p = c.recompute()?;
                    let text = if matches!(intent, Intent::Variables) {
                        p.variables()?
                    } else {
                        p.json()?
                    };
                    copy(ctx, &text).await?;
                    return Ok(sdk::unchanged());
                }
                Intent::Submit => {
                    let base = payload["formValues"]["color"]
                        .as_str()
                        .ok_or("Color is required")?;
                    // Preserve draft even when validation/storage fails; action ID stays retryable.
                    s.draft = base.into();
                    *state = serde_json::to_value(&s).map_err(|e| e.to_string())?;
                    let c = generate(ctx, base.into(), None).await?;
                    s.back.push(Screen::Draft);
                    s.screen = Screen::Palette(Box::new(c));
                }
                Intent::View(name) => {
                    let Screen::History(data) = &s.screen else {
                        return Err("History required".into());
                    };
                    let old = data.get(&name).ok_or("Missing palette")?.clone();
                    let base = old.recompute()?.colors[&500].clone();
                    let c = generate(ctx, base, Some((name, old))).await?;
                    s.back.push(s.screen.clone());
                    s.screen = Screen::Palette(Box::new(c));
                }
                Intent::Delete(name) => {
                    let Screen::History(data) = &s.screen else {
                        return Err("History required".into());
                    };
                    let old = data.get(&name).ok_or("Missing palette")?.clone();
                    let store = sdk::update_store(ctx, SLUG, move |store| {
                        let mut data = entries(store)?;
                        if data.get(&name) != Some(&old) {
                            return Err("Stale history action; reopen history".into());
                        }
                        data.remove(&name);
                        store[KEY] = serde_json::to_value(data).map_err(|e| e.to_string())?;
                        Ok(())
                    })
                    .await?;
                    s.screen = Screen::History(entries(&store)?);
                }
                Intent::Clear => {
                    let Screen::History(old) = &s.screen else {
                        return Err("History required".into());
                    };
                    let old = old.clone();
                    let store = sdk::update_store(ctx, SLUG, move |store| {
                        if entries(store)? != old {
                            return Err("Stale history action; reopen history".into());
                        }
                        store[KEY] = json!({});
                        Ok(())
                    })
                    .await?;
                    s.screen = Screen::History(entries(&store)?);
                }
                Intent::Back => {
                    s.screen = s.back.pop().ok_or("No previous screen")?;
                    if matches!(s.screen, Screen::History(_)) {
                        s.screen = Screen::History(entries(&sdk::read_store(ctx, SLUG).await?)?);
                    }
                }
            }
            bump(&mut s);
            commit_state(state, &s)
        }
        _ => unreachable!("validated operation"),
    }
}

#[cfg(test)]
#[path = "../tests/adapter.rs"]
mod tests;
