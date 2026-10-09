mod hdr;
mod iat;
mod input;
mod paint;

use ash::{vk, vk::Handle};
use search_probes_model::controls::Controls;
use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

static PRESENTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ERROR: Mutex<Option<String>> = Mutex::new(None);
static DRAWN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
static IMPORTS: OnceLock<Mutex<Vec<iat::Import>>> = OnceLock::new();
static RUNNING: AtomicBool = AtomicBool::new(false);
static CONTROLS: OnceLock<Arc<Controls>> = OnceLock::new();
static DISPATCH: [OnceLock<Dispatch>; 2] = [OnceLock::new(), OnceLock::new()];

struct Dispatch {
    module: usize,
    name: &'static str,
    gipa: vk::PFN_vkGetInstanceProcAddr,
    gdpa: vk::PFN_vkGetDeviceProcAddr,
    present: vk::PFN_vkQueuePresentKHR,
    create_instance: vk::PFN_vkCreateInstance,
    create_device: vk::PFN_vkCreateDevice,
}
impl Dispatch {
    unsafe fn load(
        module: windows_sys::Win32::Foundation::HMODULE,
        name: &'static str,
    ) -> Result<Self, String> {
        let get = |symbol: &CStr| {
            unsafe { GetProcAddress(module, symbol.as_ptr().cast()) }
                .ok_or_else(|| format!("{name} is missing {}", symbol.to_string_lossy()))
        };
        Ok(Self {
            module: module as usize,
            name,
            gipa: unsafe { std::mem::transmute(get(c"vkGetInstanceProcAddr")?) },
            gdpa: unsafe { std::mem::transmute(get(c"vkGetDeviceProcAddr")?) },
            present: unsafe { std::mem::transmute(get(c"vkQueuePresentKHR")?) },
            create_instance: unsafe { std::mem::transmute(get(c"vkCreateInstance")?) },
            create_device: unsafe { std::mem::transmute(get(c"vkCreateDevice")?) },
        })
    }
}
fn dispatch<const KIND: usize>() -> &'static Dispatch {
    DISPATCH[KIND].get().expect("installed Vulkan dispatcher")
}

struct Device {
    physical: vk::PhysicalDevice,
    instance: ash::Instance,
    device: ash::Device,
    swapchain: ash::khr::swapchain::Device,
}

struct Chain {
    device: u64,
    extent: vk::Extent2D,
    format: vk::Format,
    colour_space: vk::ColorSpaceKHR,
    usage: vk::ImageUsageFlags,
    hwnd: usize,
    painter: Option<paint::Painter>,
    failed: bool,
}

struct Registry {
    physical: BTreeMap<u64, ash::Instance>,
    devices: BTreeMap<u64, Device>,
    queues: BTreeMap<u64, (u64, u32, vk::QueueFlags)>,
    surfaces: BTreeMap<u64, usize>,
    chains: BTreeMap<u64, Chain>,
}
impl Registry {
    const fn new() -> Self {
        Self {
            physical: BTreeMap::new(),
            devices: BTreeMap::new(),
            queues: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            chains: BTreeMap::new(),
        }
    }
}

fn failure(error: impl std::fmt::Display) {
    *ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("In-game UI: {error}"));
}

pub fn take_failure() -> Option<String> {
    ERROR.lock().unwrap_or_else(|e| e.into_inner()).take()
}

