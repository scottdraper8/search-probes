use search_probes_model::{address::Address, cache::MAX_NAME_BYTES};

type Generate = unsafe extern "system" fn(*const u8, u64, *mut u8, *mut u8);

/// # Safety
/// Requires the verified 7.06 image and its initialized application on the gameplay thread.
/// The native generator reads shared naming tables; all output and RNG storage belongs to this call.
pub unsafe fn generate(base: usize, address: Address) -> Result<String, &'static str> {
    let function = base + 0xe9f460;
    let prefix = [0x48, 0x89, 0x5c, 0x24, 0x18, 0x4c, 0x89, 0x4c, 0x24, 0x20];
    if unsafe { std::slice::from_raw_parts(function as *const u8, prefix.len()) } != prefix {
        return Err("system name generator prefix mismatch");
    }
    let application = unsafe { ((base + 0x6f60828) as *const *const u8).read() };
    if application.is_null() {
        return Err("system name generator has no application");
    }
    let function = unsafe { std::mem::transmute::<usize, Generate>(function) };
    unsafe { invoke(function, application.add(0xcd0), address) }
}

unsafe fn invoke(
    function: Generate,
    generator: *const u8,
    address: Address,
) -> Result<String, &'static str> {
    let mut name = [0u8; 128];
    let mut localized = [0u8; 128];
    unsafe {
        function(
            generator,
            address.system().raw(),
            name.as_mut_ptr(),
            localized.as_mut_ptr(),
        );
    }
    let bytes = if localized[0] != 0 { &localized } else { &name };
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("unterminated system name")?;
    if end == 0 {
        return Err("native system name is empty");
    }
    let name = String::from_utf8_lossy(&bytes[..end]);
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    Ok(name[..end].into())
}
