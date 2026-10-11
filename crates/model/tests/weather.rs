use search_probes_model::{
    criteria::{Criteria, Field, PlanetFlag, Requirements, ResourceMode, Section},
    facts::{Facts, Verdict},
    weather::{apply_storm_type, apply_storms, extreme_storm_profile},
};

#[test]
fn severity_uses_added_storm_hazards_and_defers_until_the_profile_is_known() {
    // Current vanilla witnesses: ambient/storm, with [extreme, normal] pairs.
    for (temperature, toxicity, radiation, severe) in [
        (
            [[20., 12.], [140., 92.]],
            [[20., 0.], [101., 80.]],
            [[20., 0.], [20., 0.]],
            true,
        ),
        (
            [[-92., -50.], [-120., -92.]],
            [[0.; 2]; 2],
            [[0.; 2]; 2],
            true,
        ),
        (
            [[30., -20.], [20., -20.]],
            [[92., 50.], [100., 92.]],
            [[20., 0.], [20., 0.]],
            true,
        ),
        (
            [[30., -20.], [20., -20.]],
            [[20., 0.], [20., 0.]],
            [[92., 50.], [300., 92.]],
            true,
        ),
        ([[20., 20.], [-5., -5.]], [[0.; 2]; 2], [[0.; 2]; 2], false),
        (
            [[20., 20.], [0., 0.]],
            [[20., 0.], [20., 0.]],
            [[20., 0.], [20., 0.]],
            false,
        ),
    ] {
        let profile = [temperature, toxicity, radiation];
        assert_eq!(extreme_storm_profile(&profile), Ok(severe));
        for required in [false, true] {
            let mut criteria = Criteria::default();
            criteria.planets.insert(PlanetFlag::ExtremeStorms, required);
            let plan = criteria.compile().unwrap();
            let mut facts = Facts::default();
            apply_storms(&mut facts, 2, true).unwrap();
            assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
            assert!(plan.missing_planet_requirements(&facts).weather);
            facts.set_extreme_storm_profile(severe);
            apply_storms(&mut facts, 2, true).unwrap();
            assert_eq!(
                plan.planet_match(&facts),
                if required == severe {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                }
            );
            let mut cached = Facts::default();
            cached.merge(&facts);
            apply_storms(&mut cached, 2, true).unwrap();
            assert_eq!(plan.planet_match(&cached), plan.planet_match(&facts));
            assert!(!plan.missing_planet_requirements(&cached).weather);
        }
    }
    assert!(extreme_storm_profile(&[[[f32::NAN; 2]; 2]; 3]).is_err());
}

#[test]
fn storm_severity_and_cadence_are_independent_of_biome() {
    for biome in [0, 1, 4, 5, 7, 12, 13, 14, 15] {
        for (frequency, extreme, severe, cadence) in [
            (0, false, false, 0),
            (0, true, false, 0),
            (1, false, false, 1),
            (1, true, true, 1),
            (2, false, false, 2),
            (2, true, true, 2),
            (3, true, true, 2),
        ] {
            let mut facts = Facts::default();
            facts.set(Field::Biome, biome);
            facts.set_extreme_storm_profile(true);
            apply_storms(&mut facts, frequency, extreme).unwrap();
            assert_eq!(facts.get(Field::StormFrequency), Some(cadence));
            assert_eq!(facts.planet_flag(PlanetFlag::ExtremeStorms), Some(severe));
            for mask in [1, 2, 4, 2 | 4] {
                let mut criteria = Criteria::default();
                criteria.choices.insert(Field::StormFrequency, mask);
                let plan = criteria.compile().unwrap();
                assert_eq!(
                    plan.requirements,
                    Requirements {
                        planet_query: true,
                        ..Default::default()
                    }
                );
                assert_eq!(plan.planet_match(&Facts::default()), Verdict::Unknown);
                assert_eq!(
                    plan.planet_match(&facts),
                    if mask & (1 << cadence) != 0 {
                        Verdict::Match
                    } else {
                        Verdict::NoMatch
                    }
                );
            }
            for (flag, actual) in [(PlanetFlag::ExtremeStorms, severe)] {
                for required in [false, true] {
                    let mut criteria = Criteria::default();
                    criteria.planets.insert(flag, required);
                    let plan = criteria.compile().unwrap();
                    assert_eq!(
                        plan.requirements,
                        Requirements {
                            planet_query: true,
                            weather: flag == PlanetFlag::ExtremeStorms,
                            ..Default::default()
                        }
                    );
                    assert_eq!(plan.planet_match(&Facts::default()), Verdict::Unknown);
                    let expected = if actual == required {
                        Verdict::Match
                    } else {
                        Verdict::NoMatch
                    };
                    assert_eq!(plan.planet_match(&facts), expected);
                    let mut cached = Facts::default();
                    cached.merge(&facts);
                    assert_eq!(plan.planet_match(&cached), expected);
                    assert_eq!(
                        plan.missing_planet_requirements(&cached),
                        Requirements::default()
                    );
                }
            }
        }
    }
    assert!(apply_storms(&mut Facts::default(), 4, true).is_err());
}

