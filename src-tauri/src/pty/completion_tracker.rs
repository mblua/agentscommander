use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

const WATCHER_INTERVAL: Duration = Duration::from_secs(5);

type CompletedCallback = Arc<dyn Fn(Uuid, String) + Send + Sync>;
type HungCallback = Arc<dyn Fn(Uuid, String, u64) + Send + Sync>;
type FollowupCallback = Arc<dyn Fn(Uuid, String) + Send + Sync>;

pub struct CompletionTracker {
    state: Arc<Mutex<HashMap<Uuid, SessionCompletionState>>>,
    hung_timeout: Duration,
    on_completed: CompletedCallback,
    on_hung: HungCallback,
    on_followup: FollowupCallback,
}

struct SessionCompletionState {
    name: String,
    has_pending_response: bool,
    followup_sent: bool,
    idle_since: Option<Instant>,
    status: CompletionStatus,
    hung_notified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompletionStatus {
    Working,
    Completed,
    Hung,
}

impl CompletionTracker {
    pub fn new(
        hung_timeout_secs: u64,
        on_completed: impl Fn(Uuid, String) + Send + Sync + 'static,
        on_hung: impl Fn(Uuid, String, u64) + Send + Sync + 'static,
        on_followup: impl Fn(Uuid, String) + Send + Sync + 'static,
    ) -> Arc<Self> {
        log::info!(
            "[completion] initialized: hung_timeout={}s",
            hung_timeout_secs
        );
        Arc::new(Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            hung_timeout: Duration::from_secs(hung_timeout_secs),
            on_completed: Arc::new(on_completed),
            on_hung: Arc::new(on_hung),
            on_followup: Arc::new(on_followup),
        })
    }

    /// Register a Claude session for completion tracking.
    /// Only registered sessions are monitored for response tracking and hung state.
    pub fn register_session(&self, session_id: Uuid, name: String) {
        let mut state = self.state.lock().unwrap();
        state.entry(session_id).or_insert_with(|| SessionCompletionState {
            name,
            has_pending_response: false,
            followup_sent: false,
            idle_since: None,
            status: CompletionStatus::Working,
            hung_notified: false,
        });
        log::info!("[completion] registered session {}", &session_id.to_string()[..8]);
    }

    /// Called after a message is injected into a session's PTY.
    /// Marks that the agent has a pending response obligation.
    pub fn record_message_received(&self, session_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.get_mut(&session_id) {
            entry.has_pending_response = true;
            log::info!("[completion] message received for {}", &session_id.to_string()[..8]);
        }
    }

    /// Called when an agent successfully sends a message via the outbox.
    /// Clears the pending response obligation.
    pub fn record_response_sent(&self, session_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.get_mut(&session_id) {
            entry.has_pending_response = false;
            log::info!("[completion] response sent by {}", &session_id.to_string()[..8]);
        }
    }

    /// Called by the on_followup callback after injecting the reminder.
    pub fn mark_followup_sent(&self, session_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.get_mut(&session_id) {
            entry.followup_sent = true;
            log::info!("[completion] followup sent for {}", &session_id.to_string()[..8]);
        }
    }

    /// Called when a message is injected into a session (resets all tracking).
    pub fn reset(&self, session_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(s) = state.get_mut(&session_id) {
            s.has_pending_response = false;
            s.followup_sent = false;
            s.idle_since = None;
            s.status = CompletionStatus::Working;
            s.hung_notified = false;
            log::info!("[completion] reset for {}", &session_id.to_string()[..8]);
        }
    }

    /// Called when idle detector fires session_idle.
    /// Only acts on registered (Claude) sessions.
    pub fn mark_idle(&self, session_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.get_mut(&session_id) {
            if entry.idle_since.is_none() {
                entry.idle_since = Some(Instant::now());
            }
        }
    }

    /// Called when idle detector fires session_busy.
    /// Resets idle_since and hung_notified, but does NOT reset Completed status.
    /// Only reset() on message injection transitions back to Working.
    pub fn mark_busy(&self, session_id: Uuid) {
        let mut state = self.state.lock().unwrap();
        if let Some(s) = state.get_mut(&session_id) {
            s.idle_since = None;
            s.hung_notified = false;
        }
    }

