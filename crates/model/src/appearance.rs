use crate::grass;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocabulary::GRASS_COLOUR_SET;

    #[test]
    fn shader_recolour_preserves_texture_shading_and_caps_saturation() {
        let result = recolour([0.5, 0.25, 0.25], [0.5, 0.25, 0.25], [0.25, 0.75, 0.25]);
        assert!((result[1] - 0.61915668845).abs() < 1e-9);
        assert!((result[0] - result[1] * 0.5).abs() < 1e-9);
        let fixed = recolour([0.2, 0.2, 0.2], [0.5; 3], [0.1, 1., 0.1]);
        assert!((fixed[0] - fixed[1]).abs() < 1e-9);
        assert!((fixed[1] - fixed[2]).abs() < 1e-9);
    }

    #[test]
    fn canonical_pipeline_retains_hues_and_separates_neutral_lightness() {
        assert_eq!(
            linear_colour([0.02, 0.1, 0.02], |rgb| rgb).unwrap(),
            GRASS_COLOUR_SET | 1 << 3
        );
        for (rgb, index) in [([0.; 3], 9), ([0.15; 3], 8), ([1.; 3], 10)] {
            assert_ne!(linear_colour(rgb, |rgb| rgb).unwrap() & (1 << index), 0);
        }
        assert!(linear_colour([f64::NAN; 3], |rgb| rgb).is_err());
        assert!(linear_colour([1000.; 3], |rgb| rgb).is_ok());
        assert!(linear_colour([-0.01; 3], |rgb| rgb).is_err());
        let neutralized = linear_colour([0.1, 0.8, 0.1], |_| [0.6; 3]).unwrap();
        assert_eq!(neutralized & 0xff, 0);
    }
}
