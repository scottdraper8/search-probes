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
