// SPDX-License-Identifier: MIT

//! Transient "copied" feedback.
//!
//! Writing to the clipboard produces no visible change, so a control that
//! copies has to say so itself. Both binaries show a tick in place of the copy
//! affordance for a couple of seconds; this is the state behind that.

use std::time::{Duration, Instant};

/// How long the tick stays up after a copy.
const FEEDBACK: Duration = Duration::from_secs(2);

/// How often to check whether the tick has expired. Fine enough that it never
/// visibly overstays, coarse enough that the subscription is nearly free — and
/// it only runs while a tick is actually showing.
pub const TICK: Duration = Duration::from_millis(250);

/// What was last copied, and when.
///
/// Keyed on the copied text rather than on a widget identity: no two controls
/// offer the same string, so a row can ask "was it me?" without having to
/// invent an id for itself. Re-copying the same value restarts the clock.
#[derive(Debug, Clone, Default)]
pub struct Feedback(Option<(String, Instant)>);

impl Feedback {
    /// Record a copy that has just happened.
    pub fn mark(&mut self, value: String) {
        self.0 = Some((value, Instant::now()));
    }

    /// Whether the control offering `value` should be showing its tick.
    pub fn shows(&self, value: &str) -> bool {
        matches!(&self.0, Some((copied, _)) if copied == value)
    }

    /// Whether anything is showing. Drives the timer subscription, so that no
    /// timer runs when no tick is up.
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    /// Clear the tick once it has been up long enough.
    pub fn expire(&mut self) {
        if self
            .0
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= FEEDBACK)
        {
            self.0 = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_only_the_copied_value() {
        let mut feedback = Feedback::default();
        assert!(!feedback.is_active());

        feedback.mark("100.64.0.1".to_owned());
        assert!(feedback.shows("100.64.0.1"));
        assert!(!feedback.shows("host.example.ts.net"));
        assert!(feedback.is_active());
    }

    /// A fresh mark must survive an expiry check, or the tick would vanish on
    /// the very next timer tick after a copy.
    #[test]
    fn fresh_marks_survive_expiry() {
        let mut feedback = Feedback::default();
        feedback.mark("100.64.0.1".to_owned());
        feedback.expire();
        assert!(feedback.shows("100.64.0.1"));
    }

    /// The whole point is that the tick goes away on its own. Sleeps for the
    /// real feedback window rather than mocking the clock, because the clock
    /// is the behaviour under test.
    #[test]
    fn the_tick_expires() {
        let mut feedback = Feedback::default();
        feedback.mark("100.64.0.1".to_owned());

        std::thread::sleep(FEEDBACK + Duration::from_millis(50));
        feedback.expire();

        assert!(!feedback.shows("100.64.0.1"));
        assert!(!feedback.is_active());
    }

    /// Copying a second value replaces the first, so only one tick is ever up.
    #[test]
    fn a_second_copy_replaces_the_first() {
        let mut feedback = Feedback::default();
        feedback.mark("100.64.0.1".to_owned());
        feedback.mark("host.example.ts.net".to_owned());
        assert!(!feedback.shows("100.64.0.1"));
        assert!(feedback.shows("host.example.ts.net"));
    }
}
