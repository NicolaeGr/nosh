use std::sync::Arc;
use std::time::Duration;

use nosh_core::hyprland::HyprlandMonitorBackend;
use nosh_core::monitors::{
    MonitorBackend, MonitorCapabilities, MonitorId, MonitorLayout, MonitorModel, Transform,
};
use nosh_core::{BackendError, MonitorBackendEvent};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

fn socket_path(directory: &TempDir, name: &str) -> std::path::PathBuf {
    directory.path().join(name)
}

fn paths(directory: &TempDir) -> nosh_core::HyprlandPaths {
    nosh_core::HyprlandPaths {
        command_socket: socket_path(directory, "command.sock"),
        event_socket: socket_path(directory, "event.sock"),
    }
}

fn monitor_json() -> String {
    r#"[
        {"id":0,"name":"DP-1","description":"AOC Q27","make":"AOC","model":"Q27","serial":"S1","width":2560,"height":1440,"refreshRate":144.0,"x":0,"y":0,"scale":1.0,"transform":0,"focused":true,"disabled":false,"mirrorOf":"none"},
        {"id":1,"name":"HDMI-A-1","description":"Dell U","make":"Dell","model":"U2720Q","serial":"S2","width":1920,"height":1080,"refreshRate":60.0,"x":2560,"y":0,"scale":1.0,"transform":1,"focused":false,"disabled":true,"mirrorOf":"none"}
    ]"#
    .to_string()
}

struct Server {
    requests: Arc<Mutex<Vec<String>>>,
    monitor_response: Arc<Mutex<String>>,
    config_provider: Arc<Mutex<String>>,
    write_response: Arc<Mutex<String>>,
    task: JoinHandle<()>,
}

impl Server {
    async fn spawn(paths: &nosh_core::HyprlandPaths) -> Self {
        let listener = UnixListener::bind(&paths.command_socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let monitor_response = Arc::new(Mutex::new(monitor_json()));
        let config_provider = Arc::new(Mutex::new("hyprlang".to_string()));
        let write_response = Arc::new(Mutex::new("ok".to_string()));

        let task = {
            let requests = requests.clone();
            let monitor_response = monitor_response.clone();
            let config_provider = config_provider.clone();
            let write_response = write_response.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buffer = vec![0u8; 8192];
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buffer[..read]).trim().to_string();
                    requests.lock().await.push(request.clone());

                    let response = match request.as_str() {
                        "j/version" => {
                            r#"{"tag":"v0.56.2","commit":"abc","branch":"main","flags":["nix"]}"#
                                .to_string()
                        }
                        "j/status" => {
                            format!(r#"{{"configProvider":"{}"}}"#, config_provider.lock().await)
                        }
                        "j/monitors" => monitor_response.lock().await.clone(),
                        request
                            if request.starts_with("keyword monitor ")
                                || request.starts_with("eval hl.monitor(") =>
                        {
                            write_response.lock().await.clone()
                        }
                        other => panic!("unexpected request: {other}"),
                    };

                    if !response.is_empty() {
                        stream.write_all(response.as_bytes()).await.unwrap();
                    }
                    stream.write_all(&[0x04]).await.unwrap();
                    stream.flush().await.unwrap();
                }
            })
        };

        Self {
            requests,
            monitor_response,
            config_provider,
            write_response,
            task,
        }
    }

    async fn requests(&self) -> Vec<String> {
        self.requests.lock().await.clone()
    }

    async fn keyword_requests(&self) -> Vec<String> {
        self.requests()
            .await
            .into_iter()
            .filter(|request| request.starts_with("keyword monitor "))
            .collect()
    }

    async fn lua_requests(&self) -> Vec<String> {
        self.requests()
            .await
            .into_iter()
            .filter(|request| request.starts_with("eval hl.monitor("))
            .collect()
    }

    async fn set_config_provider(&self, provider: &str) {
        *self.config_provider.lock().await = provider.to_string();
    }

    async fn set_write_response(&self, response: &str) {
        *self.write_response.lock().await = response.to_string();
    }

    async fn set_monitor_response(&self, response: String) {
        *self.monitor_response.lock().await = response;
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn monitor(connector: &str, id: i32, enabled: bool) -> MonitorModel {
    MonitorModel {
        id: MonitorId {
            compositor_id: id,
            connector: connector.to_string(),
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
        },
        enabled,
        focused: false,
        primary: false,
        x: id * 1920,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
        transform: Transform::Normal,
        refresh_rate: 60.0,
        mirror_of: None,
    }
}

#[tokio::test]
async fn read_maps_monitors_including_disabled_ones() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    let backend = HyprlandMonitorBackend::new(paths);

    let layout = backend.read().await.unwrap();

    assert_eq!(layout.monitors.len(), 2);
    assert_eq!(layout.monitors[0].connector(), "DP-1");
    assert!(layout.monitors[0].enabled);
    assert_eq!(layout.monitors[0].transform, Transform::Normal);
    assert_eq!(layout.monitors[1].connector(), "HDMI-A-1");
    assert!(!layout.monitors[1].enabled);
    assert_eq!(layout.monitors[1].transform, Transform::Rotate90);
    assert_eq!(layout.enabled().count(), 1);
    assert_eq!(layout.primary.as_deref(), Some("DP-1"));

    server.task.abort();
}

