//! SigmaOS Notifications - unified notification daemon core.
//!
//! In-process notification manager inspired by mako/dunst: apps post
//! notifications, the daemon tracks active ones, keeps a bounded
//! history, expires stale entries on `tick`, and notifies registered
//! observers so a shell widget can render them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Importance of a notification; higher sorts first.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Informational.
    Low,
    /// Normal.
    #[default]
    Normal,
    /// Urgent — requires attention.
    High,
    /// Critical — system-level.
    Critical,
}

/// A single notification posted by an app.
#[derive(Debug, Clone)]
pub struct Notification {
    /// Daemon-assigned id.
    pub id: u64,
    /// Sending application or service name.
    pub app: String,
    /// Short title.
    pub summary: String,
    /// Optional body text.
    pub body: String,
    /// Importance.
    pub priority: Priority,
    /// Monotonic sequence number (for ordering/timing).
    pub seq: u64,
}

/// Observer notified whenever the active set changes.
pub trait Observer {
    /// Called after a notification is posted, dismissed, or expired.
    fn on_change(&mut self, active: &[Notification]);
}

/// Bounded history of dismissed notifications.
const HISTORY_CAP: usize = 200;

/// Unified notification daemon.
pub struct NotificationDaemon {
    active: Vec<Notification>,
    history: Vec<Notification>,
    observers: Vec<Box<dyn Observer>>,
    next_id: u64,
    seq: u64,
}

impl NotificationDaemon {
    /// Create an empty daemon.
    pub fn new() -> Self {
        Self {
            active: Vec::new(),
            history: Vec::new(),
            observers: Vec::new(),
            next_id: 0,
            seq: 0,
        }
    }

    /// Post a notification; returns its id.
    pub fn notify(
        &mut self,
        app: impl Into<String>,
        summary: impl Into<String>,
        body: impl Into<String>,
        priority: Priority,
    ) -> u64 {
        self.seq += 1;
        self.next_id += 1;
        let n = Notification {
            id: self.next_id,
            app: app.into(),
            summary: summary.into(),
            body: body.into(),
            priority,
            seq: self.seq,
        };
        self.active.push(n);
        self.active
            .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.seq.cmp(&b.seq)));
        self.notify_observers();
        self.next_id
    }

    /// Dismiss an active notification by id; returns it for history.
    pub fn dismiss(&mut self, id: u64) -> Option<Notification> {
        let pos = self.active.iter().position(|n| n.id == id)?;
        let n = self.active.remove(pos);
        self.push_history(n.clone());
        self.notify_observers();
        Some(n)
    }

    /// Dismiss all notifications from an app.
    pub fn dismiss_app(&mut self, app: &str) -> usize {
        let before = self.active.len();
        let mut removed = Vec::new();
        self.active.retain(|n| {
            if n.app == app {
                removed.push(n.clone());
                false
            } else {
                true
            }
        });
        for n in removed {
            self.push_history(n);
        }
        let count = before - self.active.len();
        if count > 0 {
            self.notify_observers();
        }
        count
    }

    /// Expire notifications older than `max_seq` delta — call periodically
    /// with the current `seq` budget; removes anything below it.
    pub fn expire_older_than(&mut self, min_seq: u64) {
        let mut expired = Vec::new();
        self.active.retain(|n| {
            if n.seq < min_seq {
                expired.push(n.clone());
                false
            } else {
                true
            }
        });
        for n in expired {
            self.push_history(n);
        }
        self.notify_observers();
    }

    /// Active notifications, highest priority first.
    pub fn active(&self) -> &[Notification] {
        &self.active
    }

    /// Recent dismissed notifications, newest first.
    pub fn history(&self) -> impl Iterator<Item = &Notification> {
        self.history.iter().rev()
    }

    /// Register an observer.
    pub fn add_observer(&mut self, observer: Box<dyn Observer>) {
        self.observers.push(observer);
    }

    fn push_history(&mut self, n: Notification) {
        if self.history.len() == HISTORY_CAP {
            self.history.remove(0);
        }
        self.history.push(n);
    }

    fn notify_observers(&mut self) {
        for obs in &mut self.observers {
            obs.on_change(&self.active);
        }
    }
}

