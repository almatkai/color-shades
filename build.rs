// Marks the process-host compilation of the shared v2 adapter source. The v2
// crate's headless beta-fixture tests stay in v2/; this crate tests real pipes.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(tezbar_process_host)");
    println!("cargo::rustc-cfg=tezbar_process_host");
}
