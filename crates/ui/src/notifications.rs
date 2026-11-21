//! Transient overlay feedback. Runtime search events are supplied by the caller, never inferred
//! from UI snapshots. Explicit timestamps keep filtering, coalescing and expiry deterministic.

use crate::{
    portal_glyphs::PortalGlyphs,
    theme::{self, Palette},
};
use egui::{Align2, RichText, Vec2};
use search_probes_model::ui_control::ToastSnapshot;
use search_probes_model::{
    cache::CachedMatch,
    settings::{NotificationSettings, ResultCardSettings, SettingsRecovery, ToastPlacement},
    target::TargetKind,
};
use std::{collections::VecDeque, time::Duration};

const LIFETIME: Duration = Duration::from_secs(4);
const FADE_IN: Duration = Duration::from_millis(250);
const FADE_OUT: Duration = Duration::from_millis(350);
const MAX_TOASTS: usize = 3;
const MAX_CONTROLLED_TOASTS: usize = 16;
const MAX_CONTROL_ID_BYTES: usize = 128;
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
pub struct SearchProgress {
    pub systems: u64,
    pub system_limit: u64,
    pub planets: Option<u64>,
    pub fauna: Option<u64>,
    pub results: usize,
    pub result_limit: usize,
    pub elapsed_ms: u64,
}

impl SearchProgress {
    fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = Vec::with_capacity(5);
        rows.push((
            "Systems",
            format!("{} / {}", number(self.systems), number(self.system_limit)),
        ));
        if let Some(planets) = self.planets {
            rows.push(("Planets", number(planets)));
        }
        if let Some(fauna) = self.fauna {
            rows.push(("Fauna", number(fauna)));
        }
        rows.push((
            "Results",
            format!(
                "{} / {}",
                number(self.results.try_into().unwrap_or(u64::MAX)),
                number(self.result_limit.try_into().unwrap_or(u64::MAX)),
            ),
        ));
        rows.push(("Elapsed time", elapsed(self.elapsed_ms)));
        rows
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    ResultFound { result: CachedMatch },
    SearchCompleted { progress: SearchProgress },
    AutomaticallyPaused { keep_game_running: bool },
    AutomaticallyResumed { progress: SearchProgress },
    CompatibilityRecovered { recovery: SettingsRecovery },
}

