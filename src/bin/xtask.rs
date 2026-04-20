#[path = "xtask/app.rs"]
mod app;
#[path = "xtask/cargo_ops.rs"]
mod cargo_ops;
#[path = "xtask/cmd.rs"]
mod cmd;
#[path = "xtask/host.rs"]
mod host;
#[path = "xtask/runtime.rs"]
mod runtime;
#[path = "xtask/signing.rs"]
mod signing;
#[path = "xtask/types.rs"]
mod types;

fn main() -> anyhow::Result<()> {
    app::run()
}
