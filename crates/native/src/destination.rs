use search_probes_model::{
    address::Address,
    target::{Target, TargetKind},
};

pub fn from_seed(
    kind: TargetKind,
    fauna_role: Option<u8>,
    seed: u64,
) -> Result<Target, &'static str> {
    let address = Address::from_raw(seed)?;
    let target = Target {
        address,
        kind,
        fauna_role,
    };
    if target.validate().is_err() {
        return Err("Invalid Search Probes destination. Abandon the mission in the Log.");
    }
    Ok(target)
}

pub fn seed(target: Target) -> Result<u64, &'static str> {
    target.validate()?;
    Ok(target.address.raw())
}

pub fn encode(address: Address) -> [u8; 17] {
    let mut encoded = [0; 17];
    for (index, byte) in encoded[..16].iter_mut().enumerate() {
        *byte = b"0123456789ABCDEF"[((address.raw() >> ((15 - index) * 4)) & 15) as usize];
    }
    encoded
}
