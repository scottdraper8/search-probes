use crate::settings::MAX_PRESETS;

pub fn mission(slot: usize) -> Result<String, &'static str> {
    validate(slot)?;
    Ok(format!("SQN_SP_P{:02}", slot + 1))
}

fn validate(slot: usize) -> Result<(), &'static str> {
    if slot < MAX_PRESETS {
        Ok(())
    } else {
        Err("invalid Guide preset slot")
    }
}

pub fn mission_slot(name: &[u8; 16]) -> Option<usize> {
    let prefix = b"SQN_SP_P0";
    if name[..prefix.len()] != *prefix
        || !(b'1'..=b'9').contains(&name[prefix.len()])
        || name[prefix.len() + 1..].iter().any(|&byte| byte != 0)
    {
        return None;
    }
    Some(usize::from(name[prefix.len()] - b'1'))
}
