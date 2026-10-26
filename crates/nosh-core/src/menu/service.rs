//! The menu service. It owns menu definitions, sources, and request state.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use tokio::task::JoinHandle;

use crate::command::Command;
use crate::event::Event;
use crate::menu::action::{expand_argv, spawn_detached};
use crate::menu::config::default_menus;
use crate::menu::model::{dedupe_ids, After, ItemState, MenuDef, MenuItem, SearchMode};
use crate::menu::query::filter_and_sort;
use crate::menu::source::{menu_source, MenuSource, SourceEnv, SourceError};
use crate::menu::usage::{self, Usage};
use crate::paths::NoshPaths;

/// Events the menu service publishes.
#[derive(Debug, Clone)]
pub enum MenuEvent {
    /// The menu exists and its definition is complete.
    Opened {
        id: String,
        menu: Box<MenuDef>,
    },
    /// A new result set. `generation` increases for every request, so the UI
    /// can drop a late answer.
    Results {
        id: String,
        query: String,
        generation: u64,
        items: Vec<MenuItem>,
    },
    /// The source failed. Previous results remain valid.
    Failed {
        id: String,
        message: String,
    },
    Closed {
        id: String,
    },
    ActionFailed {
        id: String,
        message: String,
    },
    ActionDone {
        id: String,
        after: After,
    },
}

#[derive(Debug, Clone, Default)]
pub struct MenuServiceConfig {
    pub menus: BTreeMap<String, MenuDef>,
    pub paths: Option<NoshPaths>,
}

pub struct MenuService {
    menus: BTreeMap<String, MenuDef>,
    env: Arc<SourceEnv>,
    sessions: HashMap<String, Session>,
}

struct Session {
    def: MenuDef,
    source: Arc<dyn MenuSource>,
    client_cache: Option<Vec<MenuItem>>,
    last_results: Vec<MenuItem>,
    results_generation: u64,
    query: String,
    generation: u64,
    task: Option<JoinHandle<()>>,
}

struct Completion {
    id: String,
    generation: u64,
    query: String,
    result: Result<Vec<MenuItem>, SourceError>,
}

impl MenuService {
    pub fn new(config: MenuServiceConfig) -> Self {
        let mut env = SourceEnv::from_env();
        if let Some(paths) = &config.paths {
            env.config_dir = paths.config_dir.clone();
            env.usage = Arc::new(Usage::load(paths.app_usage(), usage::now()));
        }

        let menus = if config.menus.is_empty() {
            default_menus().menus
        } else {
            config.menus
        };

        Self {
            menus,
            env: Arc::new(env),
            sessions: HashMap::new(),
        }
    }

    pub async fn run(mut self, commands: Receiver<Command>, events: Sender<Event>) {
        let (completions, completion_events) = async_channel::unbounded();

        loop {
            tokio::select! {
                command = commands.recv() => {
                    match command {
                        Ok(command) => self.handle(command, &events, &completions),
                        Err(_) => break,
                    }
                }
                completion = completion_events.recv() => {
                    match completion {
                        Ok(completion) => self.complete(completion, &events),
                        Err(_) => break,
                    }
                }
            }
        }
    }

    fn handle(
        &mut self,
        command: Command,
        events: &Sender<Event>,
        completions: &Sender<Completion>,
    ) {
        match command {
            Command::MenuOpen(id) => self.open(id, events, completions),
            Command::MenuQuery { id, query } => self.query(id, query, events, completions),
            Command::MenuActivate { id, index } => self.activate(id, index, events),
            Command::MenuReload(id) => self.reload(id, events, completions),
            Command::MenuClose(id) => self.close(id, events),
            _ => {}
        }
    }

    fn open(&mut self, id: String, events: &Sender<Event>, completions: &Sender<Completion>) {
        self.remove_session(&id);

        let Some(def) = self.menus.get(&id).cloned() else {
            emit(
                events,
                MenuEvent::Failed {
                    id,
                    message: "no such menu".to_string(),
                },
            );
            return;
        };

        let source = match menu_source(&def.source, def.resolved_template(), Arc::clone(&self.env))
        {
            Ok(source) => source,
            Err(error) => {
                emit(
                    events,
                    MenuEvent::Failed {
                        id,
                        message: error.to_string(),
                    },
                );
                return;
            }
        };

        let generation = 1;
        let task = spawn_task(
            &id,
            generation,
            String::new(),
            Arc::clone(&source),
            None,
            completions.clone(),
        );

        self.sessions.insert(
            id.clone(),
            Session {
                def: def.clone(),
                source,
                client_cache: None,
                last_results: Vec::new(),
                results_generation: 0,
                query: String::new(),
                generation,
                task: Some(task),
            },
        );

        emit(
            events,
            MenuEvent::Opened {
                id,
                menu: Box::new(def),
            },
        );
    }

