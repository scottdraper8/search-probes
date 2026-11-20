//! Transient overlay feedback. Runtime search events are supplied by the caller, never inferred
//! from UI snapshots. Explicit timestamps keep filtering, coalescing and expiry deterministic.

use crate::theme::Palette;
use egui::{Align2, RichText, Vec2};
use search_probes_model::settings::{NotificationSettings, ToastPlacement};
use search_probes_model::ui_control::ToastSnapshot;
use std::{collections::VecDeque, time::Duration};

const LIFETIME: Duration = Duration::from_secs(4);
const RESULT_BURST: Duration = Duration::from_millis(750);
const MAX_TOASTS: usize = 3;
const MAX_MESSAGE_BYTES: usize = 1024;

fn bound_message(message: &mut String) {
    if message.len() > MAX_MESSAGE_BYTES {
        let mut boundary = MAX_MESSAGE_BYTES - '…'.len_utf8();
        while !message.is_char_boundary(boundary) {
            boundary -= 1;
        }
        message.truncate(boundary);
        message.push('…');
    }
}

/// Semantic intake for the runtime lifecycle journal. User pause/resume has no variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    ResultsFound { count: usize },
    SearchCompleted { message: String },
    AutomaticallyPaused { reason: String },
    AutomaticallyResumed,
}

impl Notification {
    fn enabled(&self, preferences: &NotificationSettings) -> bool {
        match self {
            Self::ResultsFound { .. } => preferences.result_found,
            Self::SearchCompleted { .. } => preferences.search_completed,
            Self::AutomaticallyPaused { .. } => preferences.automatic_pause,
            Self::AutomaticallyResumed => preferences.automatic_resume,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::ResultsFound { .. } => "Result found",
            Self::SearchCompleted { .. } => "Search completed",
            Self::AutomaticallyPaused { .. } => "Search automatically paused",
            Self::AutomaticallyResumed => "Search automatically resumed",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::ResultsFound { count: 1 } => "Found a matching result.".into(),
            Self::ResultsFound { count } => format!("Found {count} matching results."),
            Self::SearchCompleted { message } => message.clone(),
            Self::AutomaticallyPaused { reason } => reason.clone(),
            Self::AutomaticallyResumed => "Searching again.".into(),
        }
    }
}

#[derive(Debug)]
enum Content {
    Search(Notification),
    Export { subject: &'static str, path: String },
}

#[derive(Debug)]
struct Toast {
    content: Content,
    created: Duration,
    expires: Duration,
}

#[derive(Default)]
pub(crate) struct Notifications {
    toasts: VecDeque<Toast>,
}

impl Notifications {
    fn text(content: &Content) -> (String, String) {
        match content {
            Content::Search(notification) => {
                (notification.title().to_owned(), notification.message())
            }
            Content::Export { subject, path } => (
                match *subject {
                    "Search" => "Search exported",
                    "Presets" => "Presets exported",
                    _ => "Results exported",
                }
                .to_owned(),
                format!("Exported {} to {path}", subject.to_lowercase()),
            ),
        }
    }

    pub fn update(&mut self, now: Duration, preferences: &NotificationSettings) {
        self.toasts.retain(|toast| {
            now < toast.expires
                && match &toast.content {
                    Content::Search(notification) => notification.enabled(preferences),
                    Content::Export { .. } => true,
                }
        });
    }

    pub fn push(
        &mut self,
        notification: Notification,
        now: Duration,
        preferences: &NotificationSettings,
    ) {
        self.push_aged(notification, Duration::ZERO, now, preferences);
    }

    pub fn push_aged(
        &mut self,
        mut notification: Notification,
        age: Duration,
        now: Duration,
        preferences: &NotificationSettings,
    ) {
        self.update(now, preferences);
        if !notification.enabled(preferences) || age >= LIFETIME {
            return;
        }
        let created = now.saturating_sub(age);
        let remaining = LIFETIME - age;
        match &mut notification {
            Notification::SearchCompleted { message } => bound_message(message),
            Notification::AutomaticallyPaused { reason } => bound_message(reason),
            _ => {}
        }
        if let Notification::ResultsFound { count } = notification {
            if count == 0 {
                return;
            }
            if let Some(last) = self.toasts.back_mut()
                && created.saturating_sub(last.created) <= RESULT_BURST
                && let Content::Search(Notification::ResultsFound { count: previous }) =
                    &mut last.content
            {
                *previous = previous.saturating_add(count);
                last.expires = last.expires.min(now.saturating_add(remaining));
                return;
            }
            self.append(
                Content::Search(Notification::ResultsFound { count }),
                created,
                now.saturating_add(remaining),
            );
        } else {
            self.append(
                Content::Search(notification),
                created,
                now.saturating_add(remaining),
            );
        }
    }

