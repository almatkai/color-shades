//! Standalone Tezbar process executable (schema 3, protocol 1) for Color Shades.
//! The reviewed Native V2 adapter source is compiled unchanged in behavior beside
//! a narrow shim that maps its host calls onto typed SDK requests.
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
        Box::pin(color_shades::handle(
            ctx, command, operation, payload, state,
        ))
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
