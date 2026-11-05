//! Narrow replacement for the compiled-in beta `v2_sdk` module. Only the helpers
//! the Color Shades adapter calls exist; each maps to one typed SDK surface.
use crate::native_core::common::{CoreContext, CoreResult};
use serde_json::Value;

/// Private store name below the host-provided extension data directory.
pub const STORE: &str = "color-shades";

// `copy` maps to host.clipboard.write {text} and checks clipboard.write grants.
pub use tezbar_extension_sdk::{copy, unchanged, view};

/// The adapter passes its legacy slug; the process package owns one store.
fn store(slug: &str) -> Result<&'static str, String> {
    if slug == STORE {
        Ok(STORE)
    } else {
        Err("Unknown Color Shades store".into())
    }
}

pub async fn read_store(ctx: &CoreContext, slug: &str) -> CoreResult {
    tezbar_extension_sdk::read_store(ctx, store(slug)?).await
}

pub async fn update_store<F>(ctx: &CoreContext, slug: &str, change: F) -> CoreResult
where
    F: FnOnce(&mut Value) -> Result<(), String> + Send + 'static,
{
    tezbar_extension_sdk::update_store(ctx, store(slug)?, change).await
}
