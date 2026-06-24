// Many model fields/types are consumed by the TUI plan; allow during build-up.
#![allow(dead_code)]

mod model;
mod sources;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("plaza: core scaffold (use --search <term> once implemented)");
    Ok(())
}
