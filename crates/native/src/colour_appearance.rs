use crate::{capture, colour_lut};
use search_probes_model::{appearance, criteria::Field, facts::Facts};

pub fn apply(facts: &mut Facts, captured: &capture::Facts) -> Result<(), &'static str> {
    let selected = Field::ALL.iter().copied().filter(|field| {
        !matches!(field, Field::Grass | Field::FloraColour)
            && field
                .colour_index()
                .is_some_and(|index| captured.present & capture::colour(index) != 0)
    });
    if selected.clone().next().is_none() {
        return Ok(());
    }
    let lut = colour_lut::get(captured.screen_filter)?;
    apply_with(facts, captured, |rgb| lut.sample(rgb))
}

pub fn apply_with(
    facts: &mut Facts,
    captured: &capture::Facts,
    grade: impl Fn([f64; 3]) -> [f64; 3],
) -> Result<(), &'static str> {
    for field in Field::ALL
        .iter()
        .copied()
        .filter(|field| !matches!(field, Field::Grass | Field::FloraColour))
    {
        let Some(index) = field
            .colour_index()
            .filter(|&index| captured.present & capture::colour(index) != 0)
        else {
            continue;
        };
        let rgba = captured.colours[index];
        facts.set(
            field,
            appearance::linear_colour([rgba[0], rgba[1], rgba[2]], &grade)?,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::vocabulary::GRASS_COLOUR_SET;

    #[test]
    fn only_requested_anchors_classify_and_grading_changes_their_visible_classes() {
        let mut facts = Facts::default();
        let mut raw = capture::Facts::default();
        raw.present = capture::colour(7) | capture::colour(9);
        raw.colours[7] = [0.05, 0.1, 0.8, 1.];
        raw.colours[9] = [0.05, 0.6, 0.1, 1.];
        apply_with(&mut facts, &raw, |rgb| rgb).unwrap();
        assert_eq!(facts.get(Field::Sky), Some(GRASS_COLOUR_SET | 1 << 5));
        assert_eq!(
            facts.get(Field::WaterColour),
            Some(GRASS_COLOUR_SET | 1 << 3)
        );
        assert_eq!(facts.get(Field::Horizon), None);
        apply_with(&mut facts, &raw, |_| [0.95; 3]).unwrap();
        assert_eq!(
            facts.get(Field::WaterColour),
            Some(GRASS_COLOUR_SET | 1 << 10)
        );
        assert_eq!(facts.get(Field::FloraColour), None);
        raw.colours[9][0] = f64::NAN;
        assert!(apply_with(&mut facts, &raw, |rgb| rgb).is_err());
    }
}
