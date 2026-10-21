//! Prints the live Hyprland monitor layout and the placement choice for each
//! notification corner.
//!
//! Run with:
//! `cargo run -p nosh-core --example monitors`
//!
//! This example is read-only. It exits cleanly when Hyprland is not the
//! running compositor.

use nosh_core::hyprland::HyprlandMonitorBackend;
use nosh_core::monitors::{score, select_monitor, Corner, MonitorBackend, MonitorPolicy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(backend) = HyprlandMonitorBackend::detect()? else {
        eprintln!(
            "HYPRLAND_INSTANCE_SIGNATURE is not set; Hyprland is not the running compositor."
        );
        return Ok(());
    };

    println!(
        "command socket: {}",
        backend.paths().command_socket.display()
    );
    println!("event socket: {}", backend.paths().event_socket.display());

    let capabilities = backend.probe().await?;
    println!("capabilities: {capabilities:?}");

    let layout = backend.read().await?;
    println!("primary: {:?}", layout.primary);
    println!("monitors:");

    for monitor in &layout.monitors {
        println!(
            "  {} id={} {}x{}{} at {:+}x{:+} scale={} transform={:?} refresh={:.2} enabled={} focused={} primary={} mirror_of={:?}",
            monitor.connector(),
            monitor.id.compositor_id,
            monitor.width,
            monitor.height,
            if monitor.refresh_rate > 0.0 {
                format!("@{:.2}", monitor.refresh_rate)
            } else {
                String::new()
            },
            monitor.x,
            monitor.y,
            monitor.scale,
            monitor.transform,
            monitor.refresh_rate,
            monitor.enabled,
            monitor.focused,
            monitor.primary,
            monitor.mirror_of,
        );
    }

    println!("placement (farthest policy):");
    for corner in Corner::ALL {
        match select_monitor(&layout, corner, &MonitorPolicy::Farthest) {
            Some(monitor) => println!(
                "  {}: {} (score {})",
                corner.as_str(),
                monitor.connector(),
                score(monitor, corner)
            ),
            None => println!("  {}: none", corner.as_str()),
        }
    }

    Ok(())
}
