use crate::capture;
use search_probes_model::{
    appearance,
    criteria::Field,
    facts::{ColourComponent, ColourComponentObservation, Facts, SkyReferenceObservation},
};

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
        let (classification, evidence) = appearance::generated_colour_with_appearance(linear)?;
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
                    appearance::generated_colour_with_appearance([rgba[0], rgba[1], rgba[2]])?;
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
