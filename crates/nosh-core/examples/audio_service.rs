//! Runs the Pulse audio service for one event and prints what it publishes.
//!
//! Run with:
//! `cargo run -p nosh-core --example audio_service`
//!
//! The volume step is zero, so the service writes the value the server already
//! has. Nothing in the user's audio session changes.

#[cfg(not(feature = "audio-pulse"))]
fn main() {
    println!("the Pulse backend was not built; enable the audio-pulse feature");
}

#[cfg(feature = "audio-pulse")]
mod enabled {
    use std::sync::Arc;

    use nosh_core::{
        AudioBackendRef, AudioCandidate, AudioService, AudioServiceConfig, ChangeOrigin, Command,
        Event, PulseBackend,
    };

    pub async fn run() {
        let pulse = AudioCandidate::new("pulse", || {
            Ok(Arc::new(PulseBackend::new()) as AudioBackendRef)
        });
        let service = AudioService::with_candidates(vec![pulse], AudioServiceConfig::default());
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let mut wrote = false;
        loop {
            let event = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                event_rx.recv(),
            )
            .await
            {
                Ok(Ok(event)) => event,
                _ => {
                    eprintln!("no audio event arrived");
                    break;
                }
            };

            match event {
                Event::Audio(nosh_core::AudioEvent::Snapshot(model)) => {
                    println!(
                        "sink: {:?} volume {:.2}",
                        model
                            .default_sink
                            .as_ref()
                            .map(|sink| sink.description.clone()),
                        model
                            .default_sink
                            .as_ref()
                            .map(|sink| sink.volume)
                            .unwrap_or(0.0)
                    );
                    if !wrote {
                        wrote = true;
                        command_tx
                            .send(Command::AudioStepVolume(0.0))
                            .await
                            .unwrap();
                    }
                }
                Event::Audio(nosh_core::AudioEvent::VolumeChanged { volume, origin, .. }) => {
                    println!(
                        "volume {:.2} origin {}",
                        volume,
                        match origin {
                            ChangeOrigin::User => "user",
                            ChangeOrigin::External => "external",
                            ChangeOrigin::Restore => "restore",
                            ChangeOrigin::Automatic => "automatic",
                        }
                    );
                    break;
                }
                Event::ServiceAvailability(service, available) => {
                    println!("{service:?} available: {available}");
                }
                Event::Audio(nosh_core::AudioEvent::BackendChanged { backend }) => {
                    println!("backend: {backend}");
                }
                _ => {}
            }
        }

        drop(command_tx);
        let _ = task.await;
    }
}

#[cfg(feature = "audio-pulse")]
#[tokio::main]
async fn main() {
    enabled::run().await;
}
