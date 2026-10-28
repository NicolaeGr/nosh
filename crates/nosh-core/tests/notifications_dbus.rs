//! End-to-end daemon test on a private session bus.
//!
//! Run with:
//!
//! ```sh
//! dbus-run-session -- cargo test -p nosh-core --test notifications_dbus -- --ignored
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use nosh_core::notifications::{NOTIFICATION_INTERFACE, NOTIFICATION_PATH, NOTIFICATION_SERVICE};
use nosh_core::{
    Command, Event, NoshPaths, NotificationEvent, NotificationService, NotificationServiceConfig,
};
use zbus::zvariant::OwnedValue;

#[tokio::test]
#[ignore = "requires a private session bus"]
async fn the_daemon_answers_the_freedesktop_interface() {
    let temp = tempfile::tempdir().unwrap();
    let paths = NoshPaths {
        config_dir: temp.path().join("config"),
        data_dir: temp.path().join("data"),
        cache_dir: temp.path().join("cache"),
        state_dir: temp.path().join("state"),
        runtime_dir: None,
    };
    let service = NotificationService::from_env_with(NotificationServiceConfig {
        paths: Some(paths),
        ..NotificationServiceConfig::default()
    });
    let (commands, command_rx) = async_channel::unbounded();
    let (events, event_rx) = async_channel::unbounded();
    let handle = tokio::spawn(service.run(command_rx, events));

    let connection = zbus::Connection::session().await.unwrap();
    let proxy = zbus::Proxy::new(
        &connection,
        NOTIFICATION_SERVICE,
        NOTIFICATION_PATH,
        NOTIFICATION_INTERFACE,
    )
    .await
    .unwrap();

    wait_for_daemon(&proxy).await;

    let info = proxy
        .call_method("GetServerInformation", &())
        .await
        .unwrap()
        .body()
        .deserialize::<(String, String, String, String)>()
        .unwrap();
    assert_eq!(info.0, "nosh");
    assert_eq!(info.3, "1.2");

    let capabilities = proxy
        .call_method("GetCapabilities", &())
        .await
        .unwrap()
        .body()
        .deserialize::<Vec<String>>()
        .unwrap();
    assert!(capabilities.iter().any(|value| value == "actions"));
    assert!(capabilities.iter().any(|value| value == "body-markup"));

    let mut closed = proxy.receive_signal("NotificationClosed").await.unwrap();

    let id = proxy
        .call_method(
            "Notify",
            &(
                "integration",
                0u32,
                "",
                "hello from the daemon test",
                "<b>body</b>",
                vec!["default".to_string(), "Open".to_string()],
                HashMap::<String, OwnedValue>::new(),
                0i32,
            ),
        )
        .await
        .unwrap()
        .body()
        .deserialize::<u32>()
        .unwrap();
    assert_eq!(id, 1);

    let received = next_notification_event(&event_rx).await;
    match received {
        NotificationEvent::Received {
            notification,
            replaced,
            suppress_popup,
        } => {
            assert_eq!(notification.id, 1);
            assert_eq!(notification.summary, "hello from the daemon test");
            assert_eq!(notification.body, "<b>body</b>");
            assert_eq!(notification.actions.len(), 1);
            assert_eq!(notification.actions[0].id, "default");
            assert!(!replaced);
            assert!(!suppress_popup);
        }
        other => panic!("expected a received event, got {other:?}"),
    }

    commands
        .send(Command::NotificationDismiss(id))
        .await
        .unwrap();

    let message = tokio::time::timeout(Duration::from_secs(2), closed.next())
        .await
        .expect("the close signal timed out")
        .expect("the signal stream ended");
    let (closed_id, reason) = message.body().deserialize::<(u32, u32)>().unwrap();
    assert_eq!(closed_id, id);
    assert_eq!(reason, 2);

    commands.close();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

async fn wait_for_daemon(proxy: &zbus::Proxy<'_>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if proxy.call_method("GetServerInformation", &()).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the notification daemon did not appear");
}

async fn next_notification_event(events: &async_channel::Receiver<Event>) -> NotificationEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("timed out waiting for an event")
            .expect("the event channel closed");
        if let Event::Notifications(event) = event {
            if matches!(
                event,
                NotificationEvent::Snapshot(_)
                    | NotificationEvent::BackendChanged { .. }
                    | NotificationEvent::Unavailable
            ) {
                continue;
            }
            return event;
        }
    }
}