pub fn attach(controls: Arc<Controls>) -> Result<(), String> {
    CONTROLS
        .set(controls)
        .map_err(|_| "overlay already attached")?;
    let mut pinned = std::ptr::null_mut();
    // Vulkan callers cache our callbacks beyond IAT restoration. Keep their code resident until process exit.
    if unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleExW(
            windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                | windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_PIN,
            attach as *const () as *const u16,
            &mut pinned,
        )
    } == 0
    {
        return Err("cannot retain overlay callbacks".into());
    }
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    let module = unsafe { GetModuleHandleW(windows_sys::w!("vulkan-1.dll")) };
    if module.is_null() {
        return Err("NMS Vulkan loader is unavailable".into());
    }
    DISPATCH[0]
        .set(unsafe { Dispatch::load(module, "vulkan-1.dll")? })
        .map_err(|_| "Vulkan already attached")?;
    let streamline = unsafe { GetModuleHandleW(windows_sys::w!("sl.interposer.dll")) };
    if !streamline.is_null() {
        DISPATCH[1]
            .set(unsafe { Dispatch::load(streamline, "sl.interposer.dll")? })
            .map_err(|_| "Streamline already attached")?;
    }
    let imports = iat::install(
        base,
        &[
            (b"GetProcAddress", get_proc_address as *const () as usize),
            (
                b"vkGetDeviceProcAddr",
                get_device_proc_addr::<0> as *const () as usize,
            ),
            (
                b"vkQueuePresentKHR",
                queue_present::<0> as *const () as usize,
            ),
            (
                b"GetAsyncKeyState",
                input::game_async_key as *const () as usize,
            ),
            (b"GetKeyState", input::game_key as *const () as usize),
            (
                b"GetKeyboardState",
                input::game_keyboard as *const () as usize,
            ),
            (
                b"SetCursorPos",
                input::game_cursor_pos as *const () as usize,
            ),
            (b"ClipCursor", input::game_clip as *const () as usize),
            (b"SetCursor", input::game_cursor as *const () as usize),
        ],
    )?;
    IMPORTS
        .set(Mutex::new(imports))
        .map_err(|_| "overlay imports already installed")?;
    RUNNING.store(true, Ordering::Release);
    Ok(())
}

unsafe fn replace<const KIND: usize>(
    name: *const c_char,
    original: vk::PFN_vkVoidFunction,
) -> vk::PFN_vkVoidFunction {
    let original = original?;
    if !RUNNING.load(Ordering::Acquire) {
        return Some(original);
    }
    let replacement = match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkGetInstanceProcAddr" => get_instance_proc_addr::<KIND> as *const (),
        b"vkGetDeviceProcAddr" => get_device_proc_addr::<KIND> as *const (),
        b"vkCreateInstance" => create_instance::<KIND> as *const (),
        b"vkCreateDevice" => create_device::<KIND> as *const (),
        b"vkCreateWin32SurfaceKHR" => create_surface::<KIND> as *const (),
        b"vkCreateSwapchainKHR" => create_swapchain::<KIND> as *const (),
        b"vkDestroySwapchainKHR" => destroy_swapchain::<KIND> as *const (),
        b"vkDestroyDevice" => destroy_device::<KIND> as *const (),
        b"vkDestroyInstance" => destroy_instance::<KIND> as *const (),
        b"vkDestroySurfaceKHR" => destroy_surface::<KIND> as *const (),
        b"vkQueuePresentKHR" => queue_present::<KIND> as *const (),
        _ => return Some(original),
    };
    Some(unsafe { std::mem::transmute::<*const (), unsafe extern "system" fn()>(replacement) })
}

unsafe extern "system" fn get_proc_address(
    module: windows_sys::Win32::Foundation::HMODULE,
    name: *const u8,
) -> windows_sys::Win32::Foundation::FARPROC {
    let original = unsafe { GetProcAddress(module, name) };
    if name as usize <= 65535 {
        return original;
    }
    let original = unsafe {
        std::mem::transmute::<windows_sys::Win32::Foundation::FARPROC, vk::PFN_vkVoidFunction>(
            original,
        )
    };
    unsafe {
        std::mem::transmute::<vk::PFN_vkVoidFunction, windows_sys::Win32::Foundation::FARPROC>(
            if DISPATCH[0]
                .get()
                .is_some_and(|d| d.module == module as usize)
            {
                replace::<0>(name.cast(), original)
            } else if DISPATCH[1]
                .get()
                .is_some_and(|d| d.module == module as usize)
            {
                replace::<1>(name.cast(), original)
            } else {
                original
            },
        )
    }
}
unsafe extern "system" fn get_instance_proc_addr<const KIND: usize>(
    instance: vk::Instance,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    unsafe { replace::<KIND>(name, (dispatch::<KIND>().gipa)(instance, name)) }
}
unsafe extern "system" fn get_device_proc_addr<const KIND: usize>(
    device: vk::Device,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    unsafe { replace::<KIND>(name, (dispatch::<KIND>().gdpa)(device, name)) }
}

