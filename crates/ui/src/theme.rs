//! The complete Search Probes UI color inventory.
//!
//! Components consume semantic roles from [`Palette`]; raw colors belong only in this module.
//! Published palettes are adapted to those roles instead of leaking palette-specific names into
//! the UI. Search Probes Dark retains the colors used before theme support was introduced.

use egui::{self, Color32, Stroke};
use search_probes_model::theme::UiTheme;

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub background: Color32,
    pub surface: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub selection: Color32,
    pub on_selection: Color32,
    pub link: Color32,
    pub outline: Color32,
    pub error: Color32,
    pub success: Color32,
    pub success_container: Color32,
    pub success_outline: Color32,
    pub warning: Color32,
    pub warning_container: Color32,
    pub warning_outline: Color32,
}

const fn rgb(red: u8, green: u8, blue: u8) -> Color32 {
    Color32::from_rgb(red, green, blue)
}

const fn hex(value: u32) -> Color32 {
    rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn mix(first: Color32, second: Color32, amount: f32) -> Color32 {
    let channel = |first: u8, second: u8| {
        (f32::from(first) + (f32::from(second) - f32::from(first)) * amount).round() as u8
    };
    rgb(
        channel(first.r(), second.r()),
        channel(first.g(), second.g()),
        channel(first.b(), second.b()),
    )
}

fn luminance(color: Color32) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

fn contrast(first: Color32, second: Color32) -> f64 {
    let (light, dark) = match luminance(first).partial_cmp(&luminance(second)) {
        Some(std::cmp::Ordering::Greater) => (luminance(first), luminance(second)),
        _ => (luminance(second), luminance(first)),
    };
    (light + 0.05) / (dark + 0.05)
}

fn ensure_contrast(color: Color32, backgrounds: &[Color32], target: f64) -> Color32 {
    if backgrounds
        .iter()
        .all(|background| contrast(color, *background) >= target)
    {
        return color;
    }
    let black = Color32::BLACK;
    let white = Color32::WHITE;
    let endpoint = if backgrounds
        .iter()
        .map(|background| contrast(white, *background))
        .fold(f64::INFINITY, f64::min)
        >= backgrounds
            .iter()
            .map(|background| contrast(black, *background))
            .fold(f64::INFINITY, f64::min)
    {
        white
    } else {
        black
    };
    (1..=100)
        .map(|step| mix(color, endpoint, step as f32 / 100.0))
        .find(|candidate| {
            backgrounds
                .iter()
                .all(|background| contrast(*candidate, *background) >= target)
        })
        .unwrap_or(endpoint)
}

fn resolve_source(value: u32, background: Color32) -> Color32 {
    if value <= 0x00ff_ffff {
        return hex(value);
    }
    let alpha = ((value >> 24) & 0xff) as f32 / 255.0;
    mix(background, hex(value & 0x00ff_ffff), alpha)
}

#[allow(clippy::too_many_arguments)]
fn editor_palette(
    dark: bool,
    background: u32,
    surface: u32,
    selection: u32,
    text: u32,
    muted: u32,
    primary: u32,
    link: u32,
    error: u32,
    success: u32,
    warning: u32,
) -> Palette {
    let background = hex(background);
    let surface = resolve_source(surface, background);
    let selection = resolve_source(selection, background);
    let surface_container = mix(background, surface, 0.72);
    let surface_container_high = selection;
    let on_surface = ensure_contrast(hex(text), &[background, surface], 4.5);
    let on_surface_variant = ensure_contrast(hex(muted), &[background, surface], 4.5);
    let primary = hex(primary);
    let on_primary_container = ensure_contrast(on_surface, &[selection], 4.5);
    let on_selection = ensure_contrast(on_surface, &[selection], 4.5);
    let link = ensure_contrast(hex(link), &[background], 4.5);
    let outline = ensure_contrast(hex(muted), &[background], 3.0);
    let error_container = mix(background, hex(error), 0.12);
    let success_container = mix(background, hex(success), 0.12);
    let warning_container = mix(background, hex(warning), 0.12);
    let error = ensure_contrast(hex(error), &[background, error_container], 4.5);
    let success = ensure_contrast(hex(success), &[success_container], 4.5);
    let warning = ensure_contrast(hex(warning), &[warning_container], 4.5);
    Palette {
        dark,
        background,
        surface,
        surface_container,
        surface_container_high,
        on_surface,
        on_surface_variant,
        primary,
        primary_container: selection,
        on_primary_container,
        selection,
        on_selection,
        link,
        outline,
        error,
        success,
        success_container,
        success_outline: success,
        warning,
        warning_container,
        warning_outline: warning,
    }
}

impl Palette {
    pub fn for_theme(theme: UiTheme) -> Self {
        match theme {
            UiTheme::SearchProbesDark => Self {
                dark: true,
                background: rgb(18, 24, 32),
                surface: rgb(24, 34, 44),
                surface_container: rgb(31, 41, 51),
                surface_container_high: rgb(38, 48, 58),
                on_surface: rgb(241, 244, 246),
                on_surface_variant: rgb(145, 180, 190),
                primary: rgb(32, 108, 126),
                primary_container: rgb(38, 48, 58),
                on_primary_container: rgb(211, 244, 251),
                selection: rgb(32, 108, 126),
                on_selection: rgb(255, 255, 255),
                link: rgb(90, 170, 255),
                outline: rgb(95, 108, 120),
                error: rgb(255, 143, 143),
                success: rgb(105, 220, 140),
                success_container: rgb(16, 42, 28),
                success_outline: rgb(55, 155, 92),
                warning: rgb(240, 190, 90),
                warning_container: rgb(45, 35, 18),
                warning_outline: rgb(195, 145, 55),
            },
            UiTheme::SearchProbesLight => Self {
                dark: false,
                background: rgb(243, 247, 248),
                surface: rgb(255, 255, 255),
                surface_container: rgb(227, 236, 239),
                surface_container_high: rgb(215, 227, 231),
                on_surface: rgb(20, 34, 41),
                on_surface_variant: rgb(66, 94, 104),
                primary: rgb(32, 108, 126),
                primary_container: rgb(199, 233, 240),
                on_primary_container: rgb(11, 56, 67),
                selection: rgb(32, 108, 126),
                on_selection: rgb(255, 255, 255),
                link: rgb(20, 91, 146),
                outline: rgb(94, 116, 124),
                error: rgb(179, 38, 30),
                success: rgb(20, 108, 46),
                success_container: rgb(215, 248, 217),
                success_outline: rgb(46, 125, 50),
                warning: rgb(122, 79, 0),
                warning_container: rgb(255, 224, 163),
                warning_outline: rgb(141, 91, 0),
            },
            UiTheme::AtomOneDark => editor_palette(
                true, 0x282c34, 0x0a99bbff, 0x363c4a, 0xabb2bf, 0x5c6370, 0x61afef, 0xc678dd,
                0xf44747, 0x98c379, 0xd19a66,
            ),
            UiTheme::AtomOneLight => editor_palette(
                false, 0xfafafa, 0x0c383a42, 0xe5e5e6, 0x383a42, 0xa0a1a7, 0x4078f2, 0xa626a4,
                0xf44747, 0x50a14f, 0xce7c2b,
            ),
            UiTheme::AyuDark => editor_palette(
                true, 0x0a0e14, 0x00010a, 0x273747, 0xb3b1ad, 0x626a73, 0xffb454, 0xff8f40,
                0xff3333, 0x91b362, 0xc2d94c,
            ),
            UiTheme::AyuLight => editor_palette(
                false, 0xfafafa, 0xeff0f2, 0xd1e4f4, 0x575f66, 0x607880, 0xf2ae49, 0xfa8d3e,
                0xf51818, 0x99bf4d, 0x86b300,
            ),
            UiTheme::AyuMirage => editor_palette(
                true, 0x1f2430, 0x191e2a, 0x33415e, 0xcbccc6, 0x5c6773, 0xffd580, 0xffa759,
                0xff3333, 0xa6cc70, 0xbae67e,
            ),
            UiTheme::BreezeDark => editor_palette(
                true, 0x232629, 0x2a2e32, 0x2d5c76, 0xcfcfc2, 0x7a7c7d, 0x8e44ad, 0x2980b9,
                0xda4453, 0x1c8042, 0xf67400,
            ),
            UiTheme::BreezeLight => editor_palette(
                false, 0xffffff, 0xf8f7f6, 0x94caef, 0x1f1c1b, 0x898887, 0x644a9b, 0x0057ae,
                0xbf0303, 0x2ecc71, 0xfdbc4b,
            ),
            UiTheme::CatppuccinFrappe => editor_palette(
                true, 0x303446, 0x414559, 0x8c51576d, 0xc6d0f5, 0x737994, 0x8caaee, 0xca9ee6,
                0xe78284, 0xa6d189, 0xef9f76,
            ),
            UiTheme::CatppuccinLatte => editor_palette(
                false, 0xeff1f5, 0xccd0da, 0x8cbcc0cc, 0x4c4f69, 0x9ca0b0, 0x1e66f5, 0x8839ef,
                0xd20f39, 0x40a02b, 0xfe640b,
            ),
            UiTheme::CatppuccinMacchiato => editor_palette(
                true, 0x24273a, 0x363a4f, 0x8c494d64, 0xcad3f5, 0x6e738d, 0x8aadf4, 0xc6a0f6,
                0xed8796, 0xa6da95, 0xf5a97f,
            ),
            UiTheme::CatppuccinMocha => editor_palette(
                true, 0x1e1e2e, 0x66313244, 0x8c45475a, 0xcdd6f4, 0x6c7086, 0x89b4fa, 0xcba6f7,
                0xf38ba8, 0xa6e3a1, 0xfab387,
            ),
            UiTheme::MaterialDark => Self {
                dark: true,
                background: rgb(28, 27, 31),
                surface: rgb(33, 31, 38),
                surface_container: rgb(39, 37, 43),
                surface_container_high: rgb(43, 41, 48),
                on_surface: rgb(230, 225, 229),
                on_surface_variant: rgb(202, 196, 208),
                primary: rgb(208, 188, 255),
                primary_container: rgb(79, 55, 139),
                on_primary_container: rgb(234, 221, 255),
                selection: rgb(79, 55, 139),
                on_selection: rgb(234, 221, 255),
                link: rgb(208, 188, 255),
                outline: rgb(147, 143, 153),
                error: rgb(242, 184, 181),
                success: rgb(139, 219, 161),
                success_container: rgb(18, 53, 31),
                success_outline: rgb(96, 169, 120),
                warning: rgb(242, 198, 109),
                warning_container: rgb(59, 47, 18),
                warning_outline: rgb(199, 155, 66),
            },
            UiTheme::MaterialLight => Self {
                dark: false,
                background: rgb(255, 251, 254),
                surface: rgb(247, 242, 250),
                surface_container: rgb(243, 237, 247),
                surface_container_high: rgb(236, 230, 240),
                on_surface: rgb(28, 27, 31),
                on_surface_variant: rgb(73, 69, 79),
                primary: rgb(103, 80, 164),
                primary_container: rgb(234, 221, 255),
                on_primary_container: rgb(33, 0, 93),
                selection: rgb(103, 80, 164),
                on_selection: rgb(255, 255, 255),
                link: rgb(80, 57, 139),
                outline: rgb(121, 116, 126),
                error: rgb(179, 38, 30),
                success: rgb(20, 108, 46),
                success_container: rgb(215, 248, 217),
                success_outline: rgb(46, 125, 50),
                warning: rgb(122, 79, 0),
                warning_container: rgb(255, 224, 163),
                warning_outline: rgb(141, 91, 0),
            },
            UiTheme::Dracula => Self {
                dark: true,
                background: rgb(40, 42, 54),
                surface: rgb(33, 34, 44),
                surface_container: rgb(68, 71, 90),
                surface_container_high: rgb(68, 71, 90),
                on_surface: rgb(248, 248, 242),
                on_surface_variant: rgb(158, 168, 199),
                primary: rgb(189, 147, 249),
                primary_container: rgb(68, 71, 90),
                on_primary_container: rgb(248, 248, 242),
                selection: rgb(68, 71, 90),
                on_selection: rgb(248, 248, 242),
                link: rgb(139, 233, 253),
                outline: rgb(98, 114, 164),
                error: rgb(255, 85, 85),
                success: rgb(80, 250, 123),
                success_container: rgb(33, 34, 44),
                success_outline: rgb(80, 250, 123),
                warning: rgb(241, 250, 140),
                warning_container: rgb(33, 34, 44),
                warning_outline: rgb(241, 250, 140),
            },
            UiTheme::Falcon => editor_palette(
                true, 0x223333, 0x555555, 0x3366aa, 0xddddcc, 0x888888, 0x7799cc, 0x7799cc,
                0xee2222, 0x449900, 0xaa77aa,
            ),
            UiTheme::GitHubDark => editor_palette(
                true, 0x24292e, 0x2b3036, 0x253749, 0xe1e4e8, 0x6a737d, 0xb392f0, 0xf97583,
                0xff5555, 0x28a745, 0xffab70,
            ),
            UiTheme::GitHubLight => editor_palette(
                false, 0xffffff, 0xf6f8fa, 0xdee6fc, 0x24292e, 0x6a737d, 0x6f42c1, 0xd73a49,
                0xff5555, 0x28a745, 0xe36209,
            ),
            UiTheme::GruvboxDark => editor_palette(
                true, 0x282828, 0x32302f, 0x504945, 0xebdbb2, 0x928374, 0x689d6a, 0xd79921,
                0xcc241d, 0x689d6a, 0xd65d0e,
            ),
            UiTheme::GruvboxLight => editor_palette(
                false, 0xfbf1c7, 0xf2e5bc, 0xd5c4a1, 0x3c3836, 0x928374, 0x689d6a, 0xd79921,
                0xcc241d, 0x689d6a, 0xd65d0e,
            ),
            UiTheme::Nord => Self {
                dark: true,
                background: rgb(46, 52, 64),
                surface: rgb(59, 66, 82),
                surface_container: rgb(67, 76, 94),
                surface_container_high: rgb(76, 86, 106),
                on_surface: rgb(229, 233, 240),
                on_surface_variant: rgb(216, 222, 233),
                primary: rgb(136, 192, 208),
                primary_container: rgb(67, 76, 94),
                on_primary_container: rgb(236, 239, 244),
                selection: rgb(67, 76, 94),
                on_selection: rgb(236, 239, 244),
                link: rgb(129, 161, 193),
                outline: rgb(143, 188, 187),
                error: rgb(209, 138, 146),
                success: rgb(163, 190, 140),
                success_container: rgb(46, 52, 64),
                success_outline: rgb(163, 190, 140),
                warning: rgb(235, 203, 139),
                warning_container: rgb(46, 52, 64),
                warning_outline: rgb(235, 203, 139),
            },
            UiTheme::NoMansSky => Self {
                dark: true,
                background: rgb(16, 20, 23),
                surface: rgb(27, 31, 34),
                surface_container: rgb(37, 42, 46),
                surface_container_high: rgb(52, 58, 63),
                on_surface: rgb(244, 247, 248),
                on_surface_variant: rgb(169, 178, 184),
                primary: rgb(196, 38, 67),
                primary_container: rgb(74, 24, 33),
                on_primary_container: rgb(255, 220, 226),
                selection: rgb(34, 75, 88),
                on_selection: rgb(255, 255, 255),
                link: rgb(102, 213, 232),
                outline: rgb(126, 135, 141),
                error: rgb(255, 135, 145),
                success: rgb(255, 204, 85),
                success_container: rgb(66, 55, 23),
                success_outline: rgb(211, 155, 44),
                warning: rgb(255, 155, 104),
                warning_container: rgb(70, 37, 26),
                warning_outline: rgb(215, 107, 63),
            },
            UiTheme::SolarizedDark => Self {
                dark: true,
                background: rgb(0, 43, 54),
                surface: rgb(7, 54, 66),
                surface_container: rgb(11, 64, 76),
                surface_container_high: rgb(22, 79, 92),
                on_surface: rgb(147, 161, 161),
                on_surface_variant: rgb(147, 161, 161),
                primary: rgb(42, 161, 152),
                primary_container: rgb(7, 82, 92),
                on_primary_container: rgb(253, 246, 227),
                selection: rgb(88, 110, 117),
                on_selection: rgb(253, 246, 227),
                link: rgb(42, 161, 152),
                outline: rgb(101, 123, 131),
                error: rgb(237, 106, 100),
                success: rgb(167, 184, 35),
                success_container: rgb(7, 54, 66),
                success_outline: rgb(167, 184, 35),
                warning: rgb(208, 168, 41),
                warning_container: rgb(7, 54, 66),
                warning_outline: rgb(208, 168, 41),
            },
            UiTheme::SolarizedLight => Self {
                dark: false,
                background: rgb(253, 246, 227),
                surface: rgb(238, 232, 213),
                surface_container: rgb(229, 222, 200),
                surface_container_high: rgb(221, 213, 189),
                on_surface: rgb(7, 54, 66),
                on_surface_variant: rgb(88, 110, 117),
                primary: rgb(28, 102, 149),
                primary_container: rgb(203, 225, 227),
                on_primary_container: rgb(0, 43, 54),
                selection: rgb(88, 110, 117),
                on_selection: rgb(253, 246, 227),
                link: rgb(28, 102, 149),
                outline: rgb(101, 123, 131),
                error: rgb(181, 39, 36),
                success: rgb(82, 96, 0),
                success_container: rgb(235, 237, 199),
                success_outline: rgb(82, 96, 0),
                warning: rgb(107, 82, 0),
                warning_container: rgb(243, 225, 177),
                warning_outline: rgb(107, 82, 0),
            },
            UiTheme::HighContrastDark => Self {
                dark: true,
                background: rgb(0, 0, 0),
                surface: rgb(12, 12, 12),
                surface_container: rgb(20, 20, 20),
                surface_container_high: rgb(32, 32, 32),
                on_surface: rgb(255, 255, 255),
                on_surface_variant: rgb(255, 255, 255),
                primary: rgb(89, 227, 255),
                primary_container: rgb(0, 59, 71),
                on_primary_container: rgb(255, 255, 255),
                selection: rgb(0, 78, 92),
                on_selection: rgb(255, 255, 255),
                link: rgb(89, 227, 255),
                outline: rgb(255, 255, 255),
                error: rgb(255, 154, 154),
                success: rgb(134, 255, 159),
                success_container: rgb(0, 35, 10),
                success_outline: rgb(134, 255, 159),
                warning: rgb(255, 223, 128),
                warning_container: rgb(43, 33, 0),
                warning_outline: rgb(255, 223, 128),
            },
            UiTheme::HighContrastLight => Self {
                dark: false,
                background: rgb(255, 255, 255),
                surface: rgb(247, 247, 247),
                surface_container: rgb(238, 238, 238),
                surface_container_high: rgb(221, 221, 221),
                on_surface: rgb(0, 0, 0),
                on_surface_variant: rgb(17, 17, 17),
                primary: rgb(0, 78, 92),
                primary_container: rgb(200, 245, 255),
                on_primary_container: rgb(0, 0, 0),
                selection: rgb(0, 78, 92),
                on_selection: rgb(255, 255, 255),
                link: rgb(0, 65, 122),
                outline: rgb(0, 0, 0),
                error: rgb(140, 0, 0),
                success: rgb(0, 85, 26),
                success_container: rgb(210, 255, 220),
                success_outline: rgb(0, 85, 26),
                warning: rgb(92, 59, 0),
                warning_container: rgb(255, 235, 184),
                warning_outline: rgb(92, 59, 0),
            },
            UiTheme::Homunculus => editor_palette(
                false, 0xf7f3f3, 0xdaecf7, 0xbcbcbc, 0x303030, 0x505050, 0x721045, 0x0057ae,
                0xbf0303, 0x2ecc71, 0x00ff00,
            ),
            UiTheme::Monokai => editor_palette(
                true, 0x272822, 0x3e3d32, 0x3f413e, 0xf8f8f2, 0x75715e, 0xa6e22e, 0x66d9ef,
                0xff5555, 0x20e852, 0xffb86c,
            ),
            UiTheme::Oblivion => editor_palette(
                true, 0x201f1f, 0x2e3436, 0x184880, 0xd3d7c1, 0x30a100, 0x729fcf, 0x508ed8,
                0xe85848, 0x4e9a06, 0xad7fa8,
            ),
            UiTheme::Radical => editor_palette(
                true, 0x141322, 0x231630, 0x4f2e93, 0x7c9c9e, 0x45898c, 0xa9fef7, 0xff85a1,
                0xff427b, 0xffb000, 0xffd000,
            ),
            UiTheme::TokyoNight => editor_palette(
                true, 0x1a1b26, 0x1e202e, 0x27283e, 0xa9b1d6, 0x565f89, 0x7aa2f7, 0xbb9af7,
                0xdb4b4b, 0x9ece6a, 0xe0af68,
            ),
            UiTheme::TokyoNightLight => editor_palette(
                false, 0xd5d6db, 0xdcdee3, 0xe5e6eb, 0x343b58, 0x9699a3, 0x34548a, 0x5a4a78,
                0xbd4040, 0x485e30, 0x8f5e15,
            ),
            UiTheme::TokyoNightStorm => editor_palette(
                true, 0x24283b, 0x292e42, 0x323450, 0xa9b1d6, 0x565f89, 0x7aa2f7, 0xbb9af7,
                0xdb4b4b, 0x9ece6a, 0xe0af68,
            ),
            UiTheme::VimDark => editor_palette(
                true, 0x000000, 0x17003b, 0x232323, 0xb2b2b2, 0x54ffff, 0xcd00cd, 0xffff54,
                0xff5500, 0x5454ff, 0x00ff00,
            ),
            UiTheme::VscodiumDark => editor_palette(
                true, 0x1f1f1f, 0x242424, 0x264f78, 0xd4d4d4, 0x6a9955, 0xdcdcaa, 0x569cd6,
                0xf44747, 0xa3be8c, 0xd0b47d,
            ),
        }
    }
}

fn fonts(theme: UiTheme) -> egui::FontDefinitions {
    const NMS_FONT: &str = "NMS GeoSans";

    let mut fonts = egui::FontDefinitions::default();
    if theme == UiTheme::NoMansSky {
        fonts.font_data.insert(
            NMS_FONT.to_owned(),
            std::sync::Arc::new(
                egui::FontData::from_static(include_bytes!(
                    "../assets/fonts/nms-geosans/NMSGeoSans_Kerned.ttf"
                ))
                .tweak(egui::FontTweak {
                    scale: 1.10,
                    ..Default::default()
                }),
            ),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, NMS_FONT.to_owned());
    }
    fonts
}

pub fn apply(context: &egui::Context, theme: UiTheme) {
    let palette = Palette::for_theme(theme);
    let egui_theme = if palette.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    let mut visuals = if palette.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(palette.on_surface);
    visuals.weak_text_color = Some(palette.on_surface_variant);
    visuals.panel_fill = palette.background;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.outline);
    visuals.faint_bg_color = palette.surface_container;
    visuals.extreme_bg_color = palette.surface_container_high;
    visuals.text_edit_bg_color = Some(palette.surface_container_high);
    visuals.code_bg_color = palette.surface_container;
    visuals.selection.bg_fill = palette.selection;
    visuals.selection.stroke = Stroke::new(1.0, palette.on_selection);
    visuals.hyperlink_color = palette.link;
    visuals.error_fg_color = palette.error;
    visuals.warn_fg_color = palette.warning;

    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.weak_bg_fill = palette.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.outline);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.on_surface);
    visuals.widgets.inactive.bg_fill = palette.surface_container;
    visuals.widgets.inactive.weak_bg_fill = palette.surface_container;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.outline);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.on_surface);
    visuals.widgets.hovered.bg_fill = palette.primary_container;
    visuals.widgets.hovered.weak_bg_fill = palette.primary_container;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.primary);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.on_primary_container);
    // Frameless interactive widgets, including collapsing headers, use this foreground without
    // painting `bg_fill`. Keep active/open states on the container state layer so their text also
    // remains legible directly on the surrounding surface.
    visuals.widgets.active = visuals.widgets.hovered;
    visuals.widgets.open = visuals.widgets.hovered;

    context.set_fonts(fonts(theme));
    context.set_theme(egui_theme);
    context.set_visuals_of(egui_theme, visuals);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast(first: Color32, second: Color32) -> f64 {
        let first = luminance(first);
        let second = luminance(second);
        let (light, dark) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (light + 0.05) / (dark + 0.05)
    }

    #[test]
    fn all_palette_text_pairs_meet_wcag_aa() {
        for &theme in UiTheme::ALL {
            let palette = Palette::for_theme(theme);
            for (foreground, background) in [
                (palette.on_surface, palette.background),
                (palette.on_surface, palette.surface),
                (palette.on_surface_variant, palette.background),
                (palette.on_primary_container, palette.primary_container),
                (palette.on_primary_container, palette.background),
                (palette.on_selection, palette.selection),
                (palette.link, palette.background),
                (palette.error, palette.background),
                (palette.success, palette.success_container),
                (palette.warning, palette.warning_container),
            ] {
                assert!(
                    contrast(foreground, background) >= 4.5,
                    "{} has an inaccessible text pair: {foreground:?} on {background:?}",
                    theme.label()
                );
            }
        }
    }

    #[test]
    fn accessible_palettes_meet_wcag_aaa_for_primary_text() {
        for theme in [UiTheme::HighContrastDark, UiTheme::HighContrastLight] {
            let palette = Palette::for_theme(theme);
            for (foreground, background) in [
                (palette.on_surface, palette.background),
                (palette.on_surface, palette.surface),
                (palette.on_surface_variant, palette.background),
                (palette.on_primary_container, palette.primary_container),
                (palette.on_selection, palette.selection),
                (palette.link, palette.background),
                (palette.error, palette.background),
                (palette.success, palette.success_container),
                (palette.warning, palette.warning_container),
            ] {
                assert!(
                    contrast(foreground, background) >= 7.0,
                    "{} misses enhanced contrast: {foreground:?} on {background:?}",
                    theme.label()
                );
            }
        }
    }

    #[test]
    fn all_palette_outlines_meet_non_text_contrast() {
        for &theme in UiTheme::ALL {
            let palette = Palette::for_theme(theme);
            assert!(
                contrast(palette.outline, palette.background) >= 3.0,
                "{} outline is not distinguishable",
                theme.label()
            );
        }
    }

    #[test]
    fn no_mans_sky_font_keeps_default_fallbacks() {
        let defaults = fonts(UiTheme::SearchProbesDark);
        let themed = fonts(UiTheme::NoMansSky);
        let defaults = &defaults.families[&egui::FontFamily::Proportional];
        let themed = &themed.families[&egui::FontFamily::Proportional];

        assert_eq!(themed.first().map(String::as_str), Some("NMS GeoSans"));
        assert_eq!(&themed[1..], defaults);
        assert_eq!(
            fonts(UiTheme::NoMansSky).font_data["NMS GeoSans"]
                .tweak
                .scale,
            1.10
        );
    }
}
