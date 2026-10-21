//! Reads the current Hyprland workspace state and prints it.
//!
//! Run with:
//! `cargo run -p nosh-core --example probe`
//! It exits cleanly when Hyprland is not the running compositor.

use nosh_core::hyprland::HyprlandWorkspaceBackend;
use nosh_core::WorkspaceBackend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(backend) = HyprlandWorkspaceBackend::detect()? else {
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

    let state = backend.read().await?;
    println!("focused workspace: {:?}", state.focused_workspace);

    for monitor in &state.monitors {
        println!(
            "monitor {} {} focused={}",
            monitor.id, monitor.connector, monitor.focused
        );
    }
    for workspace in &state.workspaces {
        println!(
            "workspace {} name={} monitor={:?} focused={} urgent={}",
            workspace.id, workspace.name, workspace.monitor, workspace.focused, workspace.urgent
        );
    }

    Ok(())
}