impl Default for NotificationDaemon {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_orders_by_priority() {
        let mut d = NotificationDaemon::new();
        d.notify("a", "low", "", Priority::Low);
        d.notify("b", "crit", "", Priority::Critical);
        d.notify("c", "norm", "", Priority::Normal);
        let active = d.active();
        assert_eq!(active[0].summary, "crit");
        assert_eq!(active[1].summary, "norm");
        assert_eq!(active[2].summary, "low");
    }

    #[test]
    fn dismiss_moves_to_history() {
        let mut d = NotificationDaemon::new();
        let id = d.notify("app", "s", "b", Priority::Normal);
        assert!(d.dismiss(id).is_some());
        assert!(d.active().is_empty());
        assert_eq!(d.history().count(), 1);
        assert!(d.dismiss(id).is_none());
    }

    #[test]
    fn dismiss_app_removes_all_from_app() {
        let mut d = NotificationDaemon::new();
        d.notify("a", "1", "", Priority::Normal);
        d.notify("b", "2", "", Priority::Normal);
        d.notify("a", "3", "", Priority::High);
        assert_eq!(d.dismiss_app("a"), 2);
        assert_eq!(d.active().len(), 1);
        assert_eq!(d.history().count(), 2);
    }

    #[test]
    fn expire_older_than_collects_history() {
        let mut d = NotificationDaemon::new();
        d.notify("a", "old", "", Priority::Normal); // seq 1
        d.notify("a", "new", "", Priority::Normal); // seq 2
        d.expire_older_than(2);
        assert_eq!(d.active().len(), 1);
        assert_eq!(d.active()[0].summary, "new");
        assert_eq!(d.history().count(), 1);
    }

    #[test]
    fn history_is_bounded() {
        let mut d = NotificationDaemon::new();
        for i in 0..(HISTORY_CAP + 10) {
            let id = d.notify("a", format!("n{i}"), "", Priority::Normal);
            d.dismiss(id);
        }
        assert_eq!(d.history().count(), HISTORY_CAP);
        // Newest first: the last dismissed is at the front.
        assert_eq!(
            d.history().next().unwrap().summary,
            format!("n{}", HISTORY_CAP + 9)
        );
    }

    #[test]
    fn ids_and_seq_are_monotonic() {
        let mut d = NotificationDaemon::new();
        let a = d.notify("a", "1", "", Priority::Normal);
        let b = d.notify("b", "2", "", Priority::Normal);
        assert!(b > a);
        let seqs: Vec<u64> = d.active().iter().map(|n| n.seq).collect();
        assert_eq!(seqs, vec![1, 2]);
        assert_eq!(d.active().len(), 2);
    }

    #[test]
    fn expire_boundary_is_exclusive() {
        let mut d = NotificationDaemon::new();
        d.notify("a", "s1", "", Priority::Normal); // seq 1
        d.notify("a", "s2", "", Priority::Normal); // seq 2
        d.expire_older_than(2); // seq < 2 expires
        assert_eq!(d.active().len(), 1);
        assert_eq!(d.active()[0].summary, "s2");
        // Nothing left to expire at seq 2.
        d.expire_older_than(2);
        assert_eq!(d.history().count(), 1);
    }

    #[test]
    fn observer_sees_changes() {
        use std::cell::RefCell;
        use std::rc::Rc;

        struct Rec(Rc<RefCell<Vec<usize>>>);
        impl Observer for Rec {
            fn on_change(&mut self, active: &[Notification]) {
                self.0.borrow_mut().push(active.len());
            }
        }

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut d = NotificationDaemon::new();
        d.add_observer(Box::new(Rec(Rc::clone(&seen))));
        d.notify("x", "s", "", Priority::Normal);
        let id = d.active()[0].id;
        d.dismiss(id);
        assert_eq!(*seen.borrow(), vec![1, 0]);
    }
}
