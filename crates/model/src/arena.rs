//! Independent, same-creature Arena matching. Only adoptable battle fauna enter this fact set.
use crate::{
    criteria::{ResourceMode, Section},
    facts::Verdict,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArenaField {
    Affinity,
    OverallRank,
    Combat,
    Agility,
    Health,
}
impl ArenaField {
    pub const ALL: &[Self] = &[
        Self::Affinity,
        Self::OverallRank,
        Self::Combat,
        Self::Agility,
        Self::Health,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Affinity => "Affinity",
            Self::OverallRank => "Overall rank",
            Self::Combat => "Combat effectiveness",
            Self::Agility => "Agility",
            Self::Health => "Health",
        }
    }
    pub fn options(self) -> Vec<(usize, &'static str)> {
        if self == Self::Affinity {
            vec![
                (1, "Tropical"),
                (2, "Frost"),
                (3, "Fire"),
                (4, "Toxic"),
                (5, "Desert"),
                (6, "Radioactive"),
                (7, "Anomalous"),
                (8, "Mechanical"),
            ]
        } else {
            vec![(0, "C"), (1, "B"), (2, "A"), (3, "S")]
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArenaCriteria {
    pub presence: Option<bool>,
    pub choices: BTreeMap<ArenaField, u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Selection {
    pub presence: Option<bool>,
    pub masks: [u64; ArenaField::ALL.len()],
    pub mode: ResourceMode,
}
impl ArenaCriteria {
    pub fn compile(
        &self,
        presence_mode: ResourceMode,
        rank_mode: ResourceMode,
    ) -> Result<Vec<Selection>, &'static str> {
        let mut masks = [0; ArenaField::ALL.len()];
        for (&field, &mask) in &self.choices {
            let allowed = field
                .options()
                .into_iter()
                .fold(0, |bits, (n, _)| bits | 1 << n);
            if mask & !allowed != 0 {
                return Err("unsupported Arena choice");
            }
            masks[field as usize] = mask;
        }
        let mut selections = Vec::with_capacity(2);
        if self.presence.is_some() {
            selections.push(Selection {
                presence: self.presence,
                masks: [0; ArenaField::ALL.len()],
                mode: presence_mode,
            });
        }
        if masks.iter().any(|&mask| mask != 0) {
            selections.push(Selection {
                presence: None,
                masks,
                mode: rank_mode,
            });
        }
        Ok(selections)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Ranks {
    pub combat: u8,
    pub agility: u8,
    pub health: u8,
}
impl Ranks {
    pub fn bit(self) -> u64 {
        1 << (self.combat * 16 + self.agility * 4 + self.health)
    }
    pub fn overall(self) -> u8 {
        match self.combat + self.agility + self.health {
            0 => 0,
            1..=3 => 1,
            4..=6 => 2,
            _ => 3,
        }
    }
}
/// Complete same-creature tuples, separating proven outcomes from a conservative upper bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RankEvidence {
    known: u64,
    possible: u64,
}
impl RankEvidence {
    pub fn unknown() -> Self {
        Self {
            known: 0,
            possible: u64::MAX,
        }
    }
    pub fn bounded(possible: u64) -> Result<Self, &'static str> {
        if possible == 0 {
            return Err("eligible Arena creature has no possible ranks");
        }
        Ok(Self { known: 0, possible })
    }
    pub fn exact(ranks: Ranks) -> Result<Self, &'static str> {
        let mut evidence = Self::unknown();
        evidence.observe(ranks)?;
        evidence.possible = evidence.known;
        Ok(evidence)
    }
    /// Record a proven complete tuple for this species and generation. The producer must establish
    /// placement/eligibility; a hypothetical class roll is not an observation.
    pub fn observe(&mut self, ranks: Ranks) -> Result<(), &'static str> {
        if [ranks.combat, ranks.agility, ranks.health]
            .into_iter()
            .any(|r| r > 3)
        {
            return Err("invalid Arena rank witness");
        }
        let bit = ranks.bit();
        if self.possible & bit == 0 {
            return Err("Arena rank witness contradicts its bound");
        }
        self.known |= bit;
        Ok(())
    }
    /// Narrow only with a proven bound; preserve every already established witness.
    pub fn constrain(&mut self, possible: u64) -> Result<(), &'static str> {
        let narrowed = self.possible & possible;
        if narrowed == 0 || self.known & !narrowed != 0 {
            return Err("Arena rank bound contradicts existing evidence");
        }
        self.possible = narrowed;
        Ok(())
    }
    pub fn single(self) -> Option<Ranks> {
        (self.known == self.possible && self.known.count_ones() == 1)
            .then(|| ranks_from_code(self.known.trailing_zeros() as u8))
    }
    pub fn is_complete(self) -> bool {
        self.known == self.possible
    }
}
fn ranks_from_code(code: u8) -> Ranks {
    Ranks {
        combat: code / 16,
        agility: code / 4 % 4,
        health: code % 4,
    }
}
fn tuples(mut bits: u64) -> impl Iterator<Item = Ranks> {
    std::iter::from_fn(move || {
        if bits == 0 {
            return None;
        }
        let code = bits.trailing_zeros() as u8;
        bits &= bits - 1;
        Some(ranks_from_code(code))
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Creature {
    pub affinity: u8,
    pub ranks: RankEvidence,
}
fn combine(values: impl IntoIterator<Item = Verdict>, mode: ResourceMode) -> Verdict {
    let mut result = if mode == ResourceMode::All {
        Verdict::Match
    } else {
        Verdict::NoMatch
    };
    for v in values {
        if (mode == ResourceMode::All && v == Verdict::NoMatch)
            || (mode == ResourceMode::Any && v == Verdict::Match)
        {
            return v;
        }
        if v == Verdict::Unknown {
            result = v;
        }
    }
    result
}
impl Selection {
    pub fn section(self) -> Section {
        if self.masks.iter().any(|&mask| mask != 0) {
            Section::XenoArena
        } else {
            Section::GeneralFauna
        }
    }

    fn creature(self, affinity: u8, ranks: Ranks) -> Verdict {
        let values = [
            affinity,
            ranks.overall(),
            ranks.combat,
            ranks.agility,
            ranks.health,
        ];
        combine(
            self.masks
                .into_iter()
                .zip(values)
                .filter(|(m, _)| *m != 0)
                .map(|(m, v)| {
                    if v < 64 && m & (1 << v) != 0 {
                        Verdict::Match
                    } else {
                        Verdict::NoMatch
                    }
                }),
            self.mode,
        )
    }
    pub fn evaluate(self, creatures: &[Creature]) -> Verdict {
        let fields = self.masks.iter().any(|&m| m != 0);
        let presence = self.presence.map(|expected| {
            if !creatures.is_empty() == expected {
                Verdict::Match
            } else {
                Verdict::NoMatch
            }
        });
        let found = fields.then(|| {
            combine(
                creatures.iter().map(|c| {
                    if let Some(ranks) = c.ranks.single() {
                        return self.creature(c.affinity, ranks);
                    }
                    if tuples(c.ranks.known)
                        .any(|ranks| self.creature(c.affinity, ranks) == Verdict::Match)
                    {
                        return Verdict::Match;
                    }
                    // A unanimous verdict is exact without knowing the placement. Disagreement
                    // needs a placement observation; existential acceptance of this upper bound
                    // would invent a creature variant that may not exist on this planet.
                    let mut verdict = None;
                    for ranks in tuples(c.ranks.possible) {
                        let current = self.creature(c.affinity, ranks);
                        if verdict.is_some_and(|v| v != current) {
                            return Verdict::Unknown;
                        }
                        verdict = Some(current);
                    }
                    verdict.unwrap_or(Verdict::Unknown)
                }),
                ResourceMode::Any,
            )
        });
        // Eligibility is structural: even Any mode only evaluates fields on eligible creatures.
        combine(presence.into_iter().chain(found), self.mode)
    }
}
