use std::sync::Arc;
use std::time::Duration;

use nosh_core::hyprland::{HyprlandClient, HyprlandWorkspaceBackend};
use nosh_core::paths::HyprlandPaths;
use nosh_core::{BackendError, WorkspaceBackend, WorkspaceBackendEvent};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

fn socket_path(directory: &TempDir, name: &str) -> std::path::PathBuf {
    directory.path().join(name)
}

fn paths(directory: &TempDir) -> HyprlandPaths {
    HyprlandPaths {
        command_socket: socket_path(directory, "command.sock"),
        event_socket: socket_path(directory, "event.sock"),
    }
}

/// Serves one request per connection, matching Hyprland 0.56.
async fn serve_commands(
    listener: UnixListener,
    requests: Arc<Mutex<Vec<String>>>,
    expected_requests: usize,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 4096];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);

            let request = String::from_utf8_lossy(&buffer[..read]).trim().to_string();
            requests.lock().await.push(request.clone());

            let response = match request.as_str() {
                "j/version" => {
                    r#"{"tag":"v0.56.2","commit":"abc","branch":"main","flags":["nix"]}"#
                }
                "j/status" => r#"{"configProvider":"lua"}"#,
                "j/monitors" => {
                    r#"[{"id":0,"name":"DP-1","width":2560,"height":1440,"refreshRate":144.0,"x":0,"y":0,"scale":1.0,"transform":0,"focused":true,"dpmsStatus":true,"disabled":false}]"#
                }
                "j/workspaces" => {
                    r#"[{"id":1,"name":"1","monitorID":0,"monitor":"DP-1","hasfullscreen":false,"windows":1},{"id":2,"name":"2","monitorID":0,"hasfullscreen":false,"windows":0}]"#
                }
                "j/clients" => {
                    r#"[{"address":"0xabc","workspace":{"id":2,"name":"2","monitorID":0},"class":"kitty","title":"Terminal"}]"#
                }
                "j/activeworkspace" => r#"{"id":1,"name":"1","monitorID":0}"#,
                "dispatch hl.dsp.focus({ workspace = \"3\" })" => "ok",
                other => panic!("unexpected request: {other}"),
            };

            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(&[0x04]).await.unwrap();
            stream.flush().await.unwrap();
        }
    })
}

async fn command_listener(
    paths: &HyprlandPaths,
    requests: Arc<Mutex<Vec<String>>>,
    expected_requests: usize,
) -> JoinHandle<()> {
    let listener = UnixListener::bind(&paths.command_socket).unwrap();
    serve_commands(listener, requests, expected_requests).await
}

#[tokio::test]
async fn queries_stop_at_the_terminator() {
    let directory = tempfile::tempdir().unwrap();
    let path = socket_path(&directory, "command.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 64];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..read], b"j/version");

        stream.write_all(b"payload").await.unwrap();
        stream.write_all(&[0x04]).await.unwrap();
        stream.write_all(b"ignored").await.unwrap();
        stream.flush().await.unwrap();
    });

    let client = HyprlandClient::new(path);
    let response = client.query("j/version").await.unwrap();
    assert_eq!(response, "payload");

    server.await.unwrap();
}

#[tokio::test]
async fn each_request_opens_its_own_connection() {
    let directory = tempfile::tempdir().unwrap();
    let path = socket_path(&directory, "command.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        for response in ["first", "second"] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 64];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            drop(stream);
        }
    });

    let client = HyprlandClient::new(path);
    assert_eq!(client.query("j/version").await.unwrap(), "first");
    assert_eq!(client.query("j/monitors").await.unwrap(), "second");

    server.await.unwrap();
}

#[tokio::test]
async fn responses_end_at_eof_when_there_is_no_terminator() {
    let directory = tempfile::tempdir().unwrap();
    let path = socket_path(&directory, "command.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 64];
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        stream.write_all(b"{\"tag\":\"v0.56.2\"}").await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);
    });

    let client = HyprlandClient::new(path);
    assert_eq!(
        client.query("j/version").await.unwrap(),
        "{\"tag\":\"v0.56.2\"}"
    );

    server.await.unwrap();
}

