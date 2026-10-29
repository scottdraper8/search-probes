use crate::{Result, files};
use quick_xml::{Reader, events::Event};
use std::{collections::BTreeSet, path::Path};

fn properties(path: &Path, mission: Option<&str>) -> Result<Vec<(String, String)>> {
    let bytes = std::fs::read(path)?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    let mut result = Vec::new();
    let mut depth = 0;
    let mut selected = None;
    loop {
        let event = reader.read_event()?;
        if matches!(event, Event::Start(_)) {
            depth += 1;
        }
        match event {
            Event::Start(element) | Event::Empty(element)
                if element.name().as_ref() == b"Property" =>
            {
                let mut name = None;
                let mut value = None;
                let mut id = None;
                for attribute in element.attributes() {
                    let attribute = attribute?;
                    let text = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Explicit1_0,
                            reader.decoder(),
                        )?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"name" => name = Some(text),
                        b"value" => value = Some(text),
                        b"_id" => id = Some(text),
                        _ => {}
                    }
                }
                if mission.is_some() && id.as_deref() == mission {
                    selected = Some(depth);
                }
                if mission.is_none() || selected.is_some() {
                    if let (Some(name), Some(value)) = (name, value) {
                        result.push((name, value));
                    }
                }
            }
            Event::End(_) => {
                if selected == Some(depth) {
                    selected = None;
                }
                depth -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

pub fn validate(root: &Path) -> Result<()> {
    let expected: BTreeSet<_> = [
        "LocTable.MXML",
        "METADATA/REALITY/WIKI.EXML",
        "METADATA/SIMULATION/MISSIONS/TABLES/WIKIMISSIONTABLE.EXML",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let actual: BTreeSet<_> = files(root)?
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect::<std::result::Result<_, _>>()?;
    if actual != expected {
        return Err("unexpected asset inventory".into());
    }
    let wiki = properties(&root.join("METADATA/REALITY/WIKI.EXML"), None)?;
    let table = root.join("METADATA/SIMULATION/MISSIONS/TABLES/WIKIMISSIONTABLE.EXML");
    let mut navigation = Vec::new();
    let localization = properties(&root.join("LocTable.MXML"), None)?;
    let values = |items: &[(String, String)], key: &str| -> Vec<String> {
        items
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .collect()
    };
    let missions: Vec<_> = (0..search_probes_model::settings::MAX_PRESETS)
        .map(search_probes_model::guide::mission)
        .collect::<std::result::Result<_, _>>()?;
    let mut guide = Vec::new();
    for mission in &missions {
        guide.extend(properties(&table, Some(mission))?);
    }
    let mut expected = missions.clone();
    expected.extend(
        search_probes_model::target::TargetKind::ALL
            .into_iter()
            .map(|kind| kind.mission().to_owned()),
    );
    if values(&properties(&table, None)?, "MissionID") != expected {
        return Err("unexpected mission table entries".into());
    }
    if values(&wiki, "Mission") != missions
        || values(&guide, "MissionID") != missions
        || !values(&guide, "Name").is_empty()
        || !values(&guide, "Event").is_empty()
    {
        return Err("Guide slot identities do not match the native dispatcher".into());
    }
    for kind in search_probes_model::target::TargetKind::ALL {
        use search_probes_model::target::TargetKind;
        let target = properties(&table, Some(kind.mission()))?;
        let (search, location, conditions) = match kind {
            TargetKind::Planet => (
                "Any",
                "PlanetSearch",
                vec![
                    "GcMissionConditionIsScanEventLocal",
                    "GcMissionConditionIsScanEventOnCurrentPlanet",
                ],
            ),
            TargetKind::System => (
                "SpaceMarker",
                "Nearest",
                vec!["GcMissionConditionIsScanEventLocal"],
            ),
        };
        if values(&target, "MissionID") != [kind.mission()]
            || values(&target, "Name") != [kind.event()]
            || values(&target, "UseScanEventDetailsInLogInfo") != ["true"]
            || values(&target, "ObjectivesCanBeFormattedBySequences") != ["true"]
            || values(&target, "FormattableObjective") != values(&target, "ObjectiveID")
            || values(&target, "FormattableObjectiveTip") != values(&target, "ObjectiveTipID")
            || values(&target, "UAsList") != ["0000000000000000"]
            || values(&target, "SolarSystemLocation") != ["FromList"]
            || values(&target, "SearchType") != [search]
            || values(&target, "BuildingLocation") != [location]
            || values(&target, "Conditions") != conditions
            || values(&target, "Event")
                .iter()
                .any(|event| event != kind.event())
        {
            return Err("navigation assets do not match the native destination contract".into());
        }
        let mut stages = vec![
            "GcMissionSequenceEndScanEvent",
            "GcMissionSequenceStartScanEvent",
            "GcMissionSequenceGroup",
            "GcMissionSequenceWaitForConditions",
        ];
        if kind != TargetKind::System {
            stages.push("GcMissionSequenceWaitForConditions");
        }
        stages.extend([
            "GcMissionSequenceEndScanEvent",
            "GcMissionSequenceShowMissionUpdateMessage",
        ]);
        if values(&target, "Stage") != stages {
            return Err("unsupported destination lifecycle".into());
        }
        navigation.extend(target);
    }
    if !values(&guide, "Stage").is_empty()
        || values(&navigation, "NeedsBiome")
            .iter()
            .any(|v| v != "false")
    {
        return Err("unexpected Guide stage or target biome restriction".into());
    }
    let ids: BTreeSet<_> = values(&localization, "Id").into_iter().collect();
    for (name, value) in wiki.iter().chain(guide.iter()).chain(navigation.iter()) {
        if value.starts_with("SQN_SP_")
            && !matches!(name.as_str(), "Mission" | "MissionID")
            && !ids.contains(value)
        {
            return Err(format!("missing localization {value}").into());
        }
    }
    Ok(())
}
