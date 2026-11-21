//! Coordinates installs for tools sharing the same audio.cpp runtime directory.
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use acpp_server::asset::Build;
use anyhow::{Result, anyhow, ensure};
use tokio::sync::watch;

#[derive(Default)]
pub(super) struct RuntimeInstalls {
    state: Mutex<InstallState>,
}

#[derive(Default)]
struct InstallState {
    active: Option<Arc<Flight>>,
    requests: Vec<Arc<InstallRequest>>,
    errors: HashMap<String, String>,
}

struct Flight {
    build: Build,
    scope: String,
    outcome: watch::Sender<Option<std::result::Result<(), String>>>,
}

pub(super) struct InstallRequest {
    scope: String,
    cancelled: watch::Sender<bool>,
}

impl InstallRequest {
    pub(super) fn check_cancelled(&self) -> Result<()> {
        ensure!(!*self.cancelled.borrow(), "cancelled");
        Ok(())
    }
}

struct RequestGuard<'a> {
    queue: &'a RuntimeInstalls,
    request: Arc<InstallRequest>,
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        self.queue
            .state
            .lock()
            .unwrap()
            .requests
            .retain(|request| !Arc::ptr_eq(request, &self.request));
    }
}

struct Owner<'a> {
    queue: &'a RuntimeInstalls,
    flight: Arc<Flight>,
    outcome: Option<std::result::Result<(), String>>,
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        let mut state = self.queue.state.lock().unwrap();
        state.active = None;
        self.flight.outcome.send_replace(Some(
            self.outcome
                .take()
                .unwrap_or_else(|| Err("audio.cpp runtime installation was interrupted".into())),
        ));
    }
}

impl RuntimeInstalls {
    /// The tool owns its cancellation. The Downloader still stops the actual transfer
    /// only when its owner asked for that.
    pub(super) fn cancel(&self, scope: &str) -> bool {
        let state = self.state.lock().unwrap();
        let mut found = false;
        for request in state
            .requests
            .iter()
            .filter(|request| request.scope == scope)
        {
            request.cancelled.send_replace(true);
            found = true;
        }
        found
    }

    pub(super) fn waiting_on(&self, scope: &str) -> Option<String> {
        let state = self.state.lock().unwrap();
        let active = state.active.as_ref()?;
        if active.scope == scope || !state.requests.iter().any(|request| request.scope == scope) {
            return None;
        }
        Some(active.scope.clone())
    }

    pub(super) fn error_for(&self, scope: &str) -> Option<String> {
        self.state.lock().unwrap().errors.get(scope).cloned()
    }

    pub(super) async fn run<T, Wait>(
        &self,
        build: Build,
        scope: &str,
        install: impl FnOnce(Arc<InstallRequest>) -> T,
        waiting: impl FnOnce() -> Wait,
    ) -> Result<()>
    where
        T: Future<Output = Result<()>>,
        Wait: Future<Output = ()>,
    {
        let request = Arc::new(InstallRequest {
            scope: scope.into(),
            cancelled: watch::channel(false).0,
        });
        let mut cancel = request.cancelled.subscribe();
        {
            let mut state = self.state.lock().unwrap();
            state.errors.remove(scope);
            state.requests.push(request.clone());
        }
        let _request = RequestGuard {
            queue: self,
            request: request.clone(),
        };
        let mut waiting = Some(waiting);
        let result = async { loop {
            request.check_cancelled()?;
            let (flight, owner) = {
                let mut state = self.state.lock().unwrap();
                if let Some(flight) = &state.active {
                    (flight.clone(), None)
                } else {
                    let flight = Arc::new(Flight {
                        build,
                        scope: scope.into(),
                        outcome: watch::channel(None).0,
                    });
                    state.active = Some(flight.clone());
                    let owner = Owner { queue: self, flight: flight.clone(), outcome: None };
                    (flight, Some(owner))
                }
            };
            if let Some(mut owner) = owner {
                let outcome = match request.check_cancelled() {
                    Ok(()) => install(request.clone()).await,
                    Err(error) => Err(error),
                };
                owner.outcome = Some(outcome.as_ref().map(|_| ()).map_err(|error| format!("{error:#}")));
                return outcome;
            }

            if let Some(waiting) = waiting.take() {
                waiting().await;
            }
            let mut outcome = flight.outcome.subscribe();
            let finished = tokio::select! {
                biased;
                _ = cancel.wait_for(|cancelled| *cancelled) => return Err(anyhow!("cancelled")),
                result = outcome.wait_for(Option::is_some) => result?.clone().expect("completed installation"),
            };
            if flight.build == build {
                return finished.map_err(anyhow::Error::msg);
            }
            // The other build shares the downloader but uses different files.
            // Its slot has been released; re-evaluate what our build needs.
        }}.await;
        if let Err(error) = &result {
            self.state
                .lock()
                .unwrap()
                .errors
                .insert(scope.into(), format!("{error:#}"));
        }
        result
    }
}
