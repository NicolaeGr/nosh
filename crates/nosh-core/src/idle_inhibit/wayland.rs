//! The Wayland idle inhibitor backend.
//!
//! The connection, the event queue, and the protocol objects are owned by a
//! dedicated thread. The thread binds `wl_compositor` and
//! `zwp_idle_inhibit_manager_v1`, creates one surface for the lifetime of the
//! process, and creates or destroys the inhibitor on request. A lost connection
//! is reported to the service, which reconnects and reapplies the desired state.
//!
//! The surface has no buffer. It exists only as the target of the inhibitor.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread;

use async_channel::{Receiver, Sender};
use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::{wl_compositor, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1::{self, ZwpIdleInhibitManagerV1},
    zwp_idle_inhibitor_v1::{self, ZwpIdleInhibitorV1},
};

use crate::error::BackendError;
use crate::idle_inhibit::backend::{
    IdleInhibitBackend, InhibitBackendEvent, InhibitCommand, InhibitSession,
};

const COMPOSITOR: &str = "wl_compositor";
const IDLE_INHIBIT_MANAGER: &str = "zwp_idle_inhibit_manager_v1";

/// Connects directly to the Wayland socket in `WAYLAND_DISPLAY`.
#[derive(Debug, Default)]
pub struct WaylandBackend;

impl WaylandBackend {
    pub fn new() -> Self {
        Self
    }
}

impl IdleInhibitBackend for WaylandBackend {
    fn name(&self) -> &'static str {
        "wayland"
    }

    fn connect(&self, initial_active: bool) -> Result<Box<dyn InhibitSession>, BackendError> {
        let connection = Connection::connect_to_env().map_err(|error| {
            BackendError::Unavailable(format!("cannot connect to the Wayland display: {error}"))
        })?;
        let (wake_reader, wake_writer) = UnixStream::pair().map_err(|error| {
            BackendError::Unavailable(format!("cannot create the inhibitor wake pipe: {error}"))
        })?;

        let (commands, command_receiver) = async_channel::unbounded();
        let (event_sender, events) = async_channel::unbounded();
        thread::Builder::new()
            .name("nosh-idle-inhibit".to_string())
            .spawn(move || {
                if let Err(error) = run(
                    connection,
                    initial_active,
                    command_receiver,
                    wake_reader,
                    event_sender.clone(),
                ) {
                    tracing::debug!(%error, "the Wayland idle inhibitor connection ended");
                    let _ = event_sender.try_send(InhibitBackendEvent::Lost(error));
                }
            })
            .map_err(|error| {
                BackendError::Unavailable(format!("cannot start the inhibitor thread: {error}"))
            })?;

        Ok(Box::new(WaylandSession {
            commands,
            wake: Arc::new(wake_writer),
            events,
        }))
    }
}

struct WaylandSession {
    commands: Sender<InhibitCommand>,
    wake: Arc<UnixStream>,
    events: Receiver<InhibitBackendEvent>,
}

impl InhibitSession for WaylandSession {
    fn set_active(&self, active: bool) -> Result<(), BackendError> {
        self.commands
            .try_send(InhibitCommand::Set(active))
            .map_err(|_| {
                BackendError::Unavailable("the idle inhibitor connection is closed".to_string())
            })?;
        wake(&self.wake);
        Ok(())
    }

    fn events(&self) -> Receiver<InhibitBackendEvent> {
        self.events.clone()
    }
}

impl Drop for WaylandSession {
    fn drop(&mut self) {
        self.commands.close();
        wake(&self.wake);
    }
}

fn wake(stream: &UnixStream) {
    // A full pipe already means the worker has not slept since the last wake.
    let mut stream = stream;
    let _ = stream.write(&[1]);
}

