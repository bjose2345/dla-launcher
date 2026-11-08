//! What the package preparation, catalog import and search rebuild
//! controllers share: one operation of the kind at a time, its latest
//! progress kept for polling and sent as a Tauri event, and cancellation
//! through a token the work polls.

use std::{
    fmt::Display,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use dla_application::cancellation::CancellationToken;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Progress events of one operation go out at most this often, except for a
/// stage change or the end; the UI polls `latest` between them.
const EMIT_INTERVAL: Duration = Duration::from_millis(100);

pub trait OperationProgress: Clone + Serialize + Send + Sync + 'static {
    type Stage: Copy + PartialEq + Send;

    fn operation_id(&self) -> &str;
    fn stage(&self) -> Self::Stage;
    fn is_terminal(&self) -> bool;
}

/// The latest progress of one operation kind, for polling, and its event.
pub struct ProgressPublisher<P: OperationProgress> {
    app: AppHandle,
    event: &'static str,
    target: &'static str,
    latest: Mutex<Option<P>>,
    gate: Mutex<EventGate<P::Stage>>,
}

impl<P: OperationProgress> ProgressPublisher<P> {
    fn new(app: AppHandle, event: &'static str, target: &'static str) -> Self {
        Self {
            app,
            event,
            target,
            latest: Mutex::new(None),
            gate: Mutex::new(EventGate::default()),
        }
    }

    pub fn record(&self, progress: &P) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(progress.clone());
        let emit = self
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .should_emit(progress, Instant::now());
        if emit && let Err(error) = self.app.emit(self.event, progress) {
            log::warn!(
                target: self.target,
                "event=progress_emit_failed name={} error={error}",
                self.event
            );
        }
    }

    pub fn latest(&self) -> Option<P> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct EventGate<S> {
    operation_id: Option<String>,
    stage: Option<S>,
    emitted_at: Option<Instant>,
}

impl<S> Default for EventGate<S> {
    fn default() -> Self {
        Self {
            operation_id: None,
            stage: None,
            emitted_at: None,
        }
    }
}

impl<S: Copy + PartialEq> EventGate<S> {
    fn should_emit<P: OperationProgress<Stage = S>>(&mut self, progress: &P, now: Instant) -> bool {
        let operation_changed = self.operation_id.as_deref() != Some(progress.operation_id());
        let stage_changed = self.stage != Some(progress.stage());
        let interval_elapsed = self
            .emitted_at
            .is_none_or(|emitted_at| now.saturating_duration_since(emitted_at) >= EMIT_INTERVAL);
        if !operation_changed && !stage_changed && !interval_elapsed && !progress.is_terminal() {
            return false;
        }
        self.operation_id = Some(progress.operation_id().to_owned());
        self.stage = Some(progress.stage());
        self.emitted_at = Some(now);
        true
    }
}

/// The one operation of a kind that may run at a time, with what the
/// controller needs to know about it meanwhile.
pub struct OperationSlot<A, E> {
    active: Mutex<Option<Active<A, E>>>,
}

struct Active<A, E> {
    operation_id: String,
    cancellation: CancellationToken<E>,
    attachment: A,
}

impl<A, E> OperationSlot<A, E> {
    fn new() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Active<A, E>>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the slot for `operation_id`; false while another operation holds it.
    pub fn claim(
        &self,
        operation_id: &str,
        cancellation: &CancellationToken<E>,
        attachment: A,
    ) -> bool {
        let mut active = self.lock();
        if active.is_some() {
            return false;
        }
        *active = Some(Active {
            operation_id: operation_id.to_owned(),
            cancellation: cancellation.clone(),
            attachment,
        });
        true
    }

    pub fn is_busy(&self) -> bool {
        self.lock().is_some()
    }

    pub fn holds(&self, matches: impl FnOnce(&A) -> bool) -> bool {
        self.lock()
            .as_ref()
            .is_some_and(|active| matches(&active.attachment))
    }

    pub fn attachment(&self, operation_id: &str) -> Option<A>
    where
        A: Clone,
    {
        self.lock()
            .as_ref()
            .filter(|active| active.operation_id == operation_id)
            .map(|active| active.attachment.clone())
    }

    /// Asks the operation holding the slot to stop; false when it is not
    /// `operation_id`.
    pub fn cancel(&self, operation_id: &str) -> bool {
        match self.lock().as_ref() {
            Some(active) if active.operation_id == operation_id => {
                active.cancellation.cancel();
                true
            }
            _ => false,
        }
    }

