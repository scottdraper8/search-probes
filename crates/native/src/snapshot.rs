use crate::{
    capture,
    query::{
        METADATA_OFFSET, PLANET_INPUT_OFFSET, PLANET_INPUT_SIZE, PLANET_QUERY_OFFSET,
        PLANET_QUERY_SIZE, QUERY_SIZE,
    },
};
use search_probes_model::{
    criteria::{Field, PlanetFlag, SystemFlag},
    facts::{Facts, hue},
    vocabulary::RESOURCE_IDS,
};

pub struct Snapshot {
    pub system: Facts,
    pub planets: [Facts; 8],
    pub count: usize,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated native query extent"),
    )
}

impl Snapshot {
    pub fn solar(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != QUERY_SIZE {
            return Err("invalid query snapshot size");
        }
        let count = u32_at(bytes, METADATA_OFFSET + 0x2544) as usize;
        if !(1..=8).contains(&count) {
            return Err("invalid native system planet count");
        }
        let mut system = Facts::default();
        system.set_planet_count(count as u8);
        system.set(Field::Anomaly, u32_at(bytes, 0x758));
        for (field, offset) in [
            (Field::Economy, 0x2520),
            (Field::Wealth, 0x2524),
            (Field::SystemClass, 0x252c),
            (Field::Conflict, 0x2530),
            (Field::Race, 0x2534),
            (Field::Star, 0x2550),
        ] {
            system.set(field, u32_at(bytes, METADATA_OFFSET + offset));
        }
        let mut planets = [Facts::default(); 8];
        let mut empty = false;
        let mut abandoned = false;
        let mut pirate = false;
        let mut giant = false;
        for (index, planet) in planets.iter_mut().enumerate().take(count) {
            let input = PLANET_INPUT_OFFSET + index * PLANET_INPUT_SIZE;
            let native_index = u32_at(bytes, input + 0x3c);
            if native_index != index as u32 {
                return Err("native planet input order changed");
            }
            let size = u32_at(bytes, input + 0x40);
            planet.set(Field::Size, size);
            planet.set_planet_flag(PlanetFlag::Rings, bytes[input + 0x4d] != 0);
            let moons = (0..count).any(|moon| {
                u32_at(bytes, PLANET_INPUT_OFFSET + moon * PLANET_INPUT_SIZE + 0x40) == 3
                    && u32_at(bytes, METADATA_OFFSET + 0x24b0 + moon * 4) == native_index
            });
            planet.set_planet_flag(PlanetFlag::Moons, moons);
            empty |= bytes[input + 0x4f] != 0;
            abandoned |= bytes[input + 0x4e] != 0;
            pirate |= bytes[input + 0x51] != 0;
            giant |= size == 4;
        }
        system.set(
            Field::Population,
            if empty {
                1
            } else if abandoned {
                2
            } else {
                0
            },
        );
        system.set_system_flag(SystemFlag::Pirate, pirate);
        system.set_system_flag(SystemFlag::Giant, giant);
        Ok(Self {
            system,
            planets,
            count,
        })
    }

    pub fn read_planet(
        &mut self,
        bytes: &[u8],
        index: usize,
        difficulty: usize,
    ) -> Result<(), &'static str> {
        if bytes.len() != QUERY_SIZE || index >= self.count || difficulty > 3 {
            return Err("invalid planet query snapshot");
        }
        let query = PLANET_QUERY_OFFSET + index * PLANET_QUERY_SIZE;
        let planet = &mut self.planets[index];
        let biome = u32_at(bytes, query + 0x80);
        let subtype = u32_at(bytes, query + 0x84);
        planet.set(Field::Biome, biome);
        planet.set(Field::Subtype, subtype);
        planet.set(Field::Terrain, u32_at(bytes, query + 0x134));
        planet.set(Field::StormFrequency, u32_at(bytes, query + 0x168));
        let mut resources = 0;
        for offset in [0x88, 0x98, 0xa8] {
            let id = &bytes[query + offset..query + offset + 16];
            let end = id.iter().position(|byte| *byte == 0).unwrap_or(16);
            if let Some(index) = RESOURCE_IDS
                .iter()
                .position(|candidate| candidate.as_bytes() == &id[..end])
            {
                resources |= 1 << index;
            }
        }
        planet.set_resources(resources);
        for (flag, offset) in [
            (PlanetFlag::Water, 0x120),
            (PlanetFlag::DeepWater, 0x121),
            (PlanetFlag::Sentinels, 0x124 + difficulty),
            (PlanetFlag::ExtremeSentinels, 0x128 + difficulty),
            (PlanetFlag::CorruptSentinels, 0x12c + difficulty),
            (PlanetFlag::ExtremeWeather, 0x130),
            (PlanetFlag::ExtremeHazard, 0x131),
            (PlanetFlag::Prime, 0x132),
            (PlanetFlag::CreatureDiscovery, 0x138),
            (PlanetFlag::WeirdCreatureDiscovery, 0x139),
            (PlanetFlag::CreatureTaming, 0x13a),
            (PlanetFlag::RobotCreatureDiscovery, 0x13b),
        ] {
            planet.set_planet_flag(flag, bytes[query + offset] != 0);
        }
        for (flag, value) in [
            (PlanetFlag::Infested, subtype == 24),
            (PlanetFlag::Relic, biome == 7 && subtype == 3),
            (PlanetFlag::Rgb, (8..=10).contains(&biome)),
            (
                PlanetFlag::Normal,
                biome != 6 && biome != 7 && subtype != 24,
            ),
            (
                PlanetFlag::NonGasGiant,
                planet.get(Field::Size) == Some(4) && biome != 15,
            ),
        ] {
            planet.set_planet_flag(flag, value);
        }
        Ok(())
    }

    pub fn aggregate(&mut self) {
        for &flag in SystemFlag::ALL {
            if matches!(flag, SystemFlag::Pirate | SystemFlag::Giant) {
                continue;
            }
            let mut unknown = false;
            let mut found = false;
            for planet in &self.planets[..self.count] {
                let value = match flag {
                    SystemFlag::GasGiant => planet.get(Field::Biome).map(|value| value == 15),
                    SystemFlag::Waterworld => planet.get(Field::Biome).map(|value| value == 14),
                    SystemFlag::Weird => planet.get(Field::Biome).map(|value| value == 7),
                    SystemFlag::ExtremeStorms => match (
                        planet.planet_flag(PlanetFlag::ExtremeWeather),
                        planet.get(Field::StormFrequency),
                    ) {
                        (Some(false), _) | (_, Some(0)) => Some(false),
                        (Some(true), Some(_)) => Some(true),
                        _ => None,
                    },
                    SystemFlag::NonGasGiant => planet.planet_flag(PlanetFlag::NonGasGiant),
                    SystemFlag::Water => planet.planet_flag(PlanetFlag::Water),
                    SystemFlag::DeepWater => planet.planet_flag(PlanetFlag::DeepWater),
                    SystemFlag::Infested => planet.planet_flag(PlanetFlag::Infested),
                    SystemFlag::Normal => planet.planet_flag(PlanetFlag::Normal),
                    SystemFlag::Relic => planet.planet_flag(PlanetFlag::Relic),
                    SystemFlag::Rgb => planet.planet_flag(PlanetFlag::Rgb),
                    SystemFlag::CorruptSentinels => {
                        planet.planet_flag(PlanetFlag::CorruptSentinels)
                    }
                    SystemFlag::Pirate | SystemFlag::Giant => unreachable!(),
                };
                match value {
                    Some(true) => {
                        found = true;
                        break;
                    }
                    Some(false) => {}
                    None => unknown = true,
                }
            }
            if found || !unknown {
                self.system.set_system_flag(flag, found);
            }
        }
    }
}