    fn query(
        &mut self,
        id: String,
        query: String,
        events: &Sender<Event>,
        completions: &Sender<Completion>,
    ) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };

        session.query = query.clone();
        session.generation += 1;
        let generation = session.generation;

        match session.def.source.resolved_search() {
            SearchMode::Client => {
                let Some(cache) = session.client_cache.as_ref() else {
                    // The first load is still running. Its completion filters
                    // against the newest query.
                    return;
                };
                let limit = session.def.source.resolved_limit();
                let mut items = filter_and_sort(cache, &query);
                items.truncate(limit);
                session.last_results = items.clone();
                session.results_generation = generation;
                emit(
                    events,
                    MenuEvent::Results {
                        id,
                        query,
                        generation,
                        items,
                    },
                );
            }
            SearchMode::Source => {
                if let Some(task) = session.task.take() {
                    task.abort();
                }
                session.task = Some(spawn_task(
                    &id,
                    generation,
                    query,
                    Arc::clone(&session.source),
                    Some(session.def.source.resolved_debounce_ms()),
                    completions.clone(),
                ));
            }
        }
    }

    fn complete(&mut self, completion: Completion, events: &Sender<Event>) {
        let Some(session) = self.sessions.get_mut(&completion.id) else {
            return;
        };

        let mut items = match completion.result {
            Ok(items) => items,
            Err(error) => {
                emit(
                    events,
                    MenuEvent::Failed {
                        id: completion.id,
                        message: error.to_string(),
                    },
                );
                return;
            }
        };

        let limit = session.def.source.resolved_limit();
        items.truncate(limit);
        dedupe_ids(&mut items);

        let is_initial_client_load = session.def.source.resolved_search() == SearchMode::Client
            && completion.query.is_empty();

        if is_initial_client_load {
            session.client_cache = Some(items);
            let mut filtered = filter_and_sort(
                session.client_cache.as_ref().expect("cache was just set"),
                &session.query,
            );
            filtered.truncate(limit);
            session.last_results = filtered.clone();
            session.results_generation = session.generation;
            emit(
                events,
                MenuEvent::Results {
                    id: completion.id,
                    query: session.query.clone(),
                    generation: session.generation,
                    items: filtered,
                },
            );
            return;
        }

        if completion.generation == session.generation {
            session.last_results = items.clone();
            session.results_generation = completion.generation;
            emit(
                events,
                MenuEvent::Results {
                    id: completion.id,
                    query: completion.query,
                    generation: completion.generation,
                    items,
                },
            );
        }
    }

    fn reload(&mut self, id: String, _events: &Sender<Event>, completions: &Sender<Completion>) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };

        if let Some(task) = session.task.take() {
            task.abort();
        }
        session.generation += 1;
        session.client_cache = None;

        let generation = session.generation;
        let query = match session.def.source.resolved_search() {
            SearchMode::Client => String::new(),
            SearchMode::Source => session.query.clone(),
        };
        session.task = Some(spawn_task(
            &id,
            generation,
            query,
            Arc::clone(&session.source),
            None,
            completions.clone(),
        ));
    }

    fn activate(&mut self, id: String, index: usize, events: &Sender<Event>) {
        let Some(session) = self.sessions.get(&id) else {
            return;
        };
        let Some(item) = session.last_results.get(index).cloned() else {
            return;
        };

        // A query is still in flight. The visible list belongs to the previous
        // query, so the index would point at the wrong item.
        if session.results_generation != session.generation {
            return;
        }

        if let ItemState::Disabled { reason } = &item.state {
            emit(
                events,
                MenuEvent::ActionFailed {
                    id,
                    message: reason.clone(),
                },
            );
            return;
        }

        let Some(action) = session.def.action_for(&item) else {
            emit(
                events,
                MenuEvent::ActionFailed {
                    id,
                    message: "the item has no action".to_string(),
                },
            );
            return;
        };

        let argv = expand_argv(&action.command, &item, &session.query);
        let after = if action.keep_open {
            After::None
        } else {
            action.after
        };
        match spawn_detached(&argv) {
            Ok(()) => {
                session.source.record_use(&item);
                emit(events, MenuEvent::ActionDone { id, after })
            }
            Err(error) => emit(
                events,
                MenuEvent::ActionFailed {
                    id,
                    message: error.to_string(),
                },
            ),
        }
    }

    fn close(&mut self, id: String, events: &Sender<Event>) {
        self.remove_session(&id);
        emit(events, MenuEvent::Closed { id });
    }

    fn remove_session(&mut self, id: &str) -> bool {
        match self.sessions.remove(id) {
            Some(mut session) => {
                if let Some(task) = session.task.take() {
                    task.abort();
                }
                true
            }
            None => false,
        }
    }
}

