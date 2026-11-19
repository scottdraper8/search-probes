use crate::settings::MAX_PRESETS;

pub fn mission(slot: usize) -> Result<String, &'static str> {
    validate(slot)?;
    Ok(format!("SQN_SP_P{:02}", slot + 1))
}

pub fn topic(slot: usize) -> Result<String, &'static str> {
    validate(slot)?;
    Ok(format!("SQN_SP_SLOT{}", slot + 1))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_slots_have_matching_mission_and_topic_identities() {
        for slot in 0..MAX_PRESETS {
            let mission = mission(slot).unwrap();
            let mut identity = [0; 16];
            identity[..mission.len()].copy_from_slice(mission.as_bytes());
            assert_eq!(mission_slot(&identity), Some(slot));
            assert_eq!(topic(slot).unwrap(), format!("SQN_SP_SLOT{}", slot + 1));
        }
        assert!(mission(MAX_PRESETS).is_err());
        assert!(topic(MAX_PRESETS).is_err());
    }
}