#[test]
fn types_follow_verified_warning_routes_and_require_actual_storms() {
    // Independent grouping from the emulated current executable's warning table.
    let witnesses: &[(usize, &[u32])] = &[
        (0, &[2, 10]),
        (1, &[3]),
        (2, &[4]),
        (3, &[6]),
        (4, &[1, 5, 16]),
        (5, &[11, 14]),
        (6, &[7, 8, 9, 13]),
        (7, &[12]),
        (8, &[0, 15]),
    ];
    for (choice, types) in witnesses {
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::StormType, 1 << choice);
        let plan = criteria.compile().unwrap();
        assert!(plan.requirements.weather);
        assert!(!plan.requirements.metadata && !plan.requirements.objects);
        for frequency in 0..=3 {
            for weather_type in 0..=16 {
                let mut facts = Facts::default();
                apply_storms(&mut facts, frequency, false).unwrap();
                if frequency != 0 {
                    assert!(plan.missing_planet_requirements(&facts).weather);
                } else {
                    assert!(!plan.missing_planet_requirements(&facts).weather);
                }
                apply_storm_type(&mut facts, frequency, weather_type).unwrap();
                let expected = if frequency != 0 && types.contains(&weather_type) {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                };
                assert_eq!(plan.planet_match(&facts), expected);
                let mut cached = Facts::default();
                cached.merge(&facts);
                assert_eq!(plan.planet_match(&cached), expected);
            }
        }
    }
    assert!(apply_storm_type(&mut Facts::default(), 1, 17).is_err());
    assert!(apply_storm_type(&mut Facts::default(), 4, 2).is_err());
    let mut facts = Facts::default();
    apply_storms(&mut facts, 0, false).unwrap();
    assert!(facts.get(Field::StormType).is_some());
    apply_storms(&mut facts, 1, false).unwrap();
    assert_eq!(facts.get(Field::StormType), None);
}

#[test]
fn weather_subsection_modes_include_exclusions_and_persist() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::StormFrequency, 1);
    criteria.planets.insert(PlanetFlag::ExtremeStorms, true);
    let all = criteria.compile().unwrap();
    criteria
        .match_modes
        .insert(Section::Weather, ResourceMode::Any);
    let restored: Criteria =
        serde_json::from_str(&serde_json::to_string(&criteria).unwrap()).unwrap();
    assert_eq!(restored, criteria);
    let any = restored.compile().unwrap();
    assert_ne!(all, any);
    for (frequency, extreme, all_verdict, any_verdict) in [
        (0, true, Verdict::NoMatch, Verdict::Match),
        (1, false, Verdict::NoMatch, Verdict::NoMatch),
        (1, true, Verdict::NoMatch, Verdict::Match),
    ] {
        let mut facts = Facts::default();
        facts.set_extreme_storm_profile(true);
        apply_storms(&mut facts, frequency, extreme).unwrap();
        assert_eq!(all.planet_match(&facts), all_verdict);
        assert_eq!(any.planet_match(&facts), any_verdict);
    }
    assert_eq!(any.planet_match(&Facts::default()), Verdict::Unknown);
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::StormFrequency, 1 << 3);
    assert!(criteria.compile().is_err());
    for key in ["weather_type", "weather_intensity"] {
        assert!(serde_json::from_str::<Field>(&format!("\"{key}\"")).is_err());
    }
    for key in ["storms", "extreme_weather", "paradise"] {
        assert!(serde_json::from_str::<PlanetFlag>(&format!("\"{key}\"")).is_err());
    }
}