fn spawn_task(
    id: &str,
    generation: u64,
    query: String,
    source: Arc<dyn MenuSource>,
    debounce_ms: Option<u64>,
    completions: Sender<Completion>,
) -> JoinHandle<()> {
    let id = id.to_string();
    tokio::spawn(async move {
        if let Some(debounce_ms) = debounce_ms {
            tokio::time::sleep(Duration::from_millis(debounce_ms)).await;
        }
        let result = source.query(&query).await;
        let _ = completions
            .send(Completion {
                id,
                generation,
                query,
                result,
            })
            .await;
    })
}

fn emit(events: &Sender<Event>, event: MenuEvent) {
    if events.try_send(Event::Menu(event)).is_err() {
        tracing::debug!("a menu event was not delivered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn static_menu() -> MenuDef {
        toml::from_str(
            r#"
            title = "Power"

            [view]
            type = "list"

            [source]
            type = "builtin"
            name = "static"
            search = "client"

            [[source.options.items]]
            id = "lock"
            name = "Lock"
            action = { command = ["true"] }

            [[source.options.items]]
            id = "reboot"
            name = "Reboot"
            action = { command = ["true"] }
            "#,
        )
        .unwrap()
    }

    fn service() -> MenuService {
        let mut menus = BTreeMap::new();
        menus.insert("power".to_string(), static_menu());
        MenuService::new(MenuServiceConfig { menus, paths: None })
    }

    #[tokio::test]
    async fn open_emits_the_definition_and_results() {
        let (command_sender, command_receiver) = async_channel::unbounded();
        let (event_sender, event_receiver) = async_channel::unbounded();

        tokio::spawn(service().run(command_receiver, event_sender));
        command_sender
            .send(Command::MenuOpen("power".to_string()))
            .await
            .unwrap();

        let opened = event_receiver.recv().await.unwrap();
        assert!(matches!(opened, Event::Menu(MenuEvent::Opened { .. })));

        let results = event_receiver.recv().await.unwrap();
        match results {
            Event::Menu(MenuEvent::Results { items, .. }) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].name, "Lock");
            }
            other => panic!("expected results, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn client_search_filters_without_another_source_call() {
        let (command_sender, command_receiver) = async_channel::unbounded();
        let (event_sender, event_receiver) = async_channel::unbounded();

        tokio::spawn(service().run(command_receiver, event_sender));
        command_sender
            .send(Command::MenuOpen("power".to_string()))
            .await
            .unwrap();
        let _ = event_receiver.recv().await.unwrap();
        let _ = event_receiver.recv().await.unwrap();

        command_sender
            .send(Command::MenuQuery {
                id: "power".to_string(),
                query: "reb".to_string(),
            })
            .await
            .unwrap();

        match event_receiver.recv().await.unwrap() {
            Event::Menu(MenuEvent::Results {
                items, generation, ..
            }) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].name, "Reboot");
                assert_eq!(generation, 2);
            }
            other => panic!("expected results, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_menus_fail_without_panicking() {
        let (command_sender, command_receiver) = async_channel::unbounded();
        let (event_sender, event_receiver) = async_channel::unbounded();

        tokio::spawn(service().run(command_receiver, event_sender));
        command_sender
            .send(Command::MenuOpen("nope".to_string()))
            .await
            .unwrap();

        assert!(matches!(
            event_receiver.recv().await.unwrap(),
            Event::Menu(MenuEvent::Failed { .. })
        ));
        command_sender
            .send(Command::MenuClose("nope".to_string()))
            .await
            .unwrap();
        assert!(matches!(
            event_receiver.recv().await.unwrap(),
            Event::Menu(MenuEvent::Closed { .. })
        ));
    }
}