unsafe extern "system" fn create_instance<const KIND: usize>(
    info: *const vk::InstanceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Instance,
) -> vk::Result {
    let result = unsafe { (dispatch::<KIND>().create_instance)(info, allocator, output) };
    if result == vk::Result::SUCCESS && RUNNING.load(Ordering::Acquire) {
        let captured = std::panic::catch_unwind(|| -> Result<(), String> {
            let instance = unsafe {
                ash::Instance::load(
                    &ash::StaticFn {
                        get_instance_proc_addr: dispatch::<KIND>().gipa,
                    },
                    *output,
                )
            };
            let physical = unsafe { instance.enumerate_physical_devices() }
                .map_err(|e| format!("enumerating Vulkan devices: {e:?}"))?;
            let mut registry = REGISTRY.lock().map_err(|_| "Vulkan registry poisoned")?;
            for device in physical {
                registry.physical.insert(device.as_raw(), instance.clone());
            }
            Ok(())
        });
        match captured {
            Ok(Ok(())) => {}
            Ok(Err(e)) => failure(e),
            Err(_) => failure("instance capture panicked"),
        }
    }
    result
}

unsafe extern "system" fn create_device<const KIND: usize>(
    physical: vk::PhysicalDevice,
    info: *const vk::DeviceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Device,
) -> vk::Result {
    if !RUNNING.load(Ordering::Acquire) {
        return unsafe { (dispatch::<KIND>().create_device)(physical, info, allocator, output) };
    }
    let instance = REGISTRY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .physical
        .get(&physical.as_raw())
        .cloned();
    let Some(instance) = instance else {
        failure("Vulkan device was created before instance capture");
        return unsafe { (dispatch::<KIND>().create_device)(physical, info, allocator, output) };
    };
    let result = unsafe { (instance.fp_v1_0().create_device)(physical, info, allocator, output) };
    if result == vk::Result::SUCCESS && RUNNING.load(Ordering::Acquire) {
        let captured = std::panic::catch_unwind(|| {
            let device = unsafe { ash::Device::load(instance.fp_v1_0(), *output) };
            let swapchain = ash::khr::swapchain::Device::new(&instance, &device);
            let families =
                unsafe { instance.get_physical_device_queue_family_properties(physical) };
            let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
            for queue in unsafe {
                std::slice::from_raw_parts(
                    (*info).p_queue_create_infos,
                    (*info).queue_create_info_count as usize,
                )
            } {
                if !queue.flags.is_empty() {
                    continue;
                }
                for index in 0..queue.queue_count {
                    let handle =
                        unsafe { device.get_device_queue(queue.queue_family_index, index) };
                    registry.queues.insert(
                        handle.as_raw(),
                        (
                            device.handle().as_raw(),
                            queue.queue_family_index,
                            families[queue.queue_family_index as usize].queue_flags,
                        ),
                    );
                }
            }
            registry.devices.insert(
                device.handle().as_raw(),
                Device {
                    physical,
                    instance,
                    device,
                    swapchain,
                },
            );
        });
        if captured.is_err() {
            failure("device capture panicked");
        }
    }
    result
}

unsafe extern "system" fn create_surface<const KIND: usize>(
    instance: vk::Instance,
    info: *const vk::Win32SurfaceCreateInfoKHR<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::SurfaceKHR,
) -> vk::Result {
    let static_fn = ash::StaticFn {
        get_instance_proc_addr: dispatch::<KIND>().gipa,
    };
    let instance = unsafe { ash::Instance::load(&static_fn, instance) };
    let entry = unsafe { ash::Entry::from_static_fn(static_fn) };
    let surface = ash::khr::win32_surface::Instance::new(&entry, &instance);
    let result = unsafe {
        (surface.fp().create_win32_surface_khr)(instance.handle(), info, allocator, output)
    };
    if result == vk::Result::SUCCESS && RUNNING.load(Ordering::Acquire) {
        REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .surfaces
            .insert(unsafe { (*output).as_raw() }, unsafe { (*info).hwnd }
                as usize);
    }
    result
}