#[tokio::test]
async fn a_probe_advertises_read_watch_and_write() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    let backend = HyprlandMonitorBackend::new(paths);

    let capabilities = backend.probe().await.unwrap();

    assert!(capabilities.contains(MonitorCapabilities::READ));
    assert!(capabilities.contains(MonitorCapabilities::WATCH));
    assert!(capabilities.contains(MonitorCapabilities::WRITE));
    assert!(capabilities.contains(MonitorCapabilities::MIRROR));
    assert!(capabilities.contains(MonitorCapabilities::ENABLE));

    server.task.abort();
}

#[tokio::test]
async fn watch_resyncs_only_for_monitor_events_and_reports_eof() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    let event_listener = UnixListener::bind(&paths.event_socket).unwrap();
    let backend = HyprlandMonitorBackend::new(paths);
    let (event_tx, event_rx) = async_channel::unbounded();
    let watch = tokio::spawn(async move { backend.watch(event_tx).await });

    let (mut stream, _) = event_listener.accept().await.unwrap();

    stream.write_all(b"workspacev2>>1,1\n").await.unwrap();
    stream.flush().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), event_rx.recv())
            .await
            .is_err(),
        "an unrelated event must not resync"
    );

    for event in [
        "monitoraddedv2>>1,HDMI-A-1,Dell U\n",
        "focusedmon>>DP-1,1\n",
        "configreloaded>>\n",
        "monitorremoved>>HDMI-A-1\n",
    ] {
        stream.write_all(event.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();

        let layout = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let MonitorBackendEvent::Layout(layout) = layout;
        assert_eq!(layout.monitors.len(), 2);
    }

    drop(stream);
    let error = tokio::time::timeout(Duration::from_secs(2), watch)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, BackendError::Unavailable(_)));

    server.task.abort();
}

#[tokio::test]
async fn watch_on_a_missing_event_socket_is_a_clean_error() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let backend = HyprlandMonitorBackend::new(paths);
    let (event_tx, _event_rx) = async_channel::unbounded();

    let error = backend.watch(event_tx).await.unwrap_err();

    assert!(matches!(error, BackendError::Unavailable(_)));
}

#[tokio::test]
async fn apply_disables_first_then_writes_enabled_monitors() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    let backend = HyprlandMonitorBackend::new(paths);

    let mut layout = MonitorLayout {
        monitors: vec![
            monitor("DP-1", 0, true),
            monitor("HDMI-A-1", 1, false),
            monitor("DP-2", 2, true),
        ],
        primary: None,
    };
    layout.monitors[0].mirror_of = Some("DP-2".to_string());

    backend.apply(&layout).await.unwrap();

    let requests = server.keyword_requests().await;
    assert_eq!(
        requests,
        [
            "keyword monitor \"HDMI-A-1,disable\"".to_string(),
            "keyword monitor \"DP-1,1920x1080@60.00,0x0,1,mirror,DP-2\"".to_string(),
            "keyword monitor \"DP-2,1920x1080@60.00,3840x0,1\"".to_string(),
        ]
    );

    server.task.abort();
}

#[tokio::test]
async fn apply_writes_lua_monitor_calls_when_the_provider_is_lua() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    server.set_config_provider("lua").await;
    let backend = HyprlandMonitorBackend::new(paths);

    let mut layout = MonitorLayout {
        monitors: vec![
            monitor("DP-1", 0, true),
            monitor("HDMI-A-1", 1, false),
            monitor("DP-2", 2, true),
        ],
        primary: None,
    };
    layout.monitors[0].mirror_of = Some("DP-2".to_string());

    backend.apply(&layout).await.unwrap();

    let requests = server.lua_requests().await;
    assert_eq!(
        requests,
        [
            "eval hl.monitor({ output = \"HDMI-A-1\", disabled = true })".to_string(),
            "eval hl.monitor({ output = \"DP-1\", mode = \"1920x1080@60.00\", position = \"0x0\", scale = 1, mirror = \"DP-2\" })".to_string(),
            "eval hl.monitor({ output = \"DP-2\", mode = \"1920x1080@60.00\", position = \"3840x0\", scale = 1 })".to_string(),
        ]
    );

    server.task.abort();
}

#[tokio::test]
async fn a_rejected_write_is_a_typed_error() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    server
        .set_write_response("error: invalid monitor mode")
        .await;
    let backend = HyprlandMonitorBackend::new(paths);

    let layout = MonitorLayout {
        monitors: vec![monitor("DP-1", 0, true)],
        primary: None,
    };

    let error = backend.apply(&layout).await.unwrap_err();

    assert!(matches!(error, BackendError::Request(_)));
    assert!(error.to_string().contains("invalid monitor mode"));

    server.task.abort();
}

#[tokio::test]
async fn a_resync_updates_the_layout_from_the_socket() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let server = Server::spawn(&paths).await;
    let event_listener = UnixListener::bind(&paths.event_socket).unwrap();
    let backend = HyprlandMonitorBackend::new(paths);
    let (event_tx, event_rx) = async_channel::unbounded();
    let watch = tokio::spawn(async move { backend.watch(event_tx).await });

    let (mut stream, _) = event_listener.accept().await.unwrap();
    server
        .set_monitor_response(
            r#"[{"id":0,"name":"DP-1","focused":true,"disabled":false}]"#.to_string(),
        )
        .await;

    stream
        .write_all(b"monitoraddedv2>>0,DP-1,Monitor\n")
        .await
        .unwrap();
    stream.flush().await.unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let MonitorBackendEvent::Layout(layout) = event;
    assert_eq!(layout.monitors.len(), 1);
    assert_eq!(layout.monitors[0].connector(), "DP-1");

    drop(stream);
    let _ = watch.await;
    server.task.abort();
}
