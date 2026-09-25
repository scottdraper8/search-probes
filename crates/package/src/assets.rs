use crate::{Result, files};
use quick_xml::{Reader, events::Event};
use std::{collections::BTreeSet, path::Path};

fn properties(path: &Path) -> Result<Vec<(String, String)>> {
    let bytes = std::fs::read(path)?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    let mut result = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(element) | Event::Empty(element)
                if element.name().as_ref() == b"Property" =>
            {
                let mut name = None;
                let mut value = None;
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
                        _ => {}
                    }
                }
                if let (Some(name), Some(value)) = (name, value) {
                    result.push((name, value));
                }
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
        "METADATA/SIMULATION/MISSIONS/TABLES/NPCMISSIONTABLE.EXML",
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
    let wiki = properties(&root.join("METADATA/REALITY/WIKI.EXML"))?;
    let guide =
        properties(&root.join("METADATA/SIMULATION/MISSIONS/TABLES/WIKIMISSIONTABLE.EXML"))?;
    let navigation =
        properties(&root.join("METADATA/SIMULATION/MISSIONS/TABLES/NPCMISSIONTABLE.EXML"))?;
    let localization = properties(&root.join("LocTable.MXML"))?;
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
    if values(&wiki, "Mission") != missions
        || values(&guide, "MissionID") != missions
        || !values(&guide, "Name").is_empty()
        || !values(&guide, "Event").is_empty()
    {
        return Err("Guide slot identities do not match the native dispatcher".into());
    }
    if values(&navigation, "MissionID") != ["SQN_SP_NAV"]
        || values(&navigation, "Name") != ["SE_SQN_SP_TARGET"]
        || values(&navigation, "UAsList") != ["0000000000000000"]
        || values(&navigation, "SolarSystemLocation") != ["FromList"]
    {
        return Err("navigation assets do not match the native destination contract".into());
    }
    let stages = values(&navigation, "Stage");
    if stages
        != [
            "GcMissionSequenceEndScanEvent",
            "GcMissionSequenceStartScanEvent",
            "GcMissionSequenceGroup",
            "GcMissionSequenceWaitForConditions",
            "GcMissionSequenceWaitForConditions",
            "GcMissionSequenceEndScanEvent",
            "GcMissionSequenceShowMissionUpdateMessage",
        ]
    {
        return Err("unsupported destination lifecycle".into());
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

#[cfg(test)]
mod tests {
    #[test]
    fn native_asset_contracts_match_the_runtime() {
        super::validate(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"))
            .unwrap();
    }
}