    /// Frees the slot when `operation_id` holds it.
    pub fn release(&self, operation_id: &str) {
        let mut active = self.lock();
        if active
            .as_ref()
            .is_some_and(|active| active.operation_id == operation_id)
        {
            *active = None;
        }
    }
}

pub struct BackgroundOperation<P: OperationProgress, A, E> {
    pub progress: ProgressPublisher<P>,
    pub slot: OperationSlot<A, E>,
    target: &'static str,
    name: &'static str,
}

impl<P: OperationProgress, A: Clone, E: Display> BackgroundOperation<P, A, E> {
    pub fn new(
        app: AppHandle,
        event: &'static str,
        target: &'static str,
        name: &'static str,
    ) -> Self {
        Self {
            progress: ProgressPublisher::new(app, event, target),
            slot: OperationSlot::new(),
            target,
            name,
        }
    }

    /// Takes the slot for the operation `initial` describes and publishes it;
    /// false while another operation holds the slot.
    pub fn begin(&self, attachment: A, cancellation: &CancellationToken<E>, initial: &P) -> bool {
        if !self
            .slot
            .claim(initial.operation_id(), cancellation, attachment)
        {
            return false;
        }
        self.progress.record(initial);
        true
    }

    pub fn cancel(&self, operation_id: &str) -> bool {
        let requested = self.slot.cancel(operation_id);
        if requested {
            log::info!(
                target: self.target,
                "event={}_cancel_requested operation_id={operation_id}",
                self.name
            );
        }
        requested
    }

    /// Ends the operation: logs the outcome, publishes the terminal progress
    /// `terminal` builds from this operation's latest one and its attachment
    /// when the work failed or was cancelled, and frees the slot.
    pub fn finish(
        &self,
        operation_id: &str,
        result: Result<(), E>,
        terminal: impl FnOnce(Option<&P>, Option<A>, &E) -> P,
    ) {
        match &result {
            Ok(()) => log::info!(
                target: self.target,
                "event={}_completed operation_id={operation_id}",
                self.name
            ),
            Err(error) => log::warn!(
                target: self.target,
                "event={}_stopped operation_id={operation_id} error={error}",
                self.name
            ),
        }
        if let Err(error) = &result {
            let latest = self
                .progress
                .latest()
                .filter(|progress| progress.operation_id() == operation_id);
            let attachment = self.slot.attachment(operation_id);
            self.progress
                .record(&terminal(latest.as_ref(), attachment, error));
        }
        self.slot.release(operation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Serialize)]
    struct TestProgress {
        operation_id: String,
        stage: u8,
    }

    impl OperationProgress for TestProgress {
        type Stage = u8;

        fn operation_id(&self) -> &str {
            &self.operation_id
        }

        fn stage(&self) -> u8 {
            self.stage
        }

        fn is_terminal(&self) -> bool {
            self.stage == 9
        }
    }

    fn progress(operation_id: &str, stage: u8) -> TestProgress {
        TestProgress {
            operation_id: operation_id.to_owned(),
            stage,
        }
    }

    #[test]
    fn event_gate_limits_repeated_updates_but_never_stage_changes_or_the_end() {
        let started_at = Instant::now();
        let mut gate = EventGate::default();

        assert!(gate.should_emit(&progress("operation", 1), started_at));
        assert!(!gate.should_emit(
            &progress("operation", 1),
            started_at + Duration::from_millis(99)
        ));
        assert!(gate.should_emit(&progress("operation", 1), started_at + EMIT_INTERVAL));
        assert!(gate.should_emit(
            &progress("operation", 2),
            started_at + Duration::from_millis(101)
        ));
        assert!(gate.should_emit(
            &progress("operation", 9),
            started_at + Duration::from_millis(102)
        ));
        assert!(gate.should_emit(
            &progress("next-operation", 0),
            started_at + Duration::from_millis(103)
        ));
    }

    #[test]
    fn slot_holds_one_operation_and_cancels_only_it() {
        let slot: OperationSlot<&str, ()> = OperationSlot::new();
        let first = CancellationToken::default();
        assert!(slot.claim("first", &first, "a"));
        assert!(!slot.claim("second", &CancellationToken::default(), "b"));
        assert!(slot.is_busy());
        assert!(slot.holds(|attachment| *attachment == "a"));
        assert_eq!(slot.attachment("first"), Some("a"));
        assert_eq!(slot.attachment("second"), None);

        assert!(!slot.cancel("second"));
        assert!(!first.is_cancelled());
        assert!(slot.cancel("first"));
        assert!(first.is_cancelled());

        slot.release("second");
        assert!(slot.is_busy());
        slot.release("first");
        assert!(!slot.is_busy());
    }
}
