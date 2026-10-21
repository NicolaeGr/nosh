use std::thread;

use async_channel::{Receiver, Sender};

use crate::command::Command;
use crate::error::CoreError;
use crate::event::Event;
use crate::workspaces::service::{WorkspaceService, WorkspaceServiceConfig};

#[derive(Debug, Clone, Default)]
pub struct CoreSettings {
    pub workspaces: WorkspaceServiceConfig,
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

        let (commands, command_receiver) = async_channel::unbounded();
        let (event_sender, events) = async_channel::unbounded();

        let thread = thread::Builder::new()
            .name("nosh-core".to_string())
            .spawn(move || {
                runtime.block_on(async move {
                    WorkspaceService::from_env_with(settings.workspaces)
                        .run(command_receiver, event_sender)
                        .await;
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
