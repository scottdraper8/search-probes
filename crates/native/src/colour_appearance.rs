use crate::{capture, colour_lut};
use search_probes_model::{
    appearance,
    criteria::Field,
    facts::{ColourComponent, ColourComponentObservation, Facts, SkyReferenceObservation},
};

fn classify(
    linear: [f64; 3],
    world: &colour_lut::WorldGrade,
    distance_varying: bool,
) -> Result<(u32, appearance::GeneratedColourAppearance), &'static str> {
    let strengths = if distance_varying {
        world.strengths()
    } else {
        &[1.0]
    };
    let mut classifications = 0;
    let mut evidence = None;
    for &strength in strengths {
        let (classification, current) =
            appearance::generated_colour_with_grade(linear, |rgb| world.sample(rgb, strength))?;
        classifications |= classification;
        if strength == 1.0 {
            evidence = Some(current);
        }
    }
    Ok((
        classifications,
        evidence.ok_or("world colour classification omitted full-strength grade")?,
    ))
}

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
    let world = colour_lut::world_grade(captured.screen_filter)?;
    for field in selected {
        let index = field
            .colour_index()
            .ok_or("generated colour has no index")?;
        let linear = if field == Field::Sky {
            appearance::upper_sky_reference(
                captured.colours[capture::Colour::SkyUpper as usize][..3]
                    .try_into()
                    .map_err(|_| "invalid upper-sky colour")?,
                captured.colours[capture::Colour::Sky as usize][..3]
                    .try_into()
                    .map_err(|_| "invalid sky colour")?,
                captured.sky_gradient_speed,
            )?
        } else {
            captured.colours[index][..3]
                .try_into()
                .map_err(|_| "invalid generated colour")?
        };
        let (classification, evidence) = classify(linear, &world, field == Field::WaterColour)?;
        facts.set(field, classification);
        facts.set_colour_evidence(field, evidence);

        if field == Field::Sky {
            facts.set_sky_reference(SkyReferenceObservation {
                gradient_progress: appearance::UPPER_SKY_REFERENCE_PROGRESS,
                gradient_speed: captured.sky_gradient_speed,
            });
            for (component, index) in [
                (ColourComponent::SkyColour, capture::Colour::Sky),
                (ColourComponent::SkySolarColour, capture::Colour::SkySolar),
                (ColourComponent::SkyUpperColour, capture::Colour::SkyUpper),
                (ColourComponent::SunColour, capture::Colour::Sun),
            ] {
                let rgba = captured.colours[index as usize];
                let (classification, appearance) =
                    classify([rgba[0], rgba[1], rgba[2]], &world, false)?;
                facts.set_colour_component(
                    field,
                    component,
                    ColourComponentObservation {
                        classification,
                        appearance,
                    },
                );
            }
        }
    }
    Ok(())
}
