//! Probes the audio backends and prints the current model.
//!
//! Run with:
//! `cargo run -p nosh-core --example audio`

#[cfg(feature = "audio-pulse")]
use nosh_core::PulseBackend;
use nosh_core::{AudioBackend, AudioModel, WpctlBackend};

fn print_model(name: &str, model: &AudioModel) {
    println!("{name} backend:");
    println!(
        "  default sink: {:?}",
        model
            .default_sink
            .as_ref()
            .map(|sink| sink.description.clone())
    );
    println!(
        "  default source: {:?}",
        model
            .default_source
            .as_ref()
            .map(|source| source.description.clone())
    );
    for sink in &model.sinks {
        println!(
            "  sink {} {:?} volume={:.2} muted={} default={} icon={}",
            sink.id, sink.description, sink.volume, sink.muted, sink.is_default, sink.icon_name
        );
    }
    for source in &model.sources {
        println!(
            "  source {} {:?} volume={:.2} muted={} default={} icon={}",
            source.id,
            source.description,
            source.volume,
            source.muted,
            source.is_default,
            source.icon_name
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "audio-pulse")]
    {
        let pulse = PulseBackend::new();
        match pulse.probe().await {
            Ok(capabilities) => {
                println!("pulse available: {capabilities:?}");
                match pulse.read().await {
                    Ok(model) => print_model("pulse", &model),
                    Err(error) => eprintln!("pulse read failed: {error}"),
                }
            }
            Err(error) => println!("pulse unavailable: {error}"),
        }
    }
    #[cfg(not(feature = "audio-pulse"))]
    println!("pulse backend was not built (enable the audio-pulse feature)");

    let wpctl = WpctlBackend::new();
    match wpctl.probe().await {
        Ok(capabilities) => {
            println!("wpctl available: {capabilities:?}");
            match wpctl.read().await {
                Ok(model) => print_model("wpctl", &model),
                Err(error) => eprintln!("wpctl read failed: {error}"),
            }
        }
        Err(error) => println!("wpctl unavailable: {error}"),
    }

    Ok(())
}