    fn append(&mut self, content: Content, created: Duration, expires: Duration) {
        if self.toasts.len() == MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(Toast {
            content,
            created,
            expires,
        });
    }

    pub fn clear_export(&mut self) {
        self.toasts
            .retain(|toast| !matches!(toast.content, Content::Export { .. }));
    }

    pub fn clear_search(&mut self) {
        self.toasts
            .retain(|toast| matches!(toast.content, Content::Export { .. }));
    }

    pub fn clear(&mut self) {
        self.toasts.clear();
    }

    pub fn snapshot(&self) -> Vec<ToastSnapshot> {
        self.toasts
            .iter()
            .map(|toast| {
                let (title, message) = Self::text(&toast.content);
                ToastSnapshot {
                    title,
                    message,
                    created_ms: toast.created.as_millis().min(u128::from(u64::MAX)) as u64,
                    expires_ms: toast.expires.as_millis().min(u128::from(u64::MAX)) as u64,
                }
            })
            .collect()
    }

    pub fn exported(&mut self, subject: &'static str, mut path: String, now: Duration) {
        bound_message(&mut path);
        self.clear_export();
        self.append(
            Content::Export { subject, path },
            now,
            now.saturating_add(LIFETIME),
        );
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub fn show(&self, ctx: &egui::Context, palette: Palette, placement: ToastPlacement) {
        if self.is_empty() {
            return;
        }
        let (anchor, offset) = placement_anchor(placement);
        let screen = ctx.content_rect();
        let width = (screen.width() - 32.0).clamp(120.0, 340.0);
        // Keep the stack inside short viewports; all retained cards continue to expire normally.
        let maximum = ((screen.height() - 32.0) / 140.0).floor().max(1.0) as usize;
        egui::Area::new(egui::Id::new("search_probes_notifications"))
            .order(egui::Order::Foreground)
            .anchor(anchor, offset)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_width(width);
                for toast in self.toasts.iter().rev().take(maximum) {
                    let (title, message) = Self::text(&toast.content);
                    egui::Frame::new()
                        .fill(palette.surface)
                        .stroke(egui::Stroke::new(1.0, palette.outline))
                        .corner_radius(6.0)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_width((width - 24.0).max(1.0));
                            ui.label(RichText::new(title).strong().color(palette.on_surface));
                            let mut job = egui::text::LayoutJob::simple(
                                message,
                                egui::FontId::proportional(15.0),
                                palette.on_surface,
                                width - 24.0,
                            );
                            job.wrap.max_rows = 4;
                            ui.label(job);
                        });
                }
            });
    }
}

