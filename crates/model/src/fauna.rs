use crate::{
    criteria::{ResourceMode, Section},
    facts::Verdict,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_SPECIES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Aquatic,
    Land,
    Airborne,
}
impl Domain {
    pub const ALL: &[Self] = &[Self::Aquatic, Self::Land, Self::Airborne];
    pub fn label(self) -> &'static str {
        match self {
            Self::Aquatic => "Aquatic Fauna",
            Self::Land => "Land Fauna",
            Self::Airborne => "Airborne Fauna",
        }
    }
    pub fn section(self) -> Section {
        match self {
            Self::Aquatic => Section::AquaticFauna,
            Self::Land => Section::LandFauna,
            Self::Airborne => Section::AirborneFauna,
        }
    }
    pub fn presence_label(self) -> &'static str {
        match self {
            Self::Aquatic => "Has aquatic fauna",
            Self::Land => "Has land fauna",
            Self::Airborne => "Has airborne fauna",
        }
    }
    pub fn bit(self) -> u8 {
        1 << self as u8
    }
}

macro_rules! families {
    ($($family:ident => ($label:literal, $domains:expr, $category:expr, [$($id:literal),+])) ,+ $(,)?) => {
        #[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Serialize)]
        pub enum Family { $($family),+ }
        impl Family {
            pub const ALL: &[Self] = &[$(Self::$family),+];
            pub fn label(self) -> &'static str { match self { $(Self::$family => $label),+ } }
            pub fn domains(self) -> u8 { match self { $(Self::$family => $domains),+ } }
            pub fn category(self) -> u8 { match self { $(Self::$family => $category),+ } }
            pub fn from_id(id: &[u8]) -> Option<Self> { match id { $($($id)|+ => Some(Self::$family),)+ _ => None } }
        }
    }
}
// Derived body-family mappings from the current creature catalogue. Flock and luminous versions
// share a family. Placement can override model routing (underwater crabs); no role/type shortcut
// is used for airborne classification.
families! {
    Fish => ("Fish", 1, 0, [b"FISH",b"FISHFLOCK"]),
    SwimmingCattle => ("Swimming cattle", 1, 0, [b"SWIMCOW"]),
    SwimmingRodents => ("Swimming rodents", 1, 0, [b"SWIMRODENT"]),
    Jellyfish => ("Jellyfish", 1, 0, [b"JELLYFISH"]),
    Crabs => ("Crabs", 3, 0, [b"CRAB"]),
    SeaSnakes => ("Sea snakes", 1, 0, [b"SEASNAKE"]),
    Sharks => ("Sharks", 1, 0, [b"SHARK"]),
    Birds => ("Birds", 4, 0, [b"BIRD"]),
    FlyingSnakes => ("Flying snakes", 4, 0, [b"FLYINGSNAKE"]),
    FlyingLizards => ("Flying lizards", 4, 0, [b"FLYINGLIZARD"]),
    Butterflies => ("Butterflies", 4, 0, [b"BUTTERFLY",b"BUTTERFLOCK"]),
    SmallBirds => ("Small birds", 4, 0, [b"SMALLBIRD"]),
    LargeButterflies => ("Large butterflies", 4, 0, [b"LARGEBUTTERFLY"]),
    FlyingBeetles => ("Flying beetles", 4, 0, [b"FLYINGBEETLE"]),
    Antelopes => ("Antelopes", 2, 0, [b"ANTELOPE"]),
    RobotAntelopes => ("Robotic antelopes", 2, 1, [b"ROBOTANTELOPE"]),
    Triceratops => ("Triceratops", 2, 0, [b"TRICERATOPS"]),
    Rodents => ("Rodents", 2, 0, [b"RODENT"]),
    Moles => ("Moles", 2, 0, [b"MOLE"]),
    Cattle => ("Cattle", 2, 0, [b"COW"]),
    Cats => ("Cats", 2, 0, [b"CAT"]),
    PlantCats => ("Plant cats", 2, 3, [b"PLANTCAT"]),
    SkeletalCats => ("Skeletal cats", 2, 2, [b"BONECAT"]),
    Striders => ("Striders", 2, 0, [b"STRIDER",b"STRIDERGLOW"]),
    Tyrannosaurs => ("Tyrannosaurs", 2, 0, [b"TREX"]),
    BipedalAntelopes => ("Bipedal antelopes", 2, 0, [b"TWOLEGANTELOPE"]),
    SixLeggedCattle => ("Six-legged cattle", 2, 0, [b"SIXLEGCOW"]),
    SixLeggedCats => ("Six-legged cats", 2, 0, [b"SIXLEGCAT"]),
    Grunts => ("Grunts", 2, 0, [b"GRUNT"]),
    Blobs => ("Blobs", 2, 0, [b"BLOB"]),
    Spiders => ("Spiders", 2, 0, [b"SPIDER"]),
    Arthropods => ("Arthropods", 2, 0, [b"ARTHROPOD"]),
    WalkingBuildings => ("Walking constructs", 2, 1, [b"WALKINGBUILDING"]),
    FloatingSpiders => ("Floating spiders", 2, 0, [b"FLOATSPIDER"]),
    Rollers => ("Rollers", 2, 4, [b"PROTOROLLER"]),
    AnomalousFlyers => ("Anomalous flyers", 4, 4, [b"PROTOFLYER"]),
    Diggers => ("Diggers", 2, 4, [b"PROTODIGGER"]),
    Ploughs => ("Ploughs", 2, 4, [b"PLOUGH"]),
    Drills => ("Drills", 2, 4, [b"DRILL"]),
    RollingAnomalies => ("Rolling anomalies", 2, 4, [b"WEIRDROLL"]),
    FloatingAnomalies => ("Floating anomalies", 2, 4, [b"WEIRDFLOAT"]),
    CrystalAnomalies => ("Crystal anomalies", 2, 4, [b"WEIRDCRYSTAL"]),
    AnomalousButterflies => ("Anomalous butterflies", 4, 4, [b"WEIRDBUTTERFLY"]),
    GeometricAnomalies => ("Geometric anomalies", 4, 4, [b"PURPLE_WEIRD"]),
    DeepFish => ("Deep-sea fish", 1, 0, [b"DEEPFISH",b"DEEPFISHFLOCK"]),
    LargeDeepFish => ("Large deep-sea fish", 1, 0, [b"DEEPFISHLARGE"]),
    Mantas => ("Mantas", 1, 0, [b"MANTA",b"MANTAGLOW"]),
    Seahorses => ("Seahorses", 1, 0, [b"SEAHORSE",b"SEAHORSEGLOW"]),
    Squid => ("Squid", 1, 0, [b"SQUID"]),
    HermitCrabs => ("Hermit crabs", 3, 0, [b"HERMITCRAB"]),
    Prawns => ("Prawns", 1, 0, [b"PRAWN"]),
    SkeletalCattle => ("Skeletal cattle", 2, 2, [b"BONECOW"]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaunaField {
    Family,
    Category,
    Activity,
    Rarity,
    Habitat,
    Hemisphere,
}
impl FaunaField {
    pub const ALL: &[Self] = &[
        Self::Family,
        Self::Category,
        Self::Activity,
        Self::Rarity,
        Self::Habitat,
        Self::Hemisphere,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Family => "Body family",
            Self::Category => "Category",
            Self::Activity => "Activity time",
            Self::Rarity => "Rarity",
            Self::Habitat => "Habitat",
            Self::Hemisphere => "Hemisphere",
        }
    }
    pub fn options(self, domain: Domain) -> Vec<(usize, &'static str)> {
        match self {
            Self::Family => Family::ALL
                .iter()
                .filter(|f| f.domains() & domain.bit() != 0)
                .map(|f| (*f as usize, f.label()))
                .collect(),
            Self::Category if domain == Domain::Aquatic => vec![],
            Self::Category => ["Ordinary", "Robotic", "Skeletal", "Plant-like", "Anomalous"]
                .into_iter()
                .enumerate()
                .filter(|(n, _)| {
                    Family::ALL
                        .iter()
                        .any(|f| f.domains() & domain.bit() != 0 && f.category() as usize == *n)
                })
                .collect(),
            Self::Activity => [
                "Day only",
                "Mostly day",
                "Day and night",
                "Mostly night",
                "Night only",
            ]
            .into_iter()
            .enumerate()
            .collect(),
            Self::Rarity => ["Common", "Uncommon", "Rare"]
                .into_iter()
                .enumerate()
                .collect(),
            Self::Habitat if domain == Domain::Land => vec![(0, "Surface"), (1, "Cave")],
            Self::Habitat => vec![],
            Self::Hemisphere => vec![(0, "Both"), (1, "North only"), (2, "South only")],
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeightRange {
    pub minimum_cm: Option<u32>,
    pub maximum_cm: Option<u32>,
}
impl HeightRange {
    pub fn active(self) -> bool {
        self.minimum_cm.is_some() || self.maximum_cm.is_some()
    }
    fn matches(self, heights: [i32; 2]) -> bool {
        heights.into_iter().any(|h| {
            self.minimum_cm
                .is_none_or(|min| i64::from(h) >= i64::from(min))
                && self
                    .maximum_cm
                    .is_none_or(|max| i64::from(h) <= i64::from(max))
        })
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FaunaCriteria {
    pub presence: Option<bool>,
    pub choices: BTreeMap<FaunaField, u64>,
    pub height: HeightRange,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Selection {
    pub domain: Domain,
    pub presence: Option<bool>,
    pub masks: [u64; FaunaField::ALL.len()],
    pub height: HeightRange,
    pub mode: ResourceMode,
}
impl FaunaCriteria {
    pub fn compile(
        &self,
        domain: Domain,
        mode: ResourceMode,
    ) -> Result<Option<Selection>, &'static str> {
        let mut masks = [0; FaunaField::ALL.len()];
        for (&field, &mask) in &self.choices {
            let allowed = field
                .options(domain)
                .into_iter()
                .fold(0, |bits, (n, _)| bits | 1u64 << n);
            if mask & !allowed != 0 {
                return Err("unsupported fauna choice in this habitat");
            }
            masks[field as usize] = mask;
        }
        if matches!((self.height.minimum_cm,self.height.maximum_cm),(Some(a),Some(b)) if a>b) {
            return Err("minimum fauna height exceeds maximum height");
        }
        if self.height.minimum_cm.is_some_and(|n| n > 100_000)
            || self.height.maximum_cm.is_some_and(|n| n > 100_000)
        {
            return Err("fauna height exceeds supported range");
        }
        Ok(
            (self.presence.is_some() || masks.iter().any(|m| *m != 0) || self.height.active())
                .then_some(Selection {
                    domain,
                    presence: self.presence,
                    masks,
                    height: self.height,
                    mode,
                }),
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Species {
    pub family: Family,
    pub domain: Domain,
    pub activity: u8,
    pub rarity: u8,
    pub cave: bool,
    pub hemisphere: u8,
    // Native discovery metadata adds signed jitter to the catalogue scale. Small schooling
    // fish can therefore have negative values; preserve them rather than clamping or wrapping.
    pub heights_cm: [i32; 2],
}
impl Selection {
    pub fn evaluate(self, species: &[Species]) -> Verdict {
        let has_fields = self.masks.iter().any(|m| *m != 0) || self.height.active();
        let presence = self
            .presence
            .map(|expected| species.iter().any(|s| s.domain == self.domain) == expected);
        let found = species.iter().filter(|s| s.domain == self.domain).any(|s| {
            let mut all = true;
            let mut any = false;
            for (index, value) in [
                s.family as usize,
                s.family.category() as usize,
                s.activity as usize,
                s.rarity as usize,
                usize::from(s.cave),
                s.hemisphere as usize,
            ]
            .into_iter()
            .enumerate()
            {
                if self.masks[index] != 0 {
                    let accepted = self.masks[index] & (1u64 << value) != 0;
                    all &= accepted;
                    any |= accepted;
                }
            }
            if self.height.active() {
                let accepted = self.height.matches(s.heights_cm);
                all &= accepted;
                any |= accepted;
            }
            match self.mode {
                ResourceMode::All => all,
                ResourceMode::Any => any,
            }
        });
        let accepted = match self.mode {
            ResourceMode::All => presence.unwrap_or(true) && (!has_fields || found),
            ResourceMode::Any => presence.unwrap_or(false) || (has_fields && found),
        };
        if accepted {
            Verdict::Match
        } else {
            Verdict::NoMatch
        }
    }
}
