use serde::{Deserialize, Serialize};

pub const MAX_CUSTOM_THEMES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePalette {
    pub dark: bool,
    pub background: ThemeColor,
    pub surface: ThemeColor,
    pub surface_container: ThemeColor,
    pub raised_surface: ThemeColor,
    pub text: ThemeColor,
    pub secondary_text: ThemeColor,
    pub primary: ThemeColor,
    pub primary_container: ThemeColor,
    pub primary_container_text: ThemeColor,
    pub selection: ThemeColor,
    pub selection_text: ThemeColor,
    pub link: ThemeColor,
    pub outline: ThemeColor,
    pub error: ThemeColor,
    pub success: ThemeColor,
    pub success_container: ThemeColor,
    pub success_outline: ThemeColor,
    pub warning: ThemeColor,
    pub warning_container: ThemeColor,
    pub warning_outline: ThemeColor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomTheme {
    pub name: String,
    pub palette: ThemePalette,
}

impl CustomTheme {
    pub fn validate_palette(&self) -> Result<(), &'static str> {
        let palette = &self.palette;
        for (foreground, background) in [
            (palette.text, palette.background),
            (palette.text, palette.surface),
            (palette.secondary_text, palette.background),
            (palette.secondary_text, palette.surface),
            (palette.primary_container_text, palette.primary_container),
            (palette.selection_text, palette.selection),
            (palette.link, palette.background),
            (palette.error, palette.background),
            (palette.success, palette.success_container),
            (palette.warning, palette.warning_container),
        ] {
            if contrast(foreground, background) < 4.5 {
                return Err("custom theme text colors must meet accessible contrast");
            }
        }
        if contrast(palette.outline, palette.background) < 3.0 {
            return Err("custom theme outline must be distinguishable from the background");
        }
        Ok(())
    }
}

fn luminance(color: ThemeColor) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
}