/// Runs the Wayland event loop until the connection is lost or the service
/// closes the command channel.
fn run(
    connection: Connection,
    initial_active: bool,
    commands: Receiver<InhibitCommand>,
    mut wake_reader: UnixStream,
    events: Sender<InhibitBackendEvent>,
) -> Result<(), String> {
    let (globals, mut queue) =
        registry_queue_init::<WaylandState>(&connection).map_err(|error| error.to_string())?;
    let queue_handle = queue.handle();
    let mut state = WaylandState::default();

    state.sync(&globals, &queue_handle);
    queue
        .roundtrip(&mut state)
        .map_err(|error| format!("the initial Wayland roundtrip failed: {error}"))?;
    state.sync(&globals, &queue_handle);

    let mut available = state.available();
    let mut active = initial_active && available;
    state
        .apply(active, &queue_handle, &connection)
        .map_err(|error| error.to_string())?;

    if events
        .try_send(InhibitBackendEvent::Connected { available, active })
        .is_err()
    {
        return Ok(());
    }

    let wayland_fd = connection.backend().poll_fd().as_raw_fd();
    let wake_fd = wake_reader.as_raw_fd();
    let mut buffer = [0u8; 64];

    loop {
        while let Ok(InhibitCommand::Set(requested)) = commands.try_recv() {
            let requested = requested && available;
            if requested == active {
                continue;
            }
            active = requested;
            state
                .apply(active, &queue_handle, &connection)
                .map_err(|error| error.to_string())?;
            if events
                .try_send(InhibitBackendEvent::ActiveChanged(active))
                .is_err()
            {
                return Ok(());
            }
        }
        if commands.is_closed() {
            return Ok(());
        }

        if let Some(guard) = connection.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(error))
                    if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(format!("the Wayland connection failed: {error}")),
            }
        }
        queue
            .dispatch_pending(&mut state)
            .map_err(|error| format!("Wayland event dispatch failed: {error}"))?;

        if state.globals_changed {
            state.sync(&globals, &queue_handle);
            connection
                .flush()
                .map_err(|error| format!("the Wayland connection failed: {error}"))?;
        }

        let now_available = state.available();
        if now_available != available {
            if !now_available && active {
                active = false;
                state
                    .apply(false, &queue_handle, &connection)
                    .map_err(|error| error.to_string())?;
                if events
                    .try_send(InhibitBackendEvent::ActiveChanged(false))
                    .is_err()
                {
                    return Ok(());
                }
            }
            available = now_available;
            if events
                .try_send(InhibitBackendEvent::Availability(available))
                .is_err()
            {
                return Ok(());
            }
        }

        let mut fds = [
            libc::pollfd {
                fd: wayland_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("polling the Wayland connection failed: {error}"));
        }
        if fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("the compositor closed the Wayland connection".to_string());
        }
        if fds[1].revents & libc::POLLIN != 0 {
            let _ = wake_reader.read(&mut buffer);
        }
    }
}

/// Protocol objects owned by the inhibitor connection.
#[derive(Default)]
struct WaylandState {
    compositor: Option<wl_compositor::WlCompositor>,
    surface: Option<wl_surface::WlSurface>,
    manager: Option<ZwpIdleInhibitManagerV1>,
    inhibitor: Option<ZwpIdleInhibitorV1>,
    globals_changed: bool,
}

impl WaylandState {
    /// Binds the globals that are present and lets go of those that are gone.
    fn sync(&mut self, globals: &GlobalList, queue_handle: &QueueHandle<Self>) {
        self.globals_changed = false;
        let has_compositor = has_global(globals, COMPOSITOR);
        let has_manager = has_global(globals, IDLE_INHIBIT_MANAGER);

        if !has_compositor {
            self.inhibitor = None;
            self.surface = None;
            self.compositor = None;
        } else if self.compositor.is_none() {
            self.compositor = globals.bind(queue_handle, 1..=1, ()).ok();
        }

        if self.surface.is_none() {
            if let Some(compositor) = &self.compositor {
                let surface = compositor.create_surface(queue_handle, ());
                surface.commit();
                self.surface = Some(surface);
            }
        }

        if !has_manager {
            self.inhibitor = None;
            self.manager = None;
        } else if self.manager.is_none() {
            self.manager = globals.bind(queue_handle, 1..=1, ()).ok();
        }
    }

    fn available(&self) -> bool {
        self.manager.is_some() && self.surface.is_some()
    }

    /// Creates or destroys the inhibitor and flushes the request. No roundtrip
    /// is needed: the compositor applies the request when it reads the socket.
    fn apply(
        &mut self,
        active: bool,
        queue_handle: &QueueHandle<Self>,
        connection: &Connection,
    ) -> Result<(), wayland_client::backend::WaylandError> {
        let mut changed = false;
        if active {
            if self.inhibitor.is_none() {
                if let (Some(manager), Some(surface)) = (&self.manager, &self.surface) {
                    self.inhibitor = Some(manager.create_inhibitor(surface, queue_handle, ()));
                    changed = true;
                }
            }
        } else if let Some(inhibitor) = self.inhibitor.take() {
            inhibitor.destroy();
            changed = true;
        }

        if changed {
            if let Some(surface) = &self.surface {
                surface.commit();
            }
        }
        connection.flush()
    }
}

fn has_global(globals: &GlobalList, interface: &str) -> bool {
    globals
        .contents()
        .with_list(|list| list.iter().any(|global| global.interface == interface))
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
        state.globals_changed = true;
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_compositor::WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        _event: wl_surface::Event,
        _data: &(),
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpIdleInhibitManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpIdleInhibitManagerV1,
        _event: zwp_idle_inhibit_manager_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpIdleInhibitorV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpIdleInhibitorV1,
        _event: zwp_idle_inhibitor_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
    }
}
