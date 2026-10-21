//! Prints core events until interrupted.
//!
//! Run with:
//! `cargo run -p nosh-core --example watch`

use nosh_core::Core;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let core = Core::start()?;
    let events = core.subscribe();

    while let Ok(event) = events.recv().await {
        println!("{event:?}");
    }

    Ok(())
}
