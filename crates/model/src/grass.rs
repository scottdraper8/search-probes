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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_brightness_precedes_grading_and_neutral_colours_are_distinct() {
        for (input, brightness, expected) in [
            ([0.329, 0.318, 0.310, 1.], 0.55, 9),
            ([0.678, 0.573, 0.518, 1.], 1., 10),
            ([0.493, 0.258, 0.332, 1.], 1., 7),
            ([0.261, 0.283, 0.163, 1.], 1., 3),
            ([0.1, 0.1, 0.1, 1.], 1., 9),
            ([0.45, 0.45, 0.45, 1.], 1., 8),
            ([0.8, 0.8, 0.8, 1.], 1., 10),
        ] {
            assert_eq!(
                primary_colour(input, brightness, |rgb| rgb).unwrap(),
                GRASS_COLOUR_SET | (1 << expected)
            );
        }
        let primary = [0.459, 0.420, 0.322, 1.];
        let brightness = 0.5577885269536208;
        let calls = std::cell::Cell::new(0);
        assert_eq!(
            primary_colour(primary, brightness, |rgb| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    assert_eq!(rgb, [primary[0], primary[1], primary[2]]);
                    return [0.5843401690607233, 0.4878517909693482, 0.3620626248974754];
                }
                assert_eq!(
                    rgb,
                    [
                        primary[0] * brightness,
                        primary[1] * brightness,
                        primary[2] * brightness
                    ]
                );
                [0.34321459133043936, 0.2936910143510805, 0.22421072113014257]
            })
            .unwrap(),
            GRASS_COLOUR_SET | (1 << 8) | (1 << 9)
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn visible_hues_survive_texture_darkening_and_grading_can_remove_them() {
        let green = [0.261, 0.283, 0.163, 1.];
        for brightness in [0.25, 0.37, 0.48, 0.62, 0.68, 1.] {
            assert_eq!(
                primary_colour(green, brightness, |rgb| rgb).unwrap(),
                GRASS_COLOUR_SET | (1 << 3)
            );
        }
        assert_eq!(
            primary_colour(green, 0.5, |rgb| [rgb[0]; 3]).unwrap(),
            GRASS_COLOUR_SET | (1 << 9)
        );
        for (rgb, expected) in [
            ([0.6, 0.2, 0.2, 1.], 0),
            ([0.2, 0.6, 0.2, 1.], 3),
            ([0.2, 0.2, 0.6, 1.], 5),
        ] {
            assert_eq!(
                primary_colour(rgb, 0.1, |v| v).unwrap(),
                GRASS_COLOUR_SET | (1 << expected)
            );
        }
    }

    #[test]
    fn shadow_clipping_preserves_daylight_hues_without_undoing_desaturation_or_white_clipping() {
        let neutral = (1 << 8) | (1 << 9) | (1 << 10);
        for (rgb, expected) in [
            ([1., 0., 0.], 0),
            ([1., 1., 0.], 2),
            ([0., 1., 0.], 3),
            ([0., 1., 1.], 4),
            ([0., 0., 1.], 5),
            ([1., 0., 1.], 7),
        ] {
            let source = Appearance::new(rgb).unwrap();
            let exposure = (0.30 / source.lightness).powi(3);
            let input = rgb.map(|v| linear_to_srgb(srgb_to_linear(v) * exposure));
            let primary = [input[0], input[1], input[2], 1.];
            for brightness in [0.25, 0.6, 1.] {
                let result = primary_colour(primary, brightness, |rgb| {
                    if Appearance::new(rgb).unwrap().lightness < 0.4 {
                        [0.15; 3]
                    } else {
                        rgb
                    }
                })
                .unwrap();
                assert_eq!(result & neutral, 0);
                assert_eq!(result & !GRASS_COLOUR_SET, 1 << expected);
                let grey =
                    primary_colour(primary, brightness, |rgb| [rgb.iter().sum::<f64>() / 3.; 3])
                        .unwrap();
                assert_ne!(grey & neutral, 0);
                assert_eq!(grey & !GRASS_COLOUR_SET & !neutral, 0);
            }
        }
        assert_eq!(
            primary_colour([0.95, 0.8, 0.2, 1.], 1., |rgb| {
                if rgb[0] > 0.9 { [1.; 3] } else { rgb }
            })
            .unwrap(),
            GRASS_COLOUR_SET | (1 << 10)
        );
    }

    #[test]
    fn neutral_boundary_overlap_matches_only_adjacent_categories() {
        for (lightness, expected) in [
            (0.34, 1 << 9),
            (0.39, (1 << 9) | (1 << 8)),
            (0.41, (1 << 9) | (1 << 8)),
            (0.50, 1 << 8),
            (0.64, (1 << 8) | (1 << 10)),
            (0.66, (1 << 8) | (1 << 10)),
            (0.72, 1 << 10),
        ] {
            let linear = lightness * lightness * lightness;
            let srgb = 1.055 * f64::powf(linear, 1. / 2.4) - 0.055;
            let mut facts = crate::facts::Facts::default();
            facts.set(
                crate::criteria::Field::Grass,
                primary_colour([srgb, srgb, srgb, 1.], 1., |rgb| rgb).unwrap(),
            );
            for colour in [3, 8, 9, 10] {
                let mut criteria = crate::criteria::Criteria::default();
                criteria
                    .choices
                    .insert(crate::criteria::Field::Grass, 1 << colour);
                assert_eq!(
                    criteria.compile().unwrap().planet_match(&facts)
                        == crate::facts::Verdict::Match,
                    expected & (1 << colour) != 0,
                    "lightness {lightness}, colour {colour}"
                );
            }
        }
    }

    #[test]
    fn neutral_colourfulness_limit_applies_across_hues_and_lightness() {
        for lightness in [0.30, 0.45, 0.70] {
            for degrees in (0..360).step_by(15) {
                for (relative_chroma, neutral) in [(0.05, true), (0.09, false)] {
                    let angle = f64::from(degrees).to_radians();
                    let a = lightness * relative_chroma * angle.cos();
                    let b = lightness * relative_chroma * angle.sin();
                    let l = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
                    let m = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
                    let s = (lightness - 0.0894841775 * a - 1.2914855480 * b).powi(3);
                    let rgb = [
                        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
                    ]
                    .map(|v| {
                        if v <= 0.0031308 {
                            12.92 * v
                        } else {
                            1.055 * v.powf(1. / 2.4) - 0.055
                        }
                    });
                    for brightness in [0.3, 0.6, 1.] {
                        let value = primary_colour([rgb[0], rgb[1], rgb[2], 1.], brightness, |v| v)
                            .unwrap();
                        assert_eq!(
                            value & ((1 << 8) | (1 << 9) | (1 << 10)) != 0,
                            neutral,
                            "lightness {lightness}, hue {degrees}, relative chroma {relative_chroma}, brightness {brightness}"
                        );
                        let desaturated =
                            primary_colour([rgb[0], rgb[1], rgb[2], 1.], brightness, |v| {
                                [(v[0] + v[1] + v[2]) / 3.; 3]
                            })
                            .unwrap();
                        assert_ne!(desaturated & ((1 << 8) | (1 << 9) | (1 << 10)), 0);
                        let tinted = primary_colour([0.5; 4], brightness, |_| rgb).unwrap();
                        assert_eq!(tinted & ((1 << 8) | (1 << 9) | (1 << 10)) != 0, neutral);
                    }
                }
            }
        }
    }

    #[test]
    fn bradford_adaptation_matches_d50_to_d65_reference_vectors() {
        let reference = [0.6285874760725343, 0.5772945551067482, 0.5026838529448685];
        for (input, expected) in [
            (
                [0.47202640550184766, 0.566998006320541, 0.3790828490620728],
                [0.4220924784104944, 0.5733489068850326, 0.4466674939122807],
            ),
            (
                [0.8065292124277325, 0.3292677873614649, 0.3163381643812276],
                [0.7782995537441071, 0.33707492343141104, 0.3754841274624274],
            ),
            (reference, [0.5838314900602575; 3]),
        ] {
            let actual = bradford_adapt(input, reference);
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn hue_names_account_for_mild_casts_and_preserve_colour_effects() {
        for (rgb, expected) in [
            ([0.6, 0.15, 0.15, 1.], 0),
            ([0.6, 0.32, 0.15, 1.], 1),
            ([0.6, 0.54, 0.15, 1.], 2),
            ([0.50, 0.6, 0.15, 1.], 3),
            ([0.15, 0.6, 0.15, 1.], 3),
            ([0.15, 0.6, 0.6, 1.], 4),
            ([0.15, 0.15, 0.6, 1.], 5),
            ([0.40, 0.15, 0.6, 1.], 6),
            ([0.6, 0.15, 0.6, 1.], 7),
        ] {
            for gains in [[1.10, 1., 0.85], [0.85, 1., 1.10], [1.; 3]] {
                for brightness in [0.3, 0.6, 1.] {
                    let result = primary_colour(rgb, brightness, |v| {
                        std::array::from_fn(|i| v[i] * gains[i])
                    })
                    .unwrap();
                    assert_eq!(result, GRASS_COLOUR_SET | (1 << expected));
                }
            }
        }
        for (graded, expected) in [([0.1, 0.6, 0.1], 3), ([0.35, 0.1, 0.45], 6)] {
            assert_eq!(
                primary_colour([0.6, 0.15, 0.15, 1.], 0.5, |_| graded).unwrap(),
                GRASS_COLOUR_SET | (1 << expected)
            );
        }
        assert_eq!(
            primary_colour([0.6, 0.15, 0.15, 1.], 0.5, |[r, g, b]| [b, r, g]).unwrap(),
            GRASS_COLOUR_SET | (1 << 3)
        );
    }

    #[test]
    fn invalid_inputs_and_grading_fail_instead_of_producing_matches() {
        for brightness in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert!(primary_colour([0.5; 4], brightness, |rgb| rgb).is_err());
        }
        assert!(primary_colour([f64::NAN; 4], 1., |rgb| rgb).is_err());
        assert!(primary_colour([0.5; 4], 1., |_| [f64::INFINITY; 3]).is_err());
        assert!(
            primary_colour([0.6, 0.15, 0.15, 1.], 1., |rgb| {
                if rgb[0] == rgb[1] && rgb[1] == rgb[2] {
                    [f64::NAN; 3]
                } else {
                    rgb
                }
            })
            .is_err()
        );
    }
}