impl Notification {
    fn pause_reasons(&self) -> Option<Vec<&'static str>> {
        let Self::AutomaticallyPaused { keep_game_running } = self else {
            return None;
        };
        let mut reasons = Vec::with_capacity(4);
        if !keep_game_running {
            reasons.push("An in-game menu is open");
            reasons.push("The game is unfocused");
        }
        reasons.push("The player is traveling between systems");
        reasons.push("The player is in a different galaxy than the search originated in");
        Some(reasons)
    }

    fn progress(&self) -> Option<&SearchProgress> {
        match self {
            Self::SearchCompleted { progress } | Self::AutomaticallyResumed { progress } => {
                Some(progress)
            }
            _ => None,
        }
    }

    fn recovery(&self) -> Option<SettingsRecovery> {
        match self {
            Self::CompatibilityRecovered { recovery } => Some(*recovery),
            _ => None,
        }
    }

    fn enabled(&self, preferences: &NotificationSettings) -> bool {
        match self {
            Self::ResultFound { .. } => preferences.result_found,
            Self::SearchCompleted { .. } => preferences.search_completed,
            Self::AutomaticallyPaused { .. } => preferences.automatic_pause,
            Self::AutomaticallyResumed { .. } => preferences.automatic_resume,
            Self::CompatibilityRecovered { .. } => true,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::ResultFound { .. } => "Result found",
            Self::SearchCompleted { .. } => "Search completed",
            Self::AutomaticallyPaused { .. } => "Search automatically paused",
            Self::AutomaticallyResumed { .. } => "Search automatically resumed",
            Self::CompatibilityRecovered { .. } => "Files updated",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::ResultFound { result } => result.title().unwrap_or("Name unavailable").to_owned(),
            Self::SearchCompleted { progress } | Self::AutomaticallyResumed { progress } => {
                progress
                    .rows()
                    .into_iter()
                    .map(|(label, value)| format!("• {label}: {value}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Self::AutomaticallyPaused { .. } => {
                format!(
                    "Search pauses while:\n{}",
                    self.pause_reasons()
                        .expect("pause notification has reasons")
                        .into_iter()
                        .map(|reason| format!("• {reason}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
            Self::CompatibilityRecovered { recovery } => recovery_message(*recovery),
        }
    }
}

fn recovery_subject(recovery: SettingsRecovery) -> &'static str {
    match (recovery.settings, recovery.preset_files) {
        (true, 0) => "settings.json file",
        (true, _) => "settings.json and preset files",
        (false, 1) => "preset file",
        (false, _) => "preset files",
    }
}

fn recovery_message(recovery: SettingsRecovery) -> String {
    let file_count = usize::from(recovery.settings) + recovery.preset_files;
    let suffix = if file_count == 1 { "File" } else { "Files" };
    format!(
        "Loaded incompatible {}. {suffix} updated to match current Search Probes version.",
        recovery_subject(recovery)
    )
}

#[derive(Debug)]
enum Content {
    Search(Notification),
    Export { subject: &'static str, path: String },
}

#[derive(Debug)]
struct Toast {
    control_id: Option<String>,
    content: Content,
    created: Duration,
    expires: Option<Duration>,
}

impl Toast {
    fn opacity(&self, now: Duration) -> f32 {
        let fade_in = now.saturating_sub(self.created).as_secs_f32() / FADE_IN.as_secs_f32();
        let fade_in = fade_in.clamp(0.0, 1.0);
        let fade_out = self.expires.map_or(1.0, |expires| {
            (expires.saturating_sub(now).as_secs_f32() / FADE_OUT.as_secs_f32()).clamp(0.0, 1.0)
        });
        fade_in.min(fade_out)
    }

    fn dismiss(&mut self, now: Duration) {
        let fade_end = now.saturating_add(FADE_OUT);
        self.expires = Some(
            self.expires
                .map_or(fade_end, |expires| expires.min(fade_end)),
        );
    }
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
            let visible = toast.expires.is_none_or(|expires| now < expires);
            visible
                && (toast.control_id.is_some()
                    || match &toast.content {
                        Content::Search(notification) => notification.enabled(preferences),
                        Content::Export { .. } => true,
                    })
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
        notification: Notification,
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
        self.append(
            None,
            Content::Search(notification),
            created,
            Some(now.saturating_add(remaining)),
        );
    }

    fn append(
        &mut self,
        control_id: Option<String>,
        content: Content,
        created: Duration,
        expires: Option<Duration>,
    ) {
        if control_id.is_none()
            && self
                .toasts
                .iter()
                .filter(|toast| toast.control_id.is_none())
                .count()
                == MAX_TOASTS
            && let Some(index) = self
                .toasts
                .iter()
                .position(|toast| toast.control_id.is_none())
        {
            self.toasts.remove(index);
        }
        self.toasts.push_back(Toast {
            control_id,
            content,
            created,
            expires,
        });
    }

    fn validate_control_id(id: &str) -> Result<(), String> {
        if id.is_empty() {
            return Err("Toast ID cannot be empty".into());
        }
        if id.len() > MAX_CONTROL_ID_BYTES {
            return Err(format!(
                "Toast ID cannot exceed {MAX_CONTROL_ID_BYTES} UTF-8 bytes"
            ));
        }
        Ok(())
    }

    fn replace_controlled(
        &mut self,
        id: String,
        content: Content,
        now: Duration,
    ) -> Result<(), String> {
        Self::validate_control_id(&id)?;
        self.toasts
            .retain(|toast| toast.control_id.as_deref() != Some(id.as_str()));
        if self
            .toasts
            .iter()
            .filter(|toast| toast.control_id.is_some())
            .count()
            == MAX_CONTROLLED_TOASTS
        {
            return Err(format!(
                "At most {MAX_CONTROLLED_TOASTS} controlled notifications may be shown"
            ));
        }
        self.append(Some(id), content, now, None);
        Ok(())
    }

    pub fn push_controlled(
        &mut self,
        id: String,
        notification: Notification,
        now: Duration,
    ) -> Result<(), String> {
        self.replace_controlled(id, Content::Search(notification), now)
    }

    pub fn exported_controlled(
        &mut self,
        id: String,
        subject: &'static str,
        mut path: String,
        now: Duration,
    ) -> Result<(), String> {
        bound_message(&mut path);
        self.replace_controlled(id, Content::Export { subject, path }, now)
    }

    pub fn dismiss_controlled(&mut self, id: &str, now: Duration) -> bool {
        let Some(toast) = self
            .toasts
            .iter_mut()
            .find(|toast| toast.control_id.as_deref() == Some(id))
        else {
            return false;
        };
        toast.dismiss(now);
        true
    }

    pub fn clear_export(&mut self) {
        self.toasts.retain(|toast| {
            toast.control_id.is_some() || !matches!(toast.content, Content::Export { .. })
        });
    }

    pub fn clear_search(&mut self) {
        self.toasts.retain(|toast| {
            toast.control_id.is_some() || matches!(toast.content, Content::Export { .. })
        });
    }

    pub fn clear(&mut self, now: Duration) {
        for toast in &mut self.toasts {
            toast.dismiss(now);
        }
    }

    pub fn snapshot(&self) -> Vec<ToastSnapshot> {
        self.toasts
            .iter()
            .map(|toast| {
                let (title, message) = Self::text(&toast.content);
                ToastSnapshot {
                    id: toast.control_id.clone(),
                    title,
                    message,
                    created_ms: toast.created.as_millis().min(u128::from(u64::MAX)) as u64,
                    expires_ms: toast
                        .expires
                        .map(|expires| expires.as_millis().min(u128::from(u64::MAX)) as u64),
                }
            })
            .collect()
    }

    pub fn exported(&mut self, subject: &'static str, mut path: String, now: Duration) {
        bound_message(&mut path);
        self.clear_export();
        self.append(
            None,
            Content::Export { subject, path },
            now,
            Some(now.saturating_add(LIFETIME)),
        );
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        now: Duration,
        palette: Palette,
        placement: ToastPlacement,
        result_cards: ResultCardSettings,
        portal_glyphs: &PortalGlyphs,
    ) {
        if self.is_empty() {
            return;
        }
        let (anchor, offset) = placement_anchor(placement);
        let screen = ctx.content_rect();
        let width = (screen.width() - 32.0).clamp(120.0, 340.0);
        // Keep the stack inside short viewports; all retained cards continue to expire normally.
        let maximum = ((screen.height() - 32.0) / 220.0).floor().max(1.0) as usize;
        egui::Area::new(egui::Id::new("search_probes_notifications"))
            .order(egui::Order::Foreground)
            .anchor(anchor, offset)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_width(width);
                let indices: Vec<_> = (0..self.toasts.len()).rev().take(maximum).collect();
                for index in indices {
                    let toast = &self.toasts[index];
                    let opacity = toast.opacity(now);
                    if opacity < 1.0 {
                        ctx.request_repaint_after(Duration::from_millis(16));
                    } else if let Some(expires) = toast.expires {
                        let fade_starts = expires.saturating_sub(FADE_OUT);
                        ctx.request_repaint_after(fade_starts.saturating_sub(now));
                    }
                    let (title, message) = Self::text(&toast.content);
                    let result = match &toast.content {
                        Content::Search(Notification::ResultFound { result }) => Some(result),
                        _ => None,
                    };
                    let progress = match &toast.content {
                        Content::Search(notification) => notification.progress(),
                        _ => None,
                    };
                    let pause_reasons = match &toast.content {
                        Content::Search(notification) => notification.pause_reasons(),
                        _ => None,
                    };
                    let recovery = match &toast.content {
                        Content::Search(notification) => notification.recovery(),
                        _ => None,
                    };
                    let (title_fill, title_text) = match &toast.content {
                        Content::Search(
                            Notification::ResultFound { .. } | Notification::SearchCompleted { .. },
                        ) => (palette.success, palette.foreground_on(palette.success)),
                        Content::Search(
                            Notification::AutomaticallyPaused { .. }
                            | Notification::AutomaticallyResumed { .. }
                            | Notification::CompatibilityRecovered { .. },
                        ) => (palette.warning, palette.foreground_on(palette.warning)),
                        Content::Export { .. } => (palette.link, palette.background),
                    };
                    let mut dismissed = false;
                    ui.scope(|ui| {
                        ui.set_opacity(opacity);
                        egui::Frame::new()
                            .fill(palette.surface)
                            .stroke(egui::Stroke::new(1.0, palette.outline))
                            .corner_radius(6.0)
                            .show(ui, |ui| {
                                ui.set_width(width);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                egui::Frame::new()
                                    .fill(title_fill)
                                    .corner_radius(5.0)
                                    .inner_margin(egui::Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.set_width((width - 24.0).max(1.0));
                                        let title_rect = ui.available_rect_before_wrap();
                                        let title_rect = egui::Rect::from_min_size(
                                            title_rect.min,
                                            egui::vec2(title_rect.width(), 24.0),
                                        );
                                        ui.allocate_rect(title_rect, egui::Sense::hover());
                                        let mut title_ui = ui.new_child(
                                            egui::UiBuilder::new()
                                                .max_rect(title_rect)
                                                .layout(egui::Layout::centered_and_justified(
                                                    egui::Direction::LeftToRight,
                                                )),
                                        );
                                        title_ui.label(
                                                RichText::new(title)
                                                    .size(if result.is_some() {
                                                        21.0
                                                    } else {
                                                        17.0
                                                    })
                                                    .strong()
                                                    .color(title_text),
                                            );
                                        let close_size = egui::vec2(18.0, 18.0);
                                        let close_rect = egui::Rect::from_center_size(
                                            egui::pos2(
                                                title_rect.right() - close_size.x / 2.0,
                                                title_rect.center().y,
                                            ),
                                            close_size,
                                        );
                                        let close = egui::Image::from_bytes(
                                            "bytes://search-probes/icons/close.svg",
                                            include_bytes!("../assets/icons/close.svg"),
                                        )
                                        .fit_to_exact_size(egui::vec2(13.0, 13.0));
                                        let response = ui
                                            .put(
                                                close_rect,
                                                egui::Button::image(close)
                                                    .image_tint_follows_text_color(true)
                                                    .frame(false),
                                            )
                                            .on_hover_text("Dismiss notification");
                                        response.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::WidgetType::Button,
                                                true,
                                                "Dismiss notification",
                                            )
                                        });
                                        dismissed = response.clicked();
                                    });
                                egui::Frame::new()
                                    .fill(palette.surface)
                                    .corner_radius(5.0)
                                    .inner_margin(12.0)
                                    .show(ui, |ui| {
                                        ui.set_width((width - 24.0).max(1.0));
                                        if let Some(result) = result {
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    RichText::new(
                                                        result
                                                            .title()
                                                            .unwrap_or("Name unavailable"),
                                                    )
                                                    .size(17.0)
                                                    .strong()
                                                    .color(palette.text),
                                                );
                                                if result_cards.location_subtitle {
                                                    ui.label(
                                                        RichText::new(result_subtitle(result))
                                                            .size(14.0)
                                                            .color(palette.secondary_text),
                                                    );
                                                }
                                                if result_cards.portal_glyphs {
                                                    let portal_code = result.target().portal_code();
                                                    portal_glyphs.show_centered(
                                                        ui,
                                                        &portal_code,
                                                        palette.secondary_text,
                                                    );
                                                }
                                            });
                                        } else if let Some(progress) = progress {
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                for (label, value) in progress.rows() {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(
                                                            RichText::new("•")
                                                                .size(15.0)
                                                                .color(palette.text),
                                                        );
                                                        ui.horizontal(|ui| {
                                                            ui.spacing_mut().item_spacing.x = 0.0;
                                                            theme::bold_label(
                                                                ui,
                                                                label,
                                                                15.0,
                                                                palette.text,
                                                            );
                                                            ui.label(
                                                                RichText::new(format!(": {value}"))
                                                                    .size(15.0)
                                                                    .color(palette.text),
                                                            );
                                                        });
                                                    });
                                                }
                                            });
                                        } else if let Some(reasons) = pause_reasons {
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                ui.label(
                                                    RichText::new("Search pauses while:")
                                                        .size(15.0)
                                                        .color(palette.text),
                                                );
                                                for reason in reasons {
                                                    ui.horizontal_top(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        ui.label(
                                                            RichText::new("•")
                                                                .size(15.0)
                                                                .color(palette.text),
                                                        );
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(reason)
                                                                    .size(15.0)
                                                                    .color(palette.text),
                                                            )
                                                            .wrap(),
                                                        );
                                                    });
                                                }
                                            });
                                        } else if let Some(recovery) = recovery {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().item_spacing.x = 3.0;
                                                ui.label(
                                                    RichText::new("Loaded incompatible")
                                                        .size(15.0)
                                                        .color(palette.text),
                                                );
                                                if recovery.settings {
                                                    ui.label(
                                                        RichText::new("settings.json")
                                                            .code()
                                                            .size(15.0)
                                                            .color(palette.text),
                                                    );
                                                    if recovery.preset_files > 0 {
                                                        ui.label(
                                                            RichText::new("and preset files.")
                                                                .size(15.0)
                                                                .color(palette.text),
                                                        );
                                                    } else {
                                                        ui.label(
                                                        RichText::new("file.")
                                                                .size(15.0)
                                                                .color(palette.text),
                                                        );
                                                    }
                                                } else {
                                                    let subject = if recovery.preset_files == 1 {
                                                        "preset file."
                                                    } else {
                                                        "preset files."
                                                    };
                                                    ui.label(
                                                        RichText::new(subject)
                                                            .size(15.0)
                                                            .color(palette.text),
                                                    );
                                                }
                                                let file_count = usize::from(recovery.settings)
                                                    + recovery.preset_files;
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} updated to match current Search Probes version.",
                                                        if file_count == 1 { "File" } else { "Files" }
                                                    ))
                                                    .size(15.0)
                                                    .color(palette.text),
                                                );
                                            });
                                        } else {
                                            let mut job = egui::text::LayoutJob::simple(
                                                message,
                                                egui::FontId::proportional(15.0),
                                                palette.text,
                                                width - 24.0,
                                            );
                                            job.wrap.max_rows = 8;
                                            ui.label(job);
                                        }
                                    });
                            });
                    });
                    if dismissed && let Some(toast) = self.toasts.get_mut(index) {
                        toast.dismiss(now);
                    }
                }
            });
    }
}

fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

fn elapsed(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    let hours = seconds / 3_600;
    let minutes = seconds / 60 % 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub(crate) fn result_subtitle(result: &CachedMatch) -> String {
    let system = result.system_name.as_deref().unwrap_or("name unavailable");
    let planet = result.planet_name.as_deref().unwrap_or("name unavailable");
    let galaxy = u16::from(result.address.coordinates().galaxy) + 1;
    match result.kind {
        TargetKind::System => format!("System · Galaxy {galaxy}"),
        TargetKind::Planet => format!("Planet · System {system} · Galaxy {galaxy}"),
        TargetKind::Fauna => {
            format!("Animal · Planet {planet} · System {system} · Galaxy {galaxy}")
        }
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
    use search_probes_model::address::{Address, Coordinates};

    fn result(system: u16) -> CachedMatch {
        CachedMatch {
            address: Address::new(Coordinates {
                x: 0,
                y: 0,
                z: 0,
                galaxy: 0,
                system,
                planet: 1,
            })
            .unwrap(),
            kind: TargetKind::Planet,
            system_name: Some(format!("Fixture System {system}").into()),
            planet_name: Some(format!("Fixture Planet {system}").into()),
            fauna: None,
        }
    }

    fn progress() -> SearchProgress {
        SearchProgress {
            systems: 2_400,
            system_limit: 10_000,
            planets: Some(13_842),
            fauna: Some(6_125),
            results: 4,
            result_limit: 5,
            elapsed_ms: 83_000,
        }
    }

    fn completed() -> Notification {
        Notification::SearchCompleted {
            progress: progress(),
        }
    }

    fn resumed() -> Notification {
        Notification::AutomaticallyResumed {
            progress: progress(),
        }
    }

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
                Notification::ResultFound { result: result(1) },
                completed(),
                Notification::AutomaticallyPaused {
                    keep_game_running: false,
                },
                resumed(),
            ] {
                notifications.push(event, Duration::ZERO, &preferences);
            }
            assert_eq!(notifications.toasts.len(), 3);
            notifications.update(Duration::ZERO, &NotificationSettings::default());
            assert_eq!(notifications.toasts.len(), 3);
        }
    }

    #[test]
    fn result_notifications_remain_distinct_and_the_runtime_queue_is_bounded() {
        let preferences = NotificationSettings::default();
        let mut notifications = Notifications::default();
        notifications.push(
            Notification::ResultFound { result: result(1) },
            Duration::ZERO,
            &preferences,
        );
        notifications.push(
            Notification::ResultFound { result: result(2) },
            Duration::from_millis(750),
            &preferences,
        );
        assert_eq!(notifications.toasts.len(), 2);
        notifications.update(LIFETIME - Duration::from_nanos(1), &preferences);
        assert!(!notifications.is_empty());
        notifications.update(LIFETIME + Duration::from_secs(1), &preferences);
        assert!(notifications.is_empty());
        for _ in 0..100 {
            notifications.push(resumed(), LIFETIME, &preferences);
        }
        assert_eq!(notifications.toasts.len(), MAX_TOASTS);
    }

    #[test]
    fn batched_results_keep_distinct_cards() {
        let preferences = NotificationSettings::default();
        let mut notifications = Notifications::default();
        let delivered = Duration::from_secs(2);
        notifications.push_aged(
            Notification::ResultFound { result: result(1) },
            Duration::from_secs(2),
            delivered,
            &preferences,
        );
        notifications.push_aged(
            Notification::ResultFound { result: result(2) },
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
            Notification::ResultFound { result: result(1) },
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
            resumed(),
            LIFETIME,
            Duration::ZERO,
            &NotificationSettings::default(),
        );
        assert!(notifications.is_empty());
        notifications.push_aged(
            resumed(),
            LIFETIME - Duration::from_nanos(1),
            Duration::ZERO,
            &NotificationSettings::default(),
        );
        assert!(!notifications.is_empty());
        notifications.update(Duration::from_nanos(1), &NotificationSettings::default());
        assert!(notifications.is_empty());
    }

    #[test]
    fn completion_notification_is_a_compact_metrics_breakdown() {
        assert_eq!(
            completed().message(),
            "• Systems: 2,400 / 10,000\n\
             • Planets: 13,842\n\
             • Fauna: 6,125\n\
             • Results: 4 / 5\n\
             • Elapsed time: 00:01:23"
        );
    }

    #[test]
    fn progress_breakdown_omits_scopes_the_search_did_not_use() {
        let notification = Notification::AutomaticallyResumed {
            progress: SearchProgress {
                planets: None,
                fauna: None,
                ..progress()
            },
        };
        assert_eq!(
            notification.message(),
            "• Systems: 2,400 / 10,000\n\
             • Results: 4 / 5\n\
             • Elapsed time: 00:01:23"
        );
    }

    #[test]
    fn pause_explanation_reflects_keep_game_running() {
        assert!(
            Notification::AutomaticallyPaused {
                keep_game_running: false
            }
            .message()
            .contains("in-game menu")
        );
        assert!(
            !Notification::AutomaticallyPaused {
                keep_game_running: true
            }
            .message()
            .contains("in-game menu")
        );
    }

    #[test]
    fn compatibility_recovery_is_mandatory_and_uses_dynamic_file_wording() {
        let mut notifications = Notifications::default();
        let preferences = NotificationSettings {
            result_found: false,
            search_completed: false,
            automatic_pause: false,
            automatic_resume: false,
            ..NotificationSettings::default()
        };
        notifications.push(
            Notification::CompatibilityRecovered {
                recovery: SettingsRecovery {
                    settings: true,
                    preset_files: 0,
                },
            },
            Duration::ZERO,
            &preferences,
        );
        notifications.update(Duration::ZERO, &preferences);
        assert_eq!(notifications.toasts.len(), 1);
        assert_eq!(
            notifications.snapshot()[0].message,
            "Loaded incompatible settings.json file. File updated to match current Search Probes version."
        );
        assert_eq!(
            recovery_message(SettingsRecovery {
                settings: false,
                preset_files: 2,
            }),
            "Loaded incompatible preset files. Files updated to match current Search Probes version."
        );
        assert_eq!(
            recovery_message(SettingsRecovery {
                settings: true,
                preset_files: 1,
            }),
            "Loaded incompatible settings.json and preset files. Files updated to match current Search Probes version."
        );
    }

    #[test]
    fn notification_opacity_fades_in_and_out() {
        let mut toast = Toast {
            control_id: Some("fade".into()),
            content: Content::Search(resumed()),
            created: Duration::ZERO,
            expires: None,
        };
        assert_eq!(toast.opacity(Duration::ZERO), 0.0);
        assert!((toast.opacity(FADE_IN / 2) - 0.5).abs() < 0.01);
        assert_eq!(toast.opacity(FADE_IN), 1.0);

        let dismissed = Duration::from_secs(1);
        toast.dismiss(dismissed);
        assert_eq!(toast.opacity(dismissed), 1.0);
        assert!((toast.opacity(dismissed + FADE_OUT / 2) - 0.5).abs() < 0.01);
        assert_eq!(toast.opacity(dismissed + FADE_OUT), 0.0);
    }

    #[test]
    fn controlled_notifications_persist_until_their_exact_id_is_dismissed() {
        let mut notifications = Notifications::default();
        notifications
            .push_controlled(
                "review-result".into(),
                Notification::ResultFound { result: result(1) },
                Duration::ZERO,
            )
            .unwrap();

        let preferences = NotificationSettings {
            result_found: false,
            ..NotificationSettings::default()
        };
        notifications.update(Duration::from_secs(60), &preferences);

        let snapshot = notifications.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].id.as_deref(), Some("review-result"));
        assert_eq!(snapshot[0].expires_ms, None);
        let dismissed = Duration::from_secs(60);
        assert!(!notifications.dismiss_controlled("another-id", dismissed));
        assert!(notifications.dismiss_controlled("review-result", dismissed));
        assert!(!notifications.is_empty());
        notifications.update(dismissed + FADE_OUT, &preferences);
        assert!(notifications.is_empty());
    }

    #[test]
    fn controlled_export_is_not_removed_by_runtime_export_cleanup() {
        let mut notifications = Notifications::default();
        notifications
            .exported_controlled(
                "review-export".into(),
                "Results",
                "results.csv".into(),
                Duration::ZERO,
            )
            .unwrap();
        notifications.clear_export();
        assert_eq!(
            notifications.snapshot()[0].id.as_deref(),
            Some("review-export")
        );
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
