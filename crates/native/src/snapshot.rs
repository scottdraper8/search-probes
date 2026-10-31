use crate::{
    capture,
    query::{
        METADATA_OFFSET, PLANET_INPUT_OFFSET, PLANET_INPUT_SIZE, PLANET_QUERY_OFFSET,
        PLANET_QUERY_SIZE, QUERY_SIZE,
    },
};
use search_probes_model::{
    criteria::{Field, PlanetFlag, SystemFlag, WaterPresence},
    facts::Facts,
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
            (Field::Conflict, 0x2530),
            (Field::Race, 0x2534),
            (Field::Star, 0x2550),
        ] {
            system.set(field, u32_at(bytes, METADATA_OFFSET + offset));
        }
        let mut planets = std::array::from_fn(|_| Facts::default());
        let mut empty = false;
        let mut abandoned = false;
        let mut pirate = false;
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
        search_probes_model::weather::apply_storms(
            planet,
            u32_at(bytes, query + 0x168),
            bytes[query + 0x131] != 0,
        )?;
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
        planet.set_water(if bytes[query + 0x121] != 0 {
            WaterPresence::Deep
        } else if bytes[query + 0x120] != 0 {
            WaterPresence::Shallow
        } else {
            WaterPresence::Dry
        });
        for (flag, offset) in [
            (PlanetFlag::Dissonant, 0x12c + 2),
            (PlanetFlag::Sentinels, 0x124 + difficulty),
            (PlanetFlag::ExtremeSentinels, 0x128 + difficulty),
            (PlanetFlag::CorruptSentinels, 0x12c + difficulty),
        ] {
            planet.set_planet_flag(flag, bytes[query + offset] != 0);
        }
        let sentinel_level = match (
            planet.planet_flag(PlanetFlag::Sentinels),
            planet.planet_flag(PlanetFlag::ExtremeSentinels),
            planet.planet_flag(PlanetFlag::CorruptSentinels),
        ) {
            (Some(false), Some(false), Some(false)) => 0,
            (Some(true), Some(false), Some(false)) => 1,
            (Some(true), Some(true), Some(false)) => 2,
            (Some(true), Some(false), Some(true)) => 3,
            _ => return Err("inconsistent native sentinel classification"),
        };
        planet.set(Field::Sentinels, sentinel_level);
        Ok(())
    }

    pub fn aggregate(&mut self) {
        for &flag in SystemFlag::ALL {
            if flag == SystemFlag::Pirate {
                continue;
            }
            let mut unknown = false;
            let mut found = false;
            for planet in &self.planets[..self.count] {
                let value = match flag {
                    SystemFlag::ExtremeStorms => planet.planet_flag(PlanetFlag::ExtremeStorms),
                    SystemFlag::Dissonance => planet.planet_flag(PlanetFlag::Dissonant),
                    SystemFlag::Pirate => unreachable!(),
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
        if m.sentinel_levels[difficulty] > 3 {
            return Err("invalid native sentinel level");
        }
        for (field, value) in [
            (Field::Flora, m.life_level),
            (Field::Fauna, m.creature_life_level),
            (Field::ResourceRichness, m.resource_level),
            (Field::Sentinels, m.sentinel_levels[difficulty]),
        ] {
            facts.set(field, value);
        }
    }
    if captured.present & capture::SCREEN_FILTER != 0 {
        let value =
            u32::try_from(captured.screen_filter).map_err(|_| "invalid native screen filter")?;
        if value >= 85 {
            return Err("invalid native screen filter");
        }
        facts.set(Field::WorldColourFilter, value);
    }
    if captured.present & capture::WEATHER != 0 {
        let [frequency, intensity, weather_type] = captured.weather;
        if intensity > 1 {
            return Err("invalid native storm hazard profile");
        }
        search_probes_model::weather::apply_storms(facts, frequency, intensity == 1)?;
        search_probes_model::weather::apply_storm_type(facts, frequency, weather_type)?;
    }
    Ok(())
}
