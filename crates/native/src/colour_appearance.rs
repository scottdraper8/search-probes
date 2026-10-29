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
