use crate::{facts::chromatic_hue, grass, vocabulary::GRASS_COLOUR_SET};
use serde::Serialize;

type Result<T> = std::result::Result<T, &'static str>;

pub fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

pub fn tone_map(mut linear: [f64; 3]) -> [f64; 3] {
    let peak = linear.into_iter().fold(0., f64::max);
    if peak > 0. {
        let clipped = if (1. - peak).abs() < 0.125 {
            (peak * (-8. * peak + 18.) - 6.125) * 0.25
        } else {
            peak.clamp(0., 1.)
        };
        linear = linear.map(|value| value * clipped / peak);
    }
    linear.map(|value| {
        let mapped = ((value * (value * 0.22 + 0.03) + 0.002)
            / (value * (value * 0.22 + 0.3) + 0.06)
            - 0.0333333313465)
            * 2.49283671379;
        linear_to_srgb(mapped.clamp(0., 1.)).clamp(0., 1.)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedColourAppearance {
    pub source_linear_rgb: [f64; 3],
    pub tone_mapped_srgb: [f64; 3],
}

pub fn linear_colour(linear: [f64; 3], grade: impl Fn([f64; 3]) -> [f64; 3]) -> Result<u32> {
    if linear.iter().any(|value| !value.is_finite() || *value < 0.) {
        return Err("invalid appearance colour");
    }
    let peak = linear.into_iter().fold(1., f64::max);
    let source = linear.map(|value| linear_to_srgb(value / peak));
    grass::primary_colour([source[0], source[1], source[2], 1.], 1., |rgb| {
        grade(tone_map(rgb.map(|value| srgb_to_linear(value) * peak)))
    })
}

pub fn generated_colour_with_appearance(
    linear: [f64; 3],
) -> Result<(u32, GeneratedColourAppearance)> {
    if linear.iter().any(|value| !value.is_finite() || *value < 0.) {
        return Err("invalid generated colour");
    }
    let tone_mapped_srgb = tone_map(linear);
    if tone_mapped_srgb
        .iter()
        .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
    {
        return Err("invalid tone-mapped generated colour");
    }
    let [r, g, b] = tone_mapped_srgb.map(srgb_to_linear);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let lightness = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    let chroma = a.hypot(b);

    // Atmosphere and water use the game's generated reference colours after canonical tone
    // mapping. Unlike grass and flora materials, they do not need the material classifier's
    // generous neutral tolerance or brightness compensation. A small absolute OKLab chroma admits
    // genuinely neutral results without erasing pale but visible sky and water hues.
    let colours = if chroma <= 0.015 {
        generated_neutral_colours(lightness)
    } else {
        1 << chromatic_hue(tone_mapped_srgb)?
    };
    Ok((
        GRASS_COLOUR_SET | colours,
        GeneratedColourAppearance {
            source_linear_rgb: linear,
            tone_mapped_srgb,
        },
    ))
}

pub const UPPER_SKY_REFERENCE_PROGRESS: f64 = 0.5;

// The atmosphere shader first mixes SkyUpperColour into SkyColour by
// clamp(SkyGradientSpeed * normalized_fade_progress, 0, 1). Evaluating the midpoint of that fade
// gives every generated planet the same clear-day upper-sky reference without pretending that a
// view-, sun-, fog-, and exposure-dependent dome has one exact rendered pixel colour.
pub fn upper_sky_reference(
    upper: [f64; 3],
    sky: [f64; 3],
    gradient_speed: [f64; 3],
) -> Result<[f64; 3]> {
    if upper
        .iter()
        .chain(&sky)
        .chain(&gradient_speed)
        .any(|value| !value.is_finite() || *value < 0.)
    {
        return Err("invalid upper-sky reference input");
    }
    Ok(std::array::from_fn(|channel| {
        let blend = (gradient_speed[channel] * UPPER_SKY_REFERENCE_PROGRESS).clamp(0., 1.);
        upper[channel] + (sky[channel] - upper[channel]) * blend
    }))
}

fn generated_neutral_colours(lightness: f64) -> u32 {
    const BLACK_GREY: f64 = 0.25;
    const GREY_WHITE: f64 = 0.92;
    if lightness < BLACK_GREY {
        1 << 9
    } else if lightness < GREY_WHITE {
        1 << 8
    } else {
        1 << 10
    }
}

#[derive(Clone, Copy)]
pub struct RecolourPixel {
    hsv: [f64; 3],
    response: f64,
}
impl RecolourPixel {
    pub fn new(pixel: [f64; 3]) -> Self {
        let hsv = hsv(pixel);
        Self {
            hsv,
            response: 10f64.powf(-10. * (hsv[2] - 0.5).powi(2)) * 0.47662675380706787,
        }
    }
    pub fn apply(self, average: [f64; 3], target: [f64; 3]) -> [f64; 3] {
        let hue = (self.hsv[0] - average[0] + target[0]).rem_euclid(1.);
        let saturation = self.hsv[1].min(target[1]);
        let value = (self.hsv[2] + self.response * (target[2] - average[2])).clamp(0., 1.);
        std::array::from_fn(|channel| {
            let offset = [1., 2. / 3., 1. / 3.][channel];
            let triangle = (((hue + offset).fract() * 6. - 3.).abs() - 1.).clamp(0., 1.);
            ((1. - saturation) + triangle * saturation) * value
        })
    }
}
pub fn texture_hsv(rgb: [f64; 3]) -> [f64; 3] {
    hsv(rgb)
}
pub fn recolour(pixel: [f64; 3], average: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    RecolourPixel::new(pixel).apply(hsv(average), hsv(target))
}

fn hsv(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta < 1e-10 {
        0.
    } else if max == r {
        ((g - b) / delta / 6.).rem_euclid(1.)
    } else if max == g {
        (b - r) / delta / 6. + 1. / 3.
    } else {
        (r - g) / delta / 6. + 2. / 3.
    };
    [hue, delta / (max + 1e-10), max]
}