pub fn apply_capture(
    facts: &mut Facts,
    captured: &capture::Facts,
    difficulty: usize,
) -> Result<(), &'static str> {
    if difficulty > 3 {
        return Err("invalid combat difficulty");
    }
    if captured.present & capture::METADATA != 0 {
        let m = &captured.metadata;
        for (field, value) in [
            (Field::Flora, m.life_level),
            (Field::Fauna, m.creature_life_level),
            (Field::Buildings, m.building_density),
            (Field::ResourceRichness, m.resource_level),
        ] {
            facts.set(field, value);
        }
        facts.set_planet_flag(PlanetFlag::Scrap, m.has_scrap != 0);
    }
    if captured.present & capture::WEATHER != 0 {
        facts.set(Field::StormFrequency, captured.weather[0]);
        facts.set(Field::WeatherIntensity, captured.weather[1]);
        facts.set(Field::WeatherType, captured.weather[2]);
        if let Some(extreme) = facts.planet_flag(PlanetFlag::ExtremeWeather) {
            facts.set(
                Field::WeatherConditions,
                if captured.weather[0] == 0 {
                    0
                } else if extreme {
                    2
                } else {
                    1
                },
            );
        }
    }
    if captured.present & (capture::WEATHER | capture::METADATA)
        == capture::WEATHER | capture::METADATA
    {
        let m = &captured.metadata;
        facts.set_planet_flag(
            PlanetFlag::Paradise,
            m.biome == 0
                && ![3, 24, 25].contains(&m.biome_subtype)
                && m.sentinel_levels[difficulty] == 0
                && captured.weather[0] == 0
                && captured.weather[1] == 0,
        );
    }
    for &field in Field::ALL {
        if let Some(index) = field.colour_index() {
            if captured.present & capture::colour(index) != 0 {
                let colour = captured.colours[index];
                facts.set(field, hue([colour[0], colour[1], colour[2]])?);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_negatives_wait_for_every_planet() {
        let mut snapshot = Snapshot {
            system: Facts::default(),
            planets: [Facts::default(); 8],
            count: 2,
        };
        snapshot.planets[0].set_planet_flag(PlanetFlag::Water, false);
        snapshot.aggregate();
        assert_eq!(snapshot.system.system_flag(SystemFlag::Water), None);
        snapshot.planets[1].set_planet_flag(PlanetFlag::Water, true);
        snapshot.aggregate();
        assert_eq!(snapshot.system.system_flag(SystemFlag::Water), Some(true));
    }

    #[test]
    fn selected_capture_does_not_claim_uncaptured_properties() {
        let mut facts = Facts::default();
        let mut capture = capture::Facts::default();
        capture.present = capture::colour(7);
        capture.colours[7] = [0.0, 0.0, 1.0, 1.0];
        apply_capture(&mut facts, &capture, 0).unwrap();
        assert_eq!(facts.get(Field::Sky), Some(5));
        assert_eq!(facts.get(Field::Flora), None);
        assert_eq!(facts.planet_flag(PlanetFlag::Paradise), None);
    }
}
