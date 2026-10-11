use crate::{
    criteria::{Field, PlanetFlag},
    facts::Facts,
};

/// A known absence must not match any storm type, including the generic warning.
pub const NO_STORM_TYPE: u32 = u32::MAX;

/// Native storm scheduling returns before selecting interval or fallback timers when
/// frequency is zero. Positive extreme-profile candidates wait for loaded hazard
/// proof; biome names do not determine whether a storm adds extreme exposure.
pub fn apply_storms(
    facts: &mut Facts,
    frequency: u32,
    extreme_hazard: bool,
) -> Result<(), &'static str> {
    let frequency = match frequency {
        0..=2 => frequency,
        // The reserved continuous schedule belongs at the high end of the cadence
        // filter. It is not offered as a separate natural-planet choice.
        3 => 2,
        _ => return Err("invalid native storm frequency"),
    };
    let storms = frequency != 0;
    facts.set(Field::StormFrequency, frequency);
    facts.clear_planet_flag(PlanetFlag::ExtremeStorms);
    if !storms || !extreme_hazard {
        facts.set_planet_flag(PlanetFlag::ExtremeStorms, false);
    } else if let Some(profile) = facts.extreme_storm_profile() {
        facts.set_planet_flag(PlanetFlag::ExtremeStorms, profile);
    }
    if !storms {
        facts.set(Field::StormType, NO_STORM_TYPE);
    } else if facts.get(Field::StormType) == Some(NO_STORM_TYPE) {
        facts.clear(Field::StormType);
    }
    Ok(())
}

/// Groups the current game's storm-warning routes, rather than exposing biome
/// weather configuration names. Bubble storms have a generic warning and a
/// distinctive bubble weather effect; the remaining generic routes are Other.
pub fn apply_storm_type(
    facts: &mut Facts,
    frequency: u32,
    weather_type: u32,
) -> Result<(), &'static str> {
    if frequency > 3 || weather_type > 16 {
        return Err("invalid native storm type");
    }
    let category = if frequency == 0 {
        NO_STORM_TYPE
    } else {
        match weather_type {
            2 | 10 => 0,     // Superheated rainstorm
            3 => 1,          // Blizzard
            4 => 2,          // Toxic rainstorm
            6 => 3,          // Radioactive supercell
            1 | 5 | 16 => 4, // Firestorm
            11 | 14 => 5,    // Wall of flame
            7..=9 | 13 => 6, // Gravitational anomaly
            12 => 7,         // Bubble storm
            0 | 15 => 8,     // Generic incoming storm
            _ => unreachable!(),
        }
    };
    facts.set(Field::StormType, category);
    Ok(())
}

/// Ambient and storm pairs, each ordered [extreme, normal], for temperature,
/// toxicity and radiation. The native hazard selector uses these same loaded
/// override/default tables. No biome or weather-type exceptions are necessary.
pub type StormHazards = [[[f32; 2]; 2]; 3];

pub fn extreme_storm_profile(profile: &StormHazards) -> Result<bool, &'static str> {
    if profile.iter().flatten().flatten().any(|v| !v.is_finite()) {
        return Err("non-finite native storm hazard");
    }
    let [ambient, storm] = profile[0];
    let temperature = (storm[0] > ambient[0] && storm[0] > storm[1])
        || (storm[0] < ambient[0] && storm[0] < storm[1]);
    let exposure = profile[1..]
        .iter()
        .any(|[ambient, storm]| storm[0] > ambient[0] && storm[0] > storm[1]);
    Ok(temperature || exposure)
}
