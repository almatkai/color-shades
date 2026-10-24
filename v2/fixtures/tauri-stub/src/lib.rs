//! Headless test double for the two Tauri symbols used by unchanged beta common.rs.
#[derive(Clone)]
pub struct AppHandle;
pub trait Emitter {
    fn emit<S: serde::Serialize + Clone>(&self, channel: &str, payload: S) -> Result<(), String>;
}
impl Emitter for AppHandle {
    fn emit<S: serde::Serialize + Clone>(&self, _: &str, _: S) -> Result<(), String> {
        Err("Tauri UI is unavailable in headless tests".into())
    }
}
