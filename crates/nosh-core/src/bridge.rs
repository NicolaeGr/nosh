use std::thread;

use async_channel::{Receiver, Sender};

use crate::command::Command;
use crate::error::CoreError;
use crate::event::{Event, MonitorEvent, Snapshot, WorkspaceEvent};
use crate::monitors::service::{MonitorService, MonitorServiceConfig};
use crate::workspaces::service::{WorkspaceService, WorkspaceServiceConfig};

#[derive(Debug, Clone, Default)]
pub struct CoreSettings {
    pub workspaces: WorkspaceServiceConfig,
    pub monitors: MonitorServiceConfig,
}

/// Handle to the core thread.
///
/// The core runs on its own Tokio runtime. The UI subscribes to events before
/// creating windows, sends commands, and never blocks waiting for a reply.
pub struct Core {
    commands: Sender<Command>,
    events: Receiver<Event>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Core {
    pub fn start() -> Result<Self, CoreError> {
        Self::start_with(CoreSettings::default())
    }

    pub fn start_with(settings: CoreSettings) -> Result<Self, CoreError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("nosh-core")
            .build()?;

        let (commands, public_commands) = async_channel::unbounded();
        let (event_sender, events) = async_channel::unbounded();
        let (service_events, service_event_receiver) = async_channel::unbounded();
        let (workspace_commands, workspace_receiver) = async_channel::unbounded();
        let (monitor_commands, monitor_receiver) = async_channel::unbounded();

        let thread = thread::Builder::new()
            .name("nosh-core".to_string())
            .spawn(move || {
                runtime.block_on(async move {
                    let router = tokio::spawn(route_commands(
                        public_commands,
                        workspace_commands,
                        monitor_commands,
                    ));
                    let merger = tokio::spawn(merge_events(service_event_receiver, event_sender));

                    let workspaces = WorkspaceService::from_env_with(settings.workspaces)
                        .run(workspace_receiver, service_events.clone());
                    let monitors = MonitorService::from_env_with(settings.monitors)
                        .run(monitor_receiver, service_events.clone());
                    // Both service futures own a clone. Dropping the original
                    // lets the merger finish once both services return.
                    drop(service_events);

                    tokio::join!(workspaces, monitors);
                    router.abort();
                    let _ = merger.await;
                });
            })?;

        Ok(Self {
            commands,
            events,
            thread: Some(thread),
        })
    }

    pub fn subscribe(&self) -> Receiver<Event> {
        self.events.clone()
    }

    /// Queues a command without blocking. The core channel is unbounded, so a
    /// failure means the core has stopped.
    pub fn send(&self, command: Command) {
        if let Err(error) = self.commands.try_send(command) {
            tracing::warn!(%error, "core command was not delivered");
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.commands.close();
        self.events.close();

        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::warn!("the core thread panicked");
            }
        }
    }
}

/// Routes each public command to the service that owns it.
///
/// The services each need exclusive access to their command receiver, so the
/// public channel is fanned out to two service channels.
async fn route_commands(
    commands: Receiver<Command>,
    workspace_commands: Sender<Command>,
    monitor_commands: Sender<Command>,
) {
    while let Ok(command) = commands.recv().await {
        let sender = if command.is_monitor() {
            &monitor_commands
        } else {
            &workspace_commands
        };

        if sender.send(command).await.is_err() {
            tracing::debug!("a service command channel is closed");
        }
    }
}

/// Merges the two services' partial snapshots into one accumulator and
/// forwards every event in order.
///
/// Typed updates refresh the accumulator, so the next full snapshot carries
/// the latest state from both services. A snapshot field is only replaced when
/// the incoming value is `Some`; a partial snapshot never erases the other
/// service.
async fn merge_events(events: Receiver<Event>, public: Sender<Event>) {
    let mut snapshot = Snapshot::default();

    while let Ok(event) = events.recv().await {
        let event = match event {
            Event::Snapshot(partial) => {
                merge_snapshot(&mut snapshot, partial);
                Event::Snapshot(snapshot.clone())
            }
            event => {
                absorb_event(&mut snapshot, &event);
                event
            }
        };

        if public.send(event).await.is_err() {
            return;
        }
    }
}

fn merge_snapshot(accumulator: &mut Snapshot, incoming: Snapshot) {
    if incoming.workspaces.is_some() {
        accumulator.workspaces = incoming.workspaces;
    }
    if incoming.workspace_backend.is_some() {
        accumulator.workspace_backend = incoming.workspace_backend;
    }
    if incoming.monitors.is_some() {
        accumulator.monitors = incoming.monitors;
    }
    if incoming.monitor_backend.is_some() {
        accumulator.monitor_backend = incoming.monitor_backend;
    }
}

