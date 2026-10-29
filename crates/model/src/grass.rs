use crate::{facts::hue, vocabulary::GRASS_COLOUR_SET};

pub fn primary_colour(
    primary: [f64; 4],
    brightness: f64,
    grade: impl Fn([f64; 3]) -> [f64; 3],
) -> Result<u32, &'static str> {
    PrimaryColour::new(primary, grade)?.material_colour(brightness)
}

pub struct PrimaryColour<G> {
    rgb: [f64; 3],
    hue: Option<u32>,
    grade: G,
}

impl<G: Fn([f64; 3]) -> [f64; 3]> PrimaryColour<G> {
    pub fn new(primary: [f64; 4], grade: G) -> Result<Self, &'static str> {
        let rgb = [primary[0], primary[1], primary[2]];
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("invalid primary grass colour");
        }
        let source = Appearance::new(rgb)?;
        let identity = Appearance::new(grade(rgb))?;
        let identity_chroma = if source.is_neutral(0.08) { 0.10 } else { 0.08 };
        let hue = if identity.is_neutral(identity_chroma) {
            if source.is_neutral(0.08) {
                None
            } else {
                daylight_hue(&source, &grade)?
            }
        } else {
            Some(appearance_hue(source.lightness, &identity, &grade)?)
        };
        Ok(Self { rgb, hue, grade })
    }

    pub fn fixed_colour(&self) -> Option<u32> {
        self.hue.map(|hue| GRASS_COLOUR_SET | (1 << hue))
    }

    pub fn material_colour(&self, brightness: f64) -> Result<u32, &'static str> {
        if !brightness.is_finite() || !(0. ..=1.).contains(&brightness) {
            return Err("invalid grass material brightness");
        }
        if let Some(hue) = self.hue {
            return Ok(GRASS_COLOUR_SET | (1 << hue));
        }
        let material_rgb = self.rgb.map(|v| v * brightness);
        let representative = Appearance::new((self.grade)(material_rgb))?;
        let colours = if representative.is_neutral(0.08) {
            neutral_colours(representative.lightness)
        } else {
            1 << appearance_hue(
                Appearance::new(material_rgb)?.lightness,
                &representative,
                &self.grade,
            )?
        };
        Ok(GRASS_COLOUR_SET | colours)
    }
}

struct Appearance {
    rgb: [f64; 3],
    lightness: f64,
    chroma: f64,
}

impl Appearance {
    fn new(rgb: [f64; 3]) -> Result<Self, &'static str> {
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("invalid graded grass colour");
        }
        let [r, g, b] = rgb.map(srgb_to_linear);
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        let lightness = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
        let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
        let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
        Ok(Self {
            rgb,
            lightness,
            chroma: a.hypot(b),
        })
    }

    fn is_neutral(&self, relative_chroma: f64) -> bool {
        self.lightness < 0.20 || self.chroma <= self.lightness * relative_chroma
    }
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

fn daylight_hue(
    source: &Appearance,
    grade: &impl Fn([f64; 3]) -> [f64; 3],
) -> Result<Option<u32>, &'static str> {
    if source.lightness >= 0.45 {
        return Ok(None);
    }
    let linear = source.rgb.map(srgb_to_linear);
    let mut agreed = None;
    for lightness in [0.45, 0.55, 0.65] {
        let exposure = (lightness / source.lightness).powi(3);
        let input = linear.map(|value| linear_to_srgb((value * exposure).min(1.)));
        let colour = Appearance::new(grade(input))?;
        if colour.is_neutral(0.08) {
            return Ok(None);
        }
        let name = appearance_hue(Appearance::new(input)?.lightness, &colour, grade)?;
        if agreed.is_some_and(|previous| previous != name) {
            return Ok(None);
        }
        agreed = Some(name);
    }
    Ok(agreed)
}

