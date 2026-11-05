//! Standalone Tezbar process executable (schema 3, protocol 1) for Color Shades.
//! The mature Native V2 engine/adapter is compiled beside a narrow typed SDK
//! shim. Only callback compatibility and exact JS snapshots extend the adapter.
mod migration;

/// Mirrors the module layout the v2 adapter was written against.
mod native_core {
    pub mod common {
        pub use tezbar_extension_sdk::{CoreResult, ExtensionContext as CoreContext};
    }
    pub mod extensions {
        #[path = "../../../v2/src/tezbar_adapter.rs"]
        pub mod color_shades;
        pub mod v2_sdk;
    }
}

use native_core::extensions::color_shades;
use serde_json::{json, Value};
use tezbar_extension_sdk::{BoxFuture, CoreResult, Extension, ExtensionContext};

pub const ID: &str = "com.tezbar.native.color-shades";

struct Adapter;
impl Extension for Adapter {
    fn id(&self) -> &'static str {
        ID
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn commands(&self) -> &'static [&'static str] {
        color_shades::commands()
    }
    fn initialize<'a>(&'a self, ctx: &'a ExtensionContext) -> BoxFuture<'a, CoreResult> {
        Box::pin(async move {
            migration::initialize(ctx).await?;
            Ok(json!({"ready":true}))
        })
    }
    fn handle<'a>(
        &'a self,
        ctx: &'a ExtensionContext,
        command: &'a str,
        operation: &'a str,
        payload: &'a Value,
        state: &'a mut Value,
    ) -> BoxFuture<'a, CoreResult> {
        Box::pin(async move {
            let mut result = color_shades::handle(ctx, command, operation, payload, state).await?;
            // Current renderer callbacks consume no-view without replacing the
            // mounted view. They do not handle unchanged (refresh/load-more do).
            // Keep every other field, error, state and issued action ID intact.
            if result["ok"] == true
                && result["mode"] == "unchanged"
                && matches!(
                    operation,
                    "extension:invoke-action" | "extension:search-text-changed"
                )
            {
                result["mode"] = json!("no-view");
            }
            Ok(result)
        })
    }
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("extension runtime");
    let result = runtime.block_on(tezbar_extension_sdk::serve(Adapter));
    runtime.shutdown_timeout(std::time::Duration::from_millis(500));
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