fn absorb_event(accumulator: &mut Snapshot, event: &Event) {
    match event {
        Event::Workspaces(WorkspaceEvent::Snapshot { state, backend }) => {
            accumulator.workspaces = Some(state.clone());
            accumulator.workspace_backend = Some(*backend);
        }
        Event::Monitors(MonitorEvent::Snapshot { layout, backend }) => {
            accumulator.monitors = Some(layout.clone());
            accumulator.monitor_backend = Some(*backend);
        }
        Event::Monitors(MonitorEvent::LayoutChanged(layout)) => {
            accumulator.monitors = Some(layout.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::event::Service;
    use crate::model::WorkspaceSnapshot;
    use crate::monitors::LayoutPreset;
    use crate::monitors::MonitorLayout;

    #[tokio::test]
    async fn commands_are_routed_to_the_matching_service() {
        let (public_tx, public_rx) = async_channel::unbounded();
        let (workspace_tx, workspace_rx) = async_channel::unbounded();
        let (monitor_tx, monitor_rx) = async_channel::unbounded();

        let task = tokio::spawn(route_commands(public_rx, workspace_tx, monitor_tx));

        public_tx
            .send(Command::WorkspaceActivate("1".to_string()))
            .await
            .unwrap();
        public_tx
            .send(Command::MonitorSetPrimary("DP-1".to_string()))
            .await
            .unwrap();
        public_tx
            .send(Command::MonitorMode {
                target: "1".to_string(),
                mode: LayoutPreset::Extend,
            })
            .await
            .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), workspace_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::WorkspaceActivate("1".to_string())
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), monitor_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::MonitorSetPrimary("DP-1".to_string())
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), monitor_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::MonitorMode {
                target: "1".to_string(),
                mode: LayoutPreset::Extend,
            }
        );
        assert!(workspace_rx.try_recv().is_err());

        drop(public_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn two_partial_snapshots_merge_into_one_full_snapshot() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        let workspaces = WorkspaceSnapshot {
            focused_workspace: Some("1".to_string()),
            ..WorkspaceSnapshot::default()
        };
        service_tx
            .send(Event::Snapshot(Snapshot {
                workspaces: Some(workspaces),
                workspace_backend: Some("hyprland"),
                ..Snapshot::default()
            }))
            .await
            .unwrap();

        let first = public_rx.recv().await.unwrap();
        match first {
            Event::Snapshot(snapshot) => {
                assert!(snapshot.workspaces.is_some());
                assert_eq!(snapshot.workspace_backend, Some("hyprland"));
                assert!(snapshot.monitors.is_none());
                assert!(snapshot.monitor_backend.is_none());
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        let monitors = MonitorLayout {
            primary: Some("DP-1".to_string()),
            ..MonitorLayout::default()
        };
        service_tx
            .send(Event::Snapshot(Snapshot {
                monitors: Some(monitors),
                monitor_backend: Some("hyprland"),
                ..Snapshot::default()
            }))
            .await
            .unwrap();

        let second = public_rx.recv().await.unwrap();
        match second {
            Event::Snapshot(snapshot) => {
                assert_eq!(
                    snapshot
                        .workspaces
                        .expect("workspace state was not merged")
                        .focused_workspace
                        .as_deref(),
                    Some("1")
                );
                assert_eq!(snapshot.workspace_backend, Some("hyprland"));
                assert_eq!(
                    snapshot
                        .monitors
                        .expect("monitor state was not merged")
                        .primary
                        .as_deref(),
                    Some("DP-1")
                );
                assert_eq!(snapshot.monitor_backend, Some("hyprland"));
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn typed_updates_are_reflected_in_the_next_full_snapshot() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        let workspaces = WorkspaceSnapshot {
            focused_workspace: Some("3".to_string()),
            ..WorkspaceSnapshot::default()
        };
        service_tx
            .send(Event::Workspaces(WorkspaceEvent::Snapshot {
                state: workspaces,
                backend: "hyprland",
            }))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Workspaces(WorkspaceEvent::Snapshot { .. })
        ));

        let first_monitor_layout = MonitorLayout {
            primary: Some("DP-1".to_string()),
            ..MonitorLayout::default()
        };
        service_tx
            .send(Event::Monitors(MonitorEvent::Snapshot {
                layout: first_monitor_layout,
                backend: "hyprland",
            }))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Monitors(MonitorEvent::Snapshot { .. })
        ));

        let updated_monitor_layout = MonitorLayout {
            primary: Some("DP-2".to_string()),
            ..MonitorLayout::default()
        };
        service_tx
            .send(Event::Monitors(MonitorEvent::LayoutChanged(
                updated_monitor_layout,
            )))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Monitors(MonitorEvent::LayoutChanged(_))
        ));

        service_tx
            .send(Event::Snapshot(Snapshot::default()))
            .await
            .unwrap();
        let event = public_rx.recv().await.unwrap();
        match event {
            Event::Snapshot(snapshot) => {
                assert_eq!(
                    snapshot
                        .workspaces
                        .expect("workspace update was not absorbed")
                        .focused_workspace
                        .as_deref(),
                    Some("3")
                );
                assert_eq!(snapshot.workspace_backend, Some("hyprland"));
                assert_eq!(
                    snapshot
                        .monitors
                        .expect("monitor update was not absorbed")
                        .primary
                        .as_deref(),
                    Some("DP-2")
                );
                assert_eq!(snapshot.monitor_backend, Some("hyprland"));
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn event_order_is_preserved() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        service_tx
            .send(Event::Workspaces(WorkspaceEvent::FocusedChanged {
                workspace: Some("1".to_string()),
            }))
            .await
            .unwrap();
        service_tx
            .send(Event::Monitors(MonitorEvent::ApplyFailed(
                "rejected".to_string(),
            )))
            .await
            .unwrap();
        service_tx
            .send(Event::ServiceAvailability(Service::Workspaces, true))
            .await
            .unwrap();
        service_tx
            .send(Event::Snapshot(Snapshot::default()))
            .await
            .unwrap();

        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Workspaces(WorkspaceEvent::FocusedChanged { .. })
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Monitors(MonitorEvent::ApplyFailed(_))
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::ServiceAvailability(Service::Workspaces, true)
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Snapshot(_)
        ));

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn the_merger_exits_after_both_service_senders_drop() {
        let (first_service, service_rx) = async_channel::unbounded();
        let second_service = first_service.clone();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        drop(first_service);
        second_service
            .send(Event::ServiceAvailability(Service::Monitors, true))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::ServiceAvailability(Service::Monitors, true)
        ));
        drop(second_service);

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}