unsafe extern "system" fn create_swapchain<const KIND: usize>(
    device: vk::Device,
    info: *const vk::SwapchainCreateInfoKHR<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::SwapchainKHR,
) -> vk::Result {
    if !RUNNING.load(Ordering::Acquire) {
        let create: vk::PFN_vkCreateSwapchainKHR = unsafe {
            std::mem::transmute(
                (dispatch::<KIND>().gdpa)(device, c"vkCreateSwapchainKHR".as_ptr()).unwrap(),
            )
        };
        return unsafe { create(device, info, allocator, output) };
    }
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    let Some(context) = registry.devices.get(&device.as_raw()) else {
        failure("missing Vulkan device");
        let create: vk::PFN_vkCreateSwapchainKHR = unsafe {
            std::mem::transmute(
                (dispatch::<KIND>().gdpa)(device, c"vkCreateSwapchainKHR".as_ptr()).unwrap(),
            )
        };
        return unsafe { create(device, info, allocator, output) };
    };
    let mut requested = unsafe { *info };
    if hdr::encoding(requested.image_color_space).is_some() {
        let entry = unsafe {
            ash::Entry::from_static_fn(ash::StaticFn {
                get_instance_proc_addr: dispatch::<KIND>().gipa,
            })
        };
        let surface = ash::khr::surface::Instance::new(&entry, &context.instance);
        match unsafe {
            surface.get_physical_device_surface_capabilities(context.physical, requested.surface)
        } {
            Ok(capabilities)
                if capabilities
                    .supported_usage_flags
                    .contains(vk::ImageUsageFlags::TRANSFER_SRC) =>
            {
                requested.image_usage |= vk::ImageUsageFlags::TRANSFER_SRC;
            }
            _ => {}
        }
    }
    let result = unsafe {
        (context.swapchain.fp().create_swapchain_khr)(device, &requested, allocator, output)
    };
    if result == vk::Result::SUCCESS && RUNNING.load(Ordering::Acquire) {
        let info = &requested;
        let hwnd = registry
            .surfaces
            .get(&info.surface.as_raw())
            .copied()
            .unwrap_or(0);
        if hwnd == 0
            || !info
                .image_usage
                .contains(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            || info.image_array_layers != 1
        {
            failure("unsupported game swapchain");
        } else {
            registry.chains.insert(
                unsafe { (*output).as_raw() },
                Chain {
                    device: device.as_raw(),
                    extent: info.image_extent,
                    format: info.image_format,
                    colour_space: info.image_color_space,
                    usage: info.image_usage,
                    hwnd,
                    painter: None,
                    failed: false,
                },
            );
            if let Err(error) = input::install(hwnd) {
                failure(error);
            }
        }
    }
    result
}

unsafe extern "system" fn queue_present<const KIND: usize>(
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    if !RUNNING.load(Ordering::Acquire) {
        return unsafe { (dispatch::<KIND>().present)(queue, info) };
    }
    PRESENTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let result = std::panic::catch_unwind(|| -> Result<Option<vk::Semaphore>, String> {
        let present = unsafe { &*info };
        if present.swapchain_count != 1 {
            return Ok(None);
        }
        let mut registry = REGISTRY.lock().map_err(|_| "Vulkan registry poisoned")?;
        let handle = unsafe { *present.p_swapchains };
        let index = unsafe { *present.p_image_indices };
        let Some(mut chain) = registry.chains.remove(&handle.as_raw()) else {
            return Ok(None);
        };
        let painted = (|| {
            if chain.failed {
                return Ok(None);
            }
            let &(device, family, flags) = registry
                .queues
                .get(&queue.as_raw())
                .ok_or("missing presentation queue")?;
            if device != chain.device || !flags.contains(vk::QueueFlags::GRAPHICS) {
                return Err("presentation queue must support graphics".into());
            }
            let context = registry
                .devices
                .get(&device)
                .ok_or("missing presentation device")?;
            let controls = CONTROLS.get().ok_or("missing overlay controls")?.clone();
            if chain.painter.is_none() {
                chain.painter = Some(paint::Painter::new(
                    context, handle, &chain, family, controls,
                )?);
            }
            chain
                .painter
                .as_mut()
                .unwrap()
                .present(queue, index, present, chain.hwnd)
        })();
        if painted.is_err() {
            chain.failed = true;
        }
        registry.chains.insert(handle.as_raw(), chain);
        painted
    });
    let mut modified = unsafe { *info };
    let semaphore = match result {
        Ok(Ok(Some(semaphore))) => {
            DRAWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(semaphore)
        }
        Ok(Ok(None)) => None,
        Ok(Err(error)) => {
            failure(error);
            input::release();
            None
        }
        Err(_) => {
            failure("render callback panicked");
            input::release();
            None
        }
    };
    if let Some(ref semaphore) = semaphore {
        modified.wait_semaphore_count = 1;
        modified.p_wait_semaphores = semaphore;
    }
    unsafe { (dispatch::<KIND>().present)(queue, &modified) }
}

unsafe extern "system" fn destroy_swapchain<const KIND: usize>(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(context) = registry.devices.get(&device.as_raw()) {
        let _ = unsafe { context.device.device_wait_idle() };
    }
    registry.chains.remove(&chain.as_raw());
    let destroy: vk::PFN_vkDestroySwapchainKHR = unsafe {
        std::mem::transmute(
            (dispatch::<KIND>().gdpa)(device, c"vkDestroySwapchainKHR".as_ptr()).unwrap(),
        )
    };
    unsafe { destroy(device, chain, allocator) };
}
unsafe extern "system" fn destroy_device<const KIND: usize>(
    device: vk::Device,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(context) = registry.devices.remove(&device.as_raw()) {
        let _ = unsafe { context.device.device_wait_idle() };
        registry
            .chains
            .retain(|_, chain| chain.device != device.as_raw());
        registry
            .queues
            .retain(|_, (handle, _, _)| *handle != device.as_raw());
        input::release();
    }
    let context = unsafe { borrowed_device::<KIND>(device) };
    unsafe { (context.fp_v1_0().destroy_device)(device, allocator) };
}
unsafe extern "system" fn destroy_instance<const KIND: usize>(
    instance: vk::Instance,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    registry
        .physical
        .retain(|_, context| context.handle() != instance);
    let context = unsafe {
        ash::Instance::load(
            &ash::StaticFn {
                get_instance_proc_addr: dispatch::<KIND>().gipa,
            },
            instance,
        )
    };
    unsafe { (context.fp_v1_0().destroy_instance)(instance, allocator) };
}

unsafe fn borrowed_device<const KIND: usize>(device: vk::Device) -> ash::Device {
    unsafe {
        ash::Device::load_with(
            |name| {
                (dispatch::<KIND>().gdpa)(device, name.as_ptr())
                    .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
            },
            device,
        )
    }
}
unsafe extern "system" fn destroy_surface<const KIND: usize>(
    instance: vk::Instance,
    surface: vk::SurfaceKHR,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    REGISTRY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .surfaces
        .remove(&surface.as_raw());
    let destroy: vk::PFN_vkDestroySurfaceKHR = unsafe {
        std::mem::transmute(
            (dispatch::<KIND>().gipa)(instance, c"vkDestroySurfaceKHR".as_ptr()).unwrap(),
        )
    };
    unsafe { destroy(instance, surface, allocator) };
}

#[cfg(feature = "investigation")]
pub fn stop() -> Result<(), String> {
    RUNNING.store(false, Ordering::Release);
    {
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        registry.chains.clear();
        registry.queues.clear();
        registry.devices.clear();
        registry.physical.clear();
        registry.surfaces.clear();
    }
    input::release();
    let result = input::uninstall();
    if let Some(imports) = IMPORTS.get() {
        imports.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    result
}

pub fn diagnostics() -> Option<serde_json::Value> {
    let registry = REGISTRY.try_lock().ok()?;
    Some(serde_json::json!({
        "dispatchers": DISPATCH.iter().filter_map(|d| d.get().map(|d| d.name)).collect::<Vec<_>>(),
        "presented": PRESENTED.load(std::sync::atomic::Ordering::Relaxed),
        "drawn": DRAWN.load(std::sync::atomic::Ordering::Relaxed),
        "physical_devices": registry.physical.len(),
        "devices": registry.devices.len(),
        "input": input::diagnostics(),
        "swapchains": registry.chains.iter().map(|(handle, chain)| serde_json::json!({"handle":handle,"size":[chain.extent.width,chain.extent.height],"format":format!("{:?}",chain.format),"colour_space":format!("{:?}",chain.colour_space),"window":chain.hwnd,"renderer":chain.painter.is_some(),"failed":chain.failed})).collect::<Vec<_>>()
    }))
}
