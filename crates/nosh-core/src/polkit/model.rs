//! Polkit authentication prompts.
//!
//! polkitd asks the registered agent to collect a password. The agent drives
//! polkit's privileged helper, which runs PAM and reports success to polkitd.

use async_channel::Sender;

use crate::auth::model::Secret;

/// One identity polkit offered for a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// `unix-user`, `unix-group`, or `unix-session`.
    pub kind: String,
    /// The user id when the identity is a `unix-user`.
    pub uid: Option<u32>,
}

impl Identity {
    pub fn unix_user(uid: u32) -> Self {
        Self {
            kind: "unix-user".to_string(),
            uid: Some(uid),
        }
    }
}

/// A question from polkitd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolkitPrompt {
    /// The opaque value that ties a helper session to polkitd. Never shown.
    pub cookie: String,
    pub action_id: String,
    pub message: String,
    pub icon_name: String,
    /// Extra action fields. Display only.
    pub details: Vec<(String, String)>,
}

/// The agent interface talks to the service through this enum.
///
/// `BeginAuthentication` stays open until `End` arrives, because polkitd reads
/// the authentication result when the method returns.
#[derive(Debug)]
pub enum AgentRequest {
    Begin {
        prompt: PolkitPrompt,
        identities: Vec<Identity>,
        /// Passwords from the UI. Closing the channel cancels the session.
        responses: Sender<Secret>,
    },
    End {
        cookie: String,
        success: bool,
        message: Option<String>,
    },
    Cancel {
        cookie: String,
    },
}

/// What the UI shows and hears about a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolkitEvent {
    /// A password is needed.
    Prompt(PolkitPrompt),
    /// The password was rejected. The session is over.
    Failed { cookie: String, message: String },
    /// The password was accepted and polkitd was told.
    Finished { cookie: String },
    /// polkitd or the user cancelled the prompt.
    Cancelled { cookie: String },
    /// polkitd is missing or another authentication agent already answered.
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_built_for_the_session_user_carries_the_uid() {
        let identity = Identity::unix_user(1000);
        assert_eq!(identity.kind, "unix-user");
        assert_eq!(identity.uid, Some(1000));
    }
}