fn placement_anchor(placement: ToastPlacement) -> (Align2, Vec2) {
    match placement {
        ToastPlacement::TopLeft => (Align2::LEFT_TOP, egui::vec2(16.0, 16.0)),
        ToastPlacement::TopCenter => (Align2::CENTER_TOP, egui::vec2(0.0, 16.0)),
        ToastPlacement::TopRight => (Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0)),
        ToastPlacement::BottomLeft => (Align2::LEFT_BOTTOM, egui::vec2(16.0, -16.0)),
        ToastPlacement::BottomCenter => (Align2::CENTER_BOTTOM, egui::vec2(0.0, -16.0)),
        ToastPlacement::BottomRight => (Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_are_independent_and_disabled_events_do_not_replay() {
        for disabled in 0..4 {
            let mut preferences = NotificationSettings::default();
            match disabled {
                0 => preferences.result_found = false,
                1 => preferences.search_completed = false,
                2 => preferences.automatic_pause = false,
                _ => preferences.automatic_resume = false,
            }
            let mut notifications = Notifications::default();
            for event in [
                Notification::ResultsFound { count: 1 },
                Notification::SearchCompleted {
                    message: "No matches.".into(),
                },
                Notification::AutomaticallyPaused {
                    reason: "Loading.".into(),
                },
                Notification::AutomaticallyResumed,
            ] {
                notifications.push(event, Duration::ZERO, &preferences);
            }
            assert_eq!(notifications.toasts.len(), 3);
            notifications.update(Duration::ZERO, &NotificationSettings::default());
            assert_eq!(notifications.toasts.len(), 3);
        }
    }

    #[test]
    fn result_bursts_coalesce_without_extending_lifetime_and_queue_is_bounded() {
        let preferences = NotificationSettings::default();
        let mut notifications = Notifications::default();
        notifications.push(
            Notification::ResultsFound { count: 2 },
            Duration::ZERO,
            &preferences,
        );
        notifications.push(
            Notification::ResultsFound { count: 3 },
            RESULT_BURST,
            &preferences,
        );
        assert_eq!(notifications.toasts.len(), 1);
        assert!(matches!(
            notifications.toasts[0].content,
            Content::Search(Notification::ResultsFound { count: 5 })
        ));
        notifications.update(LIFETIME - Duration::from_nanos(1), &preferences);
        assert!(!notifications.is_empty());
        notifications.update(LIFETIME, &preferences);
        assert!(notifications.is_empty());
        for _ in 0..100 {
            notifications.push(Notification::AutomaticallyResumed, LIFETIME, &preferences);
        }
        assert_eq!(notifications.toasts.len(), MAX_TOASTS);
    }

    #[test]
    fn batched_results_coalesce_by_occurrence_time_not_delivery_time() {
        let preferences = NotificationSettings::default();
        let mut notifications = Notifications::default();
        let delivered = Duration::from_secs(2);
        notifications.push_aged(
            Notification::ResultsFound { count: 1 },
            Duration::from_secs(2),
            delivered,
            &preferences,
        );
        notifications.push_aged(
            Notification::ResultsFound { count: 1 },
            Duration::from_secs(1),
            delivered,
            &preferences,
        );
        assert_eq!(notifications.toasts.len(), 2);
    }

    #[test]
    fn disabling_a_category_removes_visible_cards_without_affecting_export() {
        let mut preferences = NotificationSettings::default();
        let mut notifications = Notifications::default();
        notifications.push(
            Notification::ResultsFound { count: 1 },
            Duration::ZERO,
            &preferences,
        );
        notifications.exported("Results", "results.csv".into(), Duration::ZERO);
        preferences.result_found = false;
        notifications.update(Duration::ZERO, &preferences);
        assert_eq!(notifications.toasts.len(), 1);
        notifications.update(LIFETIME, &preferences);
        assert!(notifications.is_empty());
    }

    #[test]
    fn replacement_export_expires_from_its_own_completion() {
        let mut notifications = Notifications::default();
        notifications.exported("Results", "first.csv".into(), Duration::ZERO);
        notifications.exported("Search", "second.json".into(), LIFETIME);
        assert_eq!(notifications.toasts.len(), 1);
        assert!(matches!(
            notifications.toasts.front().map(|toast| &toast.content),
            Some(Content::Export {
                subject: "Search",
                path
            }) if path == "second.json"
        ));
        notifications.update(LIFETIME * 2, &NotificationSettings::default());
        assert!(notifications.is_empty());
    }

    #[test]
    fn delayed_delivery_does_not_replay_expired_events() {
        let mut notifications = Notifications::default();
        notifications.push_aged(
            Notification::AutomaticallyResumed,
            LIFETIME,
            Duration::ZERO,
            &NotificationSettings::default(),
        );
        assert!(notifications.is_empty());
        notifications.push_aged(
            Notification::AutomaticallyResumed,
            LIFETIME - Duration::from_nanos(1),
            Duration::ZERO,
            &NotificationSettings::default(),
        );
        assert!(!notifications.is_empty());
        notifications.update(Duration::from_nanos(1), &NotificationSettings::default());
        assert!(notifications.is_empty());
    }

    #[test]
    fn notification_storage_bounds_utf8_messages_without_splitting_characters() {
        let mut notifications = Notifications::default();
        notifications.push(
            Notification::SearchCompleted {
                message: "🪐".repeat(10_000),
            },
            Duration::ZERO,
            &NotificationSettings::default(),
        );
        let Content::Search(Notification::SearchCompleted { message }) =
            &notifications.toasts[0].content
        else {
            panic!("completion missing")
        };
        assert!(message.len() <= MAX_MESSAGE_BYTES);
        assert!(message.ends_with('…'));
    }

    #[test]
    fn all_placements_have_distinct_inset_anchors() {
        let anchors: Vec<_> = ToastPlacement::ALL
            .iter()
            .map(|&placement| placement_anchor(placement))
            .collect();
        for (index, (anchor, offset)) in anchors.iter().enumerate() {
            assert!(!anchors[..index].iter().any(|(other, _)| other == anchor));
            assert_eq!(offset.y.abs(), 16.0);
        }
    }
}
