//! The polkit authentication agent for graphical privilege prompts.

pub mod agent;
pub mod backend;
pub mod helper;
pub mod model;
pub mod service;

pub use agent::{
    PolkitAgentBackend, POLKIT_AGENT_INTERFACE, POLKIT_AGENT_PATH, POLKIT_AUTHORITY_INTERFACE,
    POLKIT_AUTHORITY_PATH, POLKIT_SERVICE,
};
pub use backend::{PolkitBackend, PolkitBackendRef};
pub use helper::{HelperMessage, HelperSession, HELPER_SOCKET};
pub use model::{AgentRequest, Identity, PolkitEvent, PolkitPrompt};
pub use service::PolkitService;
