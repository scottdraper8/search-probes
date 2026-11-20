//! Owned, query-associated observations. Recipe records do not assert placement.
use crate::address::Address;
use serde::Serialize;

pub const FLORA_COLOURS: u32 = 0x7ff;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticLevel {
    GeneratedRecipe,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    GeneratedSurfaceObjectTable,
}

/// Bits in `known` have been evaluated across all modeled alternatives. `possible`
/// is their union and `guaranteed` their intersection; neither asserts placement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ColourAlternatives {
    pub known: u32,
    pub possible: u32,
    pub guaranteed: u32,
}

impl ColourAlternatives {
    pub fn for_variants(requested: u32) -> Self {
        Self {
            known: requested,
            possible: 0,
            guaranteed: requested,
        }
    }

    pub fn include_variant(&mut self, colours: u32) {
        self.possible |= colours & self.known;
        self.guaranteed &= colours;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SurfaceModelObservation {
    pub source_table: String,
    pub source_index: usize,
    pub object_array: usize,
    pub object_index: usize,
    pub model: String,
    pub source_ground_index: i32,
    pub match_ground_colour: bool,
    pub ground_colour_index: u32,
    pub swap_primary_for_random: bool,
    pub swap_primary_for_secondary: bool,
    pub colours: ColourAlternatives,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurfaceModels {
    pub provenance: Provenance,
    pub semantic_level: SemanticLevel,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_reason: Option<String>,
    pub records: Vec<SurfaceModelObservation>,
}

impl SurfaceModels {
    /// An eligible recipe is not evidence that an object occurs. Only values excluded
    /// by every completely enumerated record are safe to reject in a planet search.
    pub fn predicate_exclusions(&self) -> u32 {
        if !self.complete
            || self
                .records
                .iter()
                .any(|record| !record.complete || record.error.is_some())
        {
            return 0;
        }
        self.records.iter().fold(FLORA_COLOURS, |absent, record| {
            absent & record.colours.known & !record.colours.possible
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanetObservations {
    pub address: Address,
    pub acquisition: ObservationAcquisition,
    pub surface_models: SurfaceModels,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationAcquisition {
    CurrentWorldGeneratedQuery,
}

impl PlanetObservations {
    pub fn validate_address(&self, address: Address) -> Result<(), &'static str> {
        if address.coordinates().planet == 0 || self.address != address {
            return Err("planet observation address does not match inspection");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: usize, colours: ColourAlternatives) -> SurfaceModelObservation {
        SurfaceModelObservation {
            source_table: "surface".into(),
            source_index: 0,
            object_array: 16,
            object_index: index,
            model: "model".into(),
            source_ground_index: 0,
            match_ground_colour: false,
            ground_colour_index: 0,
            swap_primary_for_random: false,
            swap_primary_for_secondary: false,
            colours,
            complete: true,
            error: None,
        }
    }
    fn inventory(records: Vec<SurfaceModelObservation>) -> SurfaceModels {
        SurfaceModels {
            provenance: Provenance::GeneratedSurfaceObjectTable,
            semantic_level: SemanticLevel::GeneratedRecipe,
            complete: true,
            incomplete_reason: None,
            records,
        }
    }
    fn colour(bits: u32) -> ColourAlternatives {
        ColourAlternatives {
            known: FLORA_COLOURS,
            possible: bits,
            guaranteed: bits,
        }
    }
    #[test]
    fn independent_and_coincident_records_survive() {
        let models = inventory(vec![
            record(1, colour(1)),
            record(2, colour(2)),
            record(3, colour(1)),
        ]);
        assert_eq!(models.records.len(), 3);
        assert_eq!(
            models.records[0].colours.guaranteed,
            models.records[2].colours.guaranteed
        );
        assert_ne!(
            models.records[0].object_index,
            models.records[2].object_index
        );
        assert_eq!(models.predicate_exclusions(), FLORA_COLOURS & !3);
    }
    #[test]
    fn alternate_colours_are_possible_but_not_guaranteed() {
        let mut c = ColourAlternatives::for_variants(FLORA_COLOURS);
        c.include_variant(1);
        c.include_variant(2);
        assert_eq!(c.possible, 3);
        assert_eq!(c.guaranteed, 0);
        assert_eq!(
            inventory(vec![record(0, c)]).predicate_exclusions(),
            FLORA_COLOURS & !3
        );
    }
    #[test]
    fn incomplete_enumeration_or_errors_never_prove_absence() {
        let mut models = inventory(vec![record(0, colour(1))]);
        models.complete = false;
        models.records[0].complete = false;
        models.records[0].error = Some("unsupported".into());
        assert_eq!(models.predicate_exclusions(), 0);
        models.complete = true;
        assert_eq!(models.predicate_exclusions(), 0);
        assert_eq!(inventory(vec![]).predicate_exclusions(), FLORA_COLOURS);
    }
    #[test]
    fn exact_planet_association_is_required() {
        let address = Address::from_raw(0).unwrap().planet(0).unwrap();
        let observations = PlanetObservations {
            address,
            acquisition: ObservationAcquisition::CurrentWorldGeneratedQuery,
            surface_models: inventory(vec![]),
        };
        assert!(observations.validate_address(address).is_ok());
        assert!(
            observations
                .validate_address(address.planet(1).unwrap())
                .is_err()
        );
        assert!(observations.validate_address(address.system()).is_err());
    }
}