#[tokio::test]
async fn an_empty_response_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = socket_path(&directory, "command.sock");
    let listener = UnixListener::bind(&path).unwrap();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 64];
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        drop(stream);
    });

    let client = HyprlandClient::new(path);
    let error = client.query("j/version").await.unwrap_err();
    assert!(matches!(error, BackendError::Request(_)));

    server.await.unwrap();
}

#[tokio::test]
async fn a_missing_command_socket_is_a_clean_error() {
    let directory = tempfile::tempdir().unwrap();
    let client = HyprlandClient::new(socket_path(&directory, "missing.sock"));

    let error = client.query("j/version").await.unwrap_err();
    assert!(matches!(error, BackendError::Unavailable(_)));
}

#[tokio::test]
async fn probe_reads_the_provider_and_activation_uses_lua_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server = command_listener(&paths, requests.clone(), 3).await;
    let backend = HyprlandWorkspaceBackend::new(paths);

    let capabilities = backend.probe().await.unwrap();
    assert!(capabilities.contains(nosh_core::WorkspaceCapabilities::READ));
    assert!(capabilities.contains(nosh_core::WorkspaceCapabilities::ACTIVATE));

    backend.activate("3").await.unwrap();

    let requests = requests.lock().await;
    assert!(requests
        .iter()
        .any(|request| request == "dispatch hl.dsp.focus({ workspace = \"3\" })"));

    server.await.unwrap();
}

#[tokio::test]
async fn read_builds_workspaces_from_the_command_socket() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server = command_listener(&paths, requests, 4).await;
    let backend = HyprlandWorkspaceBackend::new(paths);

    let state = backend.read().await.unwrap();

    assert_eq!(state.focused_workspace.as_deref(), Some("1"));
    assert_eq!(state.workspaces.len(), 2);
    assert_eq!(state.workspaces[0].id, "1");
    assert_eq!(state.workspaces[0].monitor.as_deref(), Some("DP-1"));
    assert!(state.workspaces[0].focused);
    assert_eq!(state.monitors.len(), 1);

    server.await.unwrap();
}

#[tokio::test]
async fn watch_reads_events_and_returns_on_eof() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let command_server = command_listener(&paths, requests, 4).await;
    let event_listener = UnixListener::bind(&paths.event_socket).unwrap();

    let backend = HyprlandWorkspaceBackend::new(paths);
    let (event_tx, event_rx) = async_channel::unbounded();
    let watch = tokio::spawn(async move { backend.watch(event_tx).await });

    let (mut stream, _) = event_listener.accept().await.unwrap();
    stream.write_all(b"workspacev2>>1,1\n").await.unwrap();
    stream.flush().await.unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    match event {
        WorkspaceBackendEvent::Snapshot(state) => {
            assert_eq!(state.focused_workspace.as_deref(), Some("1"));
            assert_eq!(state.workspaces.len(), 2);
        }
        other => panic!("expected a snapshot, got {other:?}"),
    }

    drop(stream);

    let error = tokio::time::timeout(Duration::from_secs(2), watch)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, BackendError::Unavailable(_)));

    command_server.await.unwrap();
}

#[tokio::test]
async fn watch_on_a_missing_event_socket_is_a_clean_error() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let backend = HyprlandWorkspaceBackend::new(paths);
    let (event_tx, _event_rx) = async_channel::unbounded();

    let error = backend.watch(event_tx).await.unwrap_err();
    assert!(matches!(error, BackendError::Unavailable(_)));
}

#[tokio::test]
async fn urgent_marks_the_clients_workspace_when_it_is_not_focused() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let command_server = command_listener(&paths, requests, 4).await;
    let event_listener = UnixListener::bind(&paths.event_socket).unwrap();

    let backend = HyprlandWorkspaceBackend::new(paths);
    backend.read().await.unwrap();

    let (event_tx, event_rx) = async_channel::unbounded();
    let watch = tokio::spawn(async move { backend.watch(event_tx).await });

    let (mut stream, _) = event_listener.accept().await.unwrap();
    stream.write_all(b"urgent>>0xabc\n").await.unwrap();
    stream.flush().await.unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        WorkspaceBackendEvent::UrgentChanged {
            workspace,
            urgent: true
        } if workspace == "2"
    ));

    drop(stream);
    let _ = watch.await;
    command_server.await.unwrap();
}
