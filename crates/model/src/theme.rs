use serde::{Deserialize, Serialize};

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