    /// Remove session from tracking.
    pub fn remove_session(&self, session_id: Uuid) {
        self.state.lock().unwrap().remove(&session_id);
    }

    /// Query current status for a session.
    pub fn get_status(&self, session_id: Uuid) -> CompletionStatus {
        self.state.lock().unwrap()
            .get(&session_id)
            .map(|s| s.status)
            .unwrap_or(CompletionStatus::Working)
    }

    /// Start background watcher thread.
    pub fn start(self: &Arc<Self>, shutdown: crate::shutdown::ShutdownSignal) {
        let tracker = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(WATCHER_INTERVAL);

                if shutdown.is_cancelled() {
                    log::info!("[CompletionTracker] Shutdown signal received, stopping");
                    break;
                }

                // Skip if hung timeout is disabled (0)
                if tracker.hung_timeout.is_zero() {
                    continue;
                }

                // Collect events under lock, fire callbacks after unlocking
                let mut completed: Vec<(Uuid, String)> = Vec::new();
                let mut hung: Vec<(Uuid, String, u64)> = Vec::new();
                let mut followups: Vec<(Uuid, String)> = Vec::new();

                {
                    let now = Instant::now();
                    let mut state = tracker.state.lock().unwrap();

                    for (&session_id, s) in state.iter_mut() {
                        // Completed: agent responded and is idle past timeout
                        if s.status == CompletionStatus::Working && !s.has_pending_response && s.idle_since.is_some() {
                            if let Some(idle_since) = s.idle_since {
                                if let Some(elapsed) = now.checked_duration_since(idle_since) {
                                    if elapsed > tracker.hung_timeout {
                                        s.status = CompletionStatus::Completed;
                                        completed.push((session_id, s.name.clone()));
                                    }
                                }
                            }
                        }

                        // Follow-up: agent has pending response, idle past timeout, no follow-up sent yet
                        if s.status == CompletionStatus::Working && s.has_pending_response && !s.followup_sent {
                            if let Some(idle_since) = s.idle_since {
                                if let Some(elapsed) = now.checked_duration_since(idle_since) {
                                    if elapsed > tracker.hung_timeout {
                                        followups.push((session_id, s.name.clone()));
                                        // Don't change status here — on_followup callback will set followup_sent
                                    }
                                }
                            }
                        }

                        // Hung: agent has pending response, follow-up already sent, still idle past timeout
                        if s.status == CompletionStatus::Working && s.has_pending_response && s.followup_sent {
                            if let Some(idle_since) = s.idle_since {
                                if let Some(elapsed) = now.checked_duration_since(idle_since) {
                                    if elapsed > tracker.hung_timeout && !s.hung_notified {
                                        let idle_minutes = elapsed.as_secs() / 60;
                                        s.status = CompletionStatus::Hung;
                                        s.hung_notified = true;
                                        hung.push((session_id, s.name.clone(), idle_minutes));
                                    }
                                }
                            }
                        }
                    }
                }

                // Fire callbacks outside the lock
                for (id, name) in completed {
                    log::info!("[completion] session {} ({}) completed", &id.to_string()[..8], name);
                    (tracker.on_completed)(id, name);
                }
                for (id, name) in followups {
                    log::info!("[completion] sending follow-up to {} ({})", &id.to_string()[..8], name);
                    (tracker.on_followup)(id, name);
                }
                for (id, name, idle_minutes) in hung {
                    log::warn!("[completion] session {} ({}) appears hung (idle={}min, timeout={}s)", &id.to_string()[..8], name, idle_minutes, tracker.hung_timeout.as_secs());
                    (tracker.on_hung)(id, name, idle_minutes);
                }
            }
        });
    }
}

impl Default for SessionCompletionState {
    fn default() -> Self {
        Self {
            name: String::new(),
            has_pending_response: false,
            followup_sent: false,
            idle_since: None,
            status: CompletionStatus::Working,
            hung_notified: false,
        }
    }
}
