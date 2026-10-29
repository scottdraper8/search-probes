//! Scalar inputs written by the native generated-role spawn constructor, before model loading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelScalars {
    pub adult_scale: f32,
    pub infant_scale: f32,
    pub dependency_flag: bool,
}
#[derive(Clone, Copy)]
pub struct Parameters {
    pub maximum: f32,
    pub quarter: f32,
    pub uniform: f64,
    pub infant_multiplier: f32,
}
#[derive(Clone, Copy)]
pub struct Catalogue {
    pub minimum_scale: f32,
    pub maximum_scale: f32,
    pub dependency_probability: f32,
}
impl Catalogue {
    pub fn from_native_bytes(bytes: &[u8; 0xd0]) -> Self {
        Self {
            minimum_scale: f32::from_le_bytes(bytes[0xa8..0xac].try_into().unwrap()),
            maximum_scale: f32::from_le_bytes(bytes[0xa4..0xa8].try_into().unwrap()),
            dependency_probability: f32::from_le_bytes(bytes[0x94..0x98].try_into().unwrap()),
        }
    }
}
pub fn calculate(
    role: &[u8; 0x5d8],
    catalogue: Catalogue,
    parameters: Parameters,
) -> Result<ModelScalars, &'static str> {
    let seed = u64::from_le_bytes(role[0x5b0..0x5b8].try_into().unwrap());
    let valid = role[0x5b8];
    let minimum = u32::from_le_bytes(role[0x574..0x578].try_into().unwrap());
    let maximum = u32::from_le_bytes(role[0x56c..0x570].try_into().unwrap());
    let minimum_scale = catalogue.minimum_scale;
    let maximum_scale = catalogue.maximum_scale;
    let probability = catalogue.dependency_probability;
    if valid > 1 || minimum > maximum || maximum > 3 {
        return Err("invalid source role size bounds");
    }
    if ![
        parameters.maximum,
        parameters.quarter,
        parameters.infant_multiplier,
        minimum_scale,
        maximum_scale,
    ]
    .into_iter()
    .all(|v| v.is_finite() && v > 0.)
        || !parameters.uniform.is_finite()
        || parameters.uniform <= 0.
        || !probability.is_finite()
        || maximum_scale < minimum_scale
    {
        return Err("invalid source model scale parameters");
    }
    let top = parameters.maximum.min(maximum_scale);
    let low = seed as u32;
    let (lo, hi) = if valid != 0 {
        (
            low.wrapping_add(u32::from(low == 0)),
            low.rotate_right(16) ^ (seed >> 32) as u32 ^ low,
        )
    } else {
        (1, 0)
    };
    let next = u64::from(lo)
        .wrapping_mul(0x5a76f899)
        .wrapping_add(u64::from(hi));
    let first = ((next as u32) as f64 * parameters.uniform) as f32;
    let second_low = (next as u32)
        .wrapping_mul(0x5a76f899)
        .wrapping_add((next >> 32) as u32);
    let second = (second_low as f64 * parameters.uniform) as f32;
    let span = top - minimum_scale;
    let bottom = (minimum as f32 * parameters.quarter) * span + minimum_scale;
    let upper = ((maximum + 1) as f32 * parameters.quarter) * span + minimum_scale;
    let adult_scale = ((upper - bottom) * first + bottom)
        .max(minimum_scale)
        .min(top);
    let infant_scale = parameters.infant_multiplier * adult_scale;
    if !adult_scale.is_finite()
        || adult_scale <= 0.
        || !infant_scale.is_finite()
        || infant_scale <= 0.
    {
        return Err("unrepresentable source model scale");
    }
    Ok(ModelScalars {
        adult_scale,
        infant_scale,
        dependency_flag: probability > second,
    })
}