fn contrast(first: ThemeColor, second: ThemeColor) -> f64 {
    let first = luminance(first);
    let second = luminance(second);
    let (light, dark) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    (light + 0.05) / (dark + 0.05)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ThemeSelection {
    BuiltIn { theme: UiTheme },
    Custom { name: String },
}

impl Default for ThemeSelection {
    fn default() -> Self {
        Self::BuiltIn {
            theme: UiTheme::default(),
        }
    }
}

impl ThemeSelection {
    pub fn label<'a>(&'a self, custom_themes: &'a [CustomTheme]) -> &'a str {
        match self {
            Self::BuiltIn { theme } => theme.label(),
            Self::Custom { name } => custom_themes
                .iter()
                .find(|custom| custom.name == *name)
                .map_or(name.as_str(), |custom| custom.name.as_str()),
        }
    }

    pub fn built_in(&self) -> Option<UiTheme> {
        match self {
            Self::BuiltIn { theme } => Some(*theme),
            Self::Custom { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTheme {
    #[default]
    SearchProbesDark,
    SearchProbesLight,
    AtomOneDark,
    AtomOneLight,
    AyuDark,
    AyuLight,
    AyuMirage,
    BreezeDark,
    BreezeLight,
    CatppuccinFrappe,
    CatppuccinLatte,
    CatppuccinMacchiato,
    CatppuccinMocha,
    Dracula,
    Falcon,
    GitHubDark,
    GitHubLight,
    GruvboxDark,
    GruvboxLight,
    HighContrastDark,
    HighContrastLight,
    Homunculus,
    MaterialDark,
    MaterialLight,
    Monokai,
    NoMansSky,
    Nord,
    Oblivion,
    Radical,
    SolarizedDark,
    SolarizedLight,
    TokyoNight,
    TokyoNightLight,
    TokyoNightStorm,
    VimDark,
    VscodiumDark,
}

impl UiTheme {
    pub const ALL: &[Self] = &[
        Self::SearchProbesDark,
        Self::SearchProbesLight,
        Self::AtomOneDark,
        Self::AtomOneLight,
        Self::AyuDark,
        Self::AyuLight,
        Self::AyuMirage,
        Self::BreezeDark,
        Self::BreezeLight,
        Self::CatppuccinFrappe,
        Self::CatppuccinLatte,
        Self::CatppuccinMacchiato,
        Self::CatppuccinMocha,
        Self::Dracula,
        Self::Falcon,
        Self::GitHubDark,
        Self::GitHubLight,
        Self::GruvboxDark,
        Self::GruvboxLight,
        Self::HighContrastDark,
        Self::HighContrastLight,
        Self::Homunculus,
        Self::MaterialDark,
        Self::MaterialLight,
        Self::Monokai,
        Self::NoMansSky,
        Self::Nord,
        Self::Oblivion,
        Self::Radical,
        Self::SolarizedDark,
        Self::SolarizedLight,
        Self::TokyoNight,
        Self::TokyoNightLight,
        Self::TokyoNightStorm,
        Self::VimDark,
        Self::VscodiumDark,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::SearchProbesDark => "Search Probes Dark",
            Self::SearchProbesLight => "Search Probes Light",
            Self::AtomOneDark => "Atom One Dark",
            Self::AtomOneLight => "Atom One Light",
            Self::AyuDark => "Ayu Dark",
            Self::AyuLight => "Ayu Light",
            Self::AyuMirage => "Ayu Mirage",
            Self::BreezeDark => "Breeze Dark",
            Self::BreezeLight => "Breeze Light",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Dracula => "Dracula",
            Self::Falcon => "Falcon",
            Self::GitHubDark => "GitHub Dark",
            Self::GitHubLight => "GitHub Light",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::HighContrastDark => "High Contrast Dark",
            Self::HighContrastLight => "High Contrast Light",
            Self::Homunculus => "Homunculus",
            Self::MaterialDark => "Material Dark",
            Self::MaterialLight => "Material Light",
            Self::Monokai => "Monokai",
            Self::NoMansSky => "No Man's Sky",
            Self::Nord => "Nord",
            Self::Oblivion => "Oblivion",
            Self::Radical => "Radical",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::TokyoNight => "Tokyo Night",
            Self::TokyoNightLight => "Tokyo Night Light",
            Self::TokyoNightStorm => "Tokyo Night Storm",
            Self::VimDark => "Vim Dark",
            Self::VscodiumDark => "VSCodium Dark",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(value: u8) -> ThemeColor {
        ThemeColor {
            red: value,
            green: value,
            blue: value,
        }
    }

    #[test]
    fn theme_palette_accepts_only_the_current_role_names() {
        let palette = ThemePalette {
            dark: true,
            background: color(1),
            surface: color(2),
            surface_container: color(3),
            raised_surface: color(4),
            text: color(5),
            secondary_text: color(6),
            primary: color(7),
            primary_container: color(8),
            primary_container_text: color(9),
            selection: color(10),
            selection_text: color(11),
            link: color(12),
            outline: color(13),
            error: color(14),
            success: color(15),
            success_container: color(16),
            success_outline: color(17),
            warning: color(18),
            warning_container: color(19),
            warning_outline: color(20),
        };
        let encoded = serde_json::to_value(&palette).unwrap();
        assert_eq!(
            serde_json::from_value::<ThemePalette>(encoded.clone()).unwrap(),
            palette
        );

        for (current, removed) in [
            ("raised_surface", "surface_container_high"),
            ("text", "on_surface"),
            ("secondary_text", "on_surface_variant"),
            ("primary_container_text", "on_primary_container"),
            ("selection_text", "on_selection"),
        ] {
            let mut stale = encoded.clone();
            let object = stale.as_object_mut().unwrap();
            let value = object.remove(current).unwrap();
            object.insert(removed.to_owned(), value);
            assert!(
                serde_json::from_value::<ThemePalette>(stale).is_err(),
                "removed role {removed} was accepted"
            );
        }
    }

    #[test]
    fn search_probes_themes_are_pinned_then_the_catalog_is_alphabetical() {
        assert_eq!(
            UiTheme::ALL[..2],
            [UiTheme::SearchProbesDark, UiTheme::SearchProbesLight]
        );
        assert!(
            UiTheme::ALL[2..]
                .windows(2)
                .all(|pair| pair[0].label().to_lowercase() <= pair[1].label().to_lowercase())
        );
    }
}
