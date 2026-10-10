pub const TERRAIN_FEATURES: &[&str] = &[
    "Floating islands",
    "Mountains",
    "Plateaus",
    "Canyons",
    "Craters",
    "Arches",
    "Caves",
];

pub const FLOATING_ISLANDS: u32 = 1;
pub const FEATURE_SET: u32 = 1 << 31;
pub const ALL_FEATURES: u32 = (1 << TERRAIN_FEATURES.len()) - 1;

#[derive(Clone, Copy, Default)]
pub struct NoiseLayer {
    pub active: bool,
    pub subtract: bool,
    pub height: f32,
    pub width: f32,
    pub region_ratio: f32,
    pub region_scale: f32,
    pub plateau_stratas: f32,
}

impl NoiseLayer {
    fn positive(self) -> bool {
        self.active
            && !self.subtract
            && self.height > 0.
            && self.width > 0.
            && self.region_ratio > 0.
            && self.region_scale > 0.
    }
}

#[derive(Clone, Copy, Default)]
pub struct Shape {
    pub active: bool,
    pub subtract: bool,
    pub trench: bool,
    pub kind: u32,
    pub height: f32,
    pub width: f32,
    pub ratio: f32,
    pub region_size: f32,
}

impl Shape {
    fn positive(self) -> bool {
        self.active
            && self.height > 0.
            && self.width > 0.
            && self.ratio > 0.
            && self.region_size > 0.
    }
}

#[derive(Default)]
pub struct Terrain {
    pub noise: [NoiseLayer; 8],
    pub shapes: [Shape; 7],
    pub caves: [Shape; 2],
}

impl Terrain {
    pub fn features(&self) -> u32 {
        let mut features = 0;
        if self.noise[2].positive() {
            features |= 1 << 1;
        }
        if self
            .noise
            .iter()
            .any(|layer| layer.positive() && layer.plateau_stratas > 1.)
        {
            features |= 1 << 2;
        }
        if self
            .shapes
            .iter()
            .any(|s| s.positive() && s.kind == 0 && s.subtract && s.trench)
        {
            features |= 1 << 3;
        }
        let crater = self.shapes[1];
        if crater.positive() && crater.kind == 1 && crater.subtract && !crater.trench {
            features |= 1 << 4;
        }
        if self.shapes[2..4]
            .iter()
            .any(|s| s.positive() && s.kind == 0 && !s.subtract && !s.trench)
        {
            features |= 1 << 5;
        }
        if self.caves.iter().all(|s| s.positive() && s.subtract) {
            features |= 1 << 6;
        }
        features
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(kind: u32, subtract: bool, trench: bool) -> Shape {
        Shape {
            active: true,
            kind,
            subtract,
            trench,
            height: 100.,
            width: 50.,
            ratio: 1.,
            region_size: 800.,
        }
    }

    #[test]
    fn disabled_or_zero_sized_shapes_do_not_match() {
        let mut terrain = Terrain::default();
        terrain.shapes[1] = shape(1, true, false);
        assert_eq!(terrain.features(), 1 << 4);
        terrain.shapes[1].active = false;
        assert_eq!(terrain.features(), 0);
        terrain.shapes[1].active = true;
        terrain.shapes[1].width = 0.;
        assert_eq!(terrain.features(), 0);
    }

    #[test]
    fn shape_geometry_distinguishes_canyons_craters_arches_and_caves() {
        let mut terrain = Terrain::default();
        terrain.shapes[0] = shape(1, false, false);
        assert_eq!(terrain.features(), 0);
        terrain.shapes[0] = shape(0, true, true);
        terrain.shapes[1] = shape(1, true, false);
        terrain.shapes[2] = shape(0, false, false);
        terrain.caves = [shape(1, true, false), shape(0, true, false)];
        assert_eq!(terrain.features(), 0b111_1000);
        terrain.caves[1].active = false;
        assert_eq!(terrain.features(), 0b011_1000);
    }

    #[test]
    fn plateaus_include_underwater_geometry_but_require_multiple_strata() {
        let mut terrain = Terrain::default();
        let layer = NoiseLayer {
            active: true,
            height: 128.,
            width: 350.,
            region_ratio: 0.2,
            region_scale: 1.,
            plateau_stratas: 2.,
            ..Default::default()
        };
        terrain.noise[4] = layer;
        assert_eq!(terrain.features(), 1 << 2);
        terrain.noise[4].active = false;
        terrain.noise[7] = NoiseLayer {
            plateau_stratas: 1.,
            ..layer
        };
        assert_eq!(terrain.features(), 0);
        terrain.noise[2] = layer;
        assert_eq!(terrain.features(), (1 << 1) | (1 << 2));
        terrain.noise[2].active = false;
        assert_eq!(terrain.features(), 0);
    }
}