fn appearance_hue(
    input_lightness: f64,
    colour: &Appearance,
    grade: &impl Fn([f64; 3]) -> [f64; 3],
) -> Result<u32, &'static str> {
    let original = hue(colour.rgb)?;
    let grey = linear_to_srgb(input_lightness.powi(3));
    let reference = Appearance::new(grade([grey; 3]))?;
    if reference.lightness < 0.20 || !reference.is_neutral(0.08) || reference.chroma < 1e-6 {
        return Ok(original);
    }
    let name = hue(bradford_adapt(colour.rgb, reference.rgb))?;
    Ok(if name == 8 { original } else { name })
}

fn bradford_adapt(rgb: [f64; 3], reference: [f64; 3]) -> [f64; 3] {
    const RGB_TO_CONE: [[f64; 3]; 3] = [
        [0.42265804144643404, 0.4913566407473867, 0.02736449713748613],
        [
            0.055690799998372405,
            0.9615562080812273,
            0.023189374899123684,
        ],
        [
            0.021379241138820733,
            0.08764392158298849,
            0.9807434330228718,
        ],
    ];
    const CONE_TO_RGB: [[f64; 3]; 3] = [
        [2.5384479741710906, -1.2934825680200772, -0.0402432469070053],
        [
            -0.14600036509239717,
            1.1166224074523543,
            -0.022328519689431513,
        ],
        [
            -0.04228837574887107,
            -0.07159006996314218,
            1.0225073096004835,
        ],
    ];
    let multiply = |matrix: [[f64; 3]; 3], value: [f64; 3]| {
        matrix.map(|row| row.into_iter().zip(value).map(|(a, b)| a * b).sum::<f64>())
    };
    let reference = reference.map(srgb_to_linear);
    let luminance = reference[0] * 0.21263900587151027
        + reference[1] * 0.7151686787677559
        + reference[2] * 0.07219231536073371;
    let source = multiply(RGB_TO_CONE, reference);
    let target = multiply(RGB_TO_CONE, [luminance; 3]);
    let cone = multiply(RGB_TO_CONE, rgb.map(srgb_to_linear));
    let adapted = multiply(
        CONE_TO_RGB,
        std::array::from_fn(|i| cone[i] * target[i] / source[i]),
    );
    let peak = adapted.into_iter().fold(1., f64::max);
    adapted.map(|v| linear_to_srgb((v / peak).clamp(0., 1.)))
}

fn neutral_colours(lightness: f64) -> u32 {
    const BLACK_GREY: f64 = 0.40;
    const GREY_WHITE: f64 = 0.65;
    const OVERLAP: f64 = 0.025;
    let mut colours = 0;
    if lightness <= BLACK_GREY + OVERLAP {
        colours |= 1 << 9;
    }
    if (BLACK_GREY - OVERLAP..=GREY_WHITE + OVERLAP).contains(&lightness) {
        colours |= 1 << 8;
    }
    if lightness >= GREY_WHITE - OVERLAP {
        colours |= 1 << 10;
    }
    colours
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GrassModel {
    Cross,
    Lush,
    Bubble,
    Barren,
}

impl GrassModel {
    pub const ALL: [Self; 4] = [Self::Cross, Self::Lush, Self::Bubble, Self::Barren];
    pub const fn mask(self) -> u32 {
        1 << self as u32
    }
    pub const fn path(self) -> &'static str {
        match self {
            Self::Cross => "MODELS/PLANETS/BIOMES/COMMON/GRASS/NEWCROSSGRASS.SCENE.MBIN",
            Self::Lush => "MODELS/PLANETS/BIOMES/COMMON/GRASS/NEWLUSHGRASS.SCENE.MBIN",
            Self::Bubble => "MODELS/PLANETS/BIOMES/COMMON/GRASS/BUBBLELUSHGRASS.SCENE.MBIN",
            Self::Barren => "MODELS/PLANETS/BIOMES/BARREN/HQ/FOLIAGE/BARRENGRASSLARGE.SCENE.MBIN",
        }
    }
}
