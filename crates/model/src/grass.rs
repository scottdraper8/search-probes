use crate::{facts::hue, vocabulary::GRASS_COLOUR_SET};

pub fn primary_colour(
    primary: [f64; 4],
    grade: impl Fn([f64; 3]) -> [f64; 3],
) -> Result<u32, &'static str> {
    let rgb = [primary[0], primary[1], primary[2]];
    if rgb
        .iter()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("invalid primary grass colour");
    }
    let rgb = grade(rgb);
    if rgb
        .iter()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("invalid graded grass colour");
    }
    Ok(GRASS_COLOUR_SET | (1 << classify(rgb)?))
}

fn classify(rgb: [f64; 3]) -> Result<u32, &'static str> {
    let [r, g, b] = rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let lightness = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    if lightness < 0.20 || a.hypot(b) <= lightness * 0.10 {
        Ok(if lightness < 0.50 { 9 } else { 10 })
    } else {
        hue(rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_primary_bindings_separate_black_white_pink_and_green() {
        for (rgb, expected) in [
            ([0.329, 0.3179999887943268, 0.310], 9),
            ([0.678, 0.573, 0.518], 10),
            (
                [0.49300000071525574, 0.257999986410141, 0.3320000469684601],
                7,
            ),
            ([0.261, 0.283, 0.163], 3),
        ] {
            let [r, g, b] = rgb;
            assert_eq!(
                primary_colour([r, g, b, 1.], |rgb| rgb).unwrap(),
                GRASS_COLOUR_SET | (1 << expected)
            );
        }
    }

    #[test]
    fn grading_changes_primary_colour_and_invalid_inputs_fail() {
        let green = [0.05, 0.8, 0.05, 1.];
        for (graded, expected) in [([0.05; 3], 9), ([0.9; 3], 10)] {
            assert_eq!(
                primary_colour(green, |_| graded).unwrap(),
                GRASS_COLOUR_SET | (1 << expected)
            );
        }
        assert!(primary_colour([f64::NAN; 4], |rgb| rgb).is_err());
        assert!(primary_colour([0.5; 4], |_| [f64::INFINITY; 3]).is_err());
    }
}
