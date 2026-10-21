use std::sync::Arc;

use crate::backends::BackendRef;
use crate::error::BackendError;
use crate::model::WorkspaceCapabilities;

type BackendFactory = Arc<dyn Fn() -> Result<BackendRef, BackendError> + Send + Sync>;

#[derive(Clone)]
pub struct Candidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl Candidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> Result<BackendRef, BackendError> + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> Result<BackendRef, BackendError> {
        (self.make)()
    }
}

pub struct Resolved {
    pub backend: BackendRef,
    pub name: &'static str,
    pub capabilities: WorkspaceCapabilities,
    /// Position in the candidate list. Zero is the most specific backend.
    pub priority: usize,
}

/// Tries candidates from most specific to most generic.
///
/// A candidate is selected only when its probe succeeds and it provides every
/// required capability. Probing must have no side effects.
pub async fn resolve(
    feature: &'static str,
    candidates: Vec<Candidate>,
    required: WorkspaceCapabilities,
) -> Result<Resolved, BackendError> {
    for (priority, candidate) in candidates.into_iter().enumerate() {
        let backend = match candidate.create() {
            Ok(backend) => backend,
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "backend constructor failed"
                );
                continue;
            }
        };

        match backend.probe().await {
            Ok(capabilities) if capabilities.contains(required) => {
                tracing::info!(
                    feature,
                    backend = candidate.name,
                    ?capabilities,
                    "selected backend"
                );
                return Ok(Resolved {
                    backend,
                    name: candidate.name,
                    capabilities,
                    priority,
                });
            }
            Ok(capabilities) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    ?capabilities,
                    "backend lacks a required capability"
                );
            }
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "backend probe failed"
                );
            }
        }
    }

    tracing::debug!(feature, "no backend satisfies the required capabilities");
    Err(BackendError::NoBackend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::test_support::FakeWorkspaceBackend;

    fn candidate(name: &'static str, backend: Arc<FakeWorkspaceBackend>) -> Candidate {
        Candidate::new(name, move || Ok(backend.clone()))
    }

    #[tokio::test]
    async fn the_most_specific_backend_wins() {
        let specific = FakeWorkspaceBackend::new(
            "specific",
            WorkspaceCapabilities::READ | WorkspaceCapabilities::WATCH,
        )
        .arc();
        let generic = FakeWorkspaceBackend::new(
            "generic",
            WorkspaceCapabilities::READ | WorkspaceCapabilities::WATCH,
        )
        .arc();

        let resolved = resolve(
            "workspaces",
            vec![
                candidate("specific", specific),
                candidate("generic", generic),
            ],
            WorkspaceCapabilities::BAR,
        )
        .await
        .unwrap();

        assert_eq!(resolved.name, "specific");
        assert_eq!(resolved.priority, 0);
    }

    #[tokio::test]
    async fn a_failed_probe_falls_through_to_the_next_candidate() {
        let mut specific = FakeWorkspaceBackend::new(
            "specific",
            WorkspaceCapabilities::READ | WorkspaceCapabilities::WATCH,
        );
        specific.probe_result = Err(BackendError::Unavailable("no compositor".into()));
        let specific = specific.arc();
        let generic = FakeWorkspaceBackend::new(
            "generic",
            WorkspaceCapabilities::READ | WorkspaceCapabilities::WATCH,
        )
        .arc();

        let resolved = resolve(
            "workspaces",
            vec![
                candidate("specific", specific),
                candidate("generic", generic),
            ],
            WorkspaceCapabilities::BAR,
        )
        .await
        .unwrap();

        assert_eq!(resolved.name, "generic");
        assert_eq!(resolved.priority, 1);
    }

    #[tokio::test]
    async fn a_missing_capability_falls_through() {
        let read_only = FakeWorkspaceBackend::new("read-only", WorkspaceCapabilities::READ).arc();
        let generic = FakeWorkspaceBackend::new(
            "generic",
            WorkspaceCapabilities::READ | WorkspaceCapabilities::WATCH,
        )
        .arc();

        let resolved = resolve(
            "workspaces",
            vec![
                candidate("read-only", read_only),
                candidate("generic", generic),
            ],
            WorkspaceCapabilities::BAR,
        )
        .await
        .unwrap();

        assert_eq!(resolved.name, "generic");
        assert_eq!(resolved.priority, 1);
    }

    #[tokio::test]
    async fn no_candidate_is_a_clean_error() {
        let result = resolve(
            "workspaces",
            vec![
                Candidate::new("broken", || {
                    Err(BackendError::Unavailable("constructor failed".into()))
                }),
                Candidate::new("read-only", || {
                    Ok(FakeWorkspaceBackend::new("read-only", WorkspaceCapabilities::READ).arc())
                }),
            ],
            WorkspaceCapabilities::BAR,
        )
        .await;

        assert!(matches!(result, Err(BackendError::NoBackend)));
    }
}
