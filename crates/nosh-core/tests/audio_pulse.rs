//! Live checks against a running Pulse or pipewire-pulse server.
//!
//! These are ignored by default because they need an audio session. Each write
//! repeats the value the server already has, so the test does not change the
//! user's volume, mute, or default device.

#![cfg(feature = "audio-pulse")]

use nosh_core::{AudioBackend, EndpointKind, PulseBackend};

#[tokio::test]
#[ignore = "requires a running Pulse or pipewire-pulse server"]
async fn pulse_reads_devices_and_repeats_the_current_values() {
    let backend = PulseBackend::new();
    backend
        .probe()
        .await
        .expect("the Pulse server should be available");

    let before = backend.read().await.expect("a Pulse read should succeed");
    assert!(!before.sinks.is_empty(), "no sinks were found");

    let sink = before
        .default_sink
        .clone()
        .expect("there should be a default sink");
    assert!(sink.volume <= 1.5);

    backend
        .set_volume(EndpointKind::Sink, sink.id, sink.volume)
        .await
        .expect("setting the current volume should succeed");
    backend
        .set_muted(EndpointKind::Sink, sink.id, sink.muted)
        .await
        .expect("repeating the current mute state should succeed");
    backend
        .set_default(EndpointKind::Sink, sink.id)
        .await
        .expect("re-selecting the current default sink should succeed");

    let after = backend
        .read()
        .await
        .expect("a second Pulse read should work");
    let same = after
        .default_sink
        .expect("the default sink should still exist");
    assert_eq!(same.id, sink.id);
    assert!((same.volume - sink.volume).abs() < 0.01);
    assert_eq!(same.muted, sink.muted);
}
