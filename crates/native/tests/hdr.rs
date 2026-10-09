#![cfg(feature = "gpu-qualification")]

#[path = "../src/overlay/hdr.rs"]
mod hdr;
use ash::vk;
use std::sync::atomic::{AtomicUsize, Ordering};

static VALIDATION_ERRORS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn validation(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        VALIDATION_ERRORS.fetch_add(1, Ordering::Relaxed);
    }
    eprintln!(
        "Vulkan {severity:?}: {}",
        unsafe { std::ffi::CStr::from_ptr((*data).p_message) }.to_string_lossy()
    );
    vk::FALSE
}

fn range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}

#[test]
fn hdr_graphics_compositor_preserves_scene_and_blends_pq_and_scrgb() {
    let entry =
        unsafe { ash::Entry::load() }.expect("Vulkan loader required for GPU qualification");
    let layers = [c"VK_LAYER_KHRONOS_validation".as_ptr()];
    let extensions = [ash::ext::debug_utils::NAME.as_ptr()];
    let enabled = [vk::ValidationFeatureEnableEXT::SYNCHRONIZATION_VALIDATION];
    let mut features = vk::ValidationFeaturesEXT::default().enabled_validation_features(&enabled);
    let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(validation));
    let instance = unsafe {
        entry.create_instance(
            &vk::InstanceCreateInfo::default()
                .enabled_layer_names(&layers)
                .enabled_extension_names(&extensions)
                .push_next(&mut features)
                .push_next(&mut debug_info),
            None,
        )
    }
    .expect("GPU qualification requires VK_LAYER_KHRONOS_validation");
    let debug = ash::ext::debug_utils::Instance::new(&entry, &instance);
    let messenger = unsafe { debug.create_debug_utils_messenger(&debug_info, None) }.unwrap();
    let (physical, family) = unsafe { instance.enumerate_physical_devices() }
        .unwrap()
        .into_iter()
        .find_map(|p| {
            unsafe { instance.get_physical_device_queue_family_properties(p) }
                .iter()
                .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .map(|i| (p, i as u32))
        })
        .expect("Vulkan graphics queue");
    let priorities = [1.0];
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&priorities)];
    let device = unsafe {
        instance.create_device(
            physical,
            &vk::DeviceCreateInfo::default().queue_create_infos(&queues),
            None,
        )
    }
    .unwrap();
    let properties = unsafe { instance.get_physical_device_properties(physical) };
    println!(
        "HDR qualification device: {} ({:?})",
        unsafe { std::ffi::CStr::from_ptr(properties.device_name.as_ptr()) }.to_string_lossy(),
        properties.device_type
    );
    let extent = vk::Extent2D {
        width: 4,
        height: 4,
    };
    unsafe {
        let memory = instance.get_physical_device_memory_properties(physical);
        let allocate = |requirements: vk::MemoryRequirements, flags| {
            let index = (0..memory.memory_type_count)
                .find(|&i| {
                    requirements.memory_type_bits & (1 << i) != 0
                        && memory.memory_types[i as usize]
                            .property_flags
                            .contains(flags)
                })
                .unwrap();
            device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(index),
                    None,
                )
                .unwrap()
        };
        let pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
            .unwrap();
        let command = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .unwrap()[0];
        let queue = device.get_device_queue(family, 0);
        let fence = device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .unwrap();
        let buffer = device
            .create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(128)
                    .usage(vk::BufferUsageFlags::TRANSFER_DST)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
            .unwrap();
        let buffer_memory = allocate(
            device.get_buffer_memory_requirements(buffer),
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        device.bind_buffer_memory(buffer, buffer_memory, 0).unwrap();
        let pointer = device
            .map_memory(buffer_memory, 0, 128, vk::MemoryMapFlags::empty())
            .unwrap()
            .cast::<u8>();
        for (space, format) in [
            (
                vk::ColorSpaceKHR::HDR10_ST2084_EXT,
                vk::Format::A2B10G10R10_UNORM_PACK32,
            ),
            (
                vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT,
                vk::Format::R16G16B16A16_SFLOAT,
            ),
        ] {
            let image = device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D {
                            width: extent.width,
                            height: extent.height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(
                            vk::ImageUsageFlags::TRANSFER_SRC
                                | vk::ImageUsageFlags::TRANSFER_DST
                                | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                        )
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .unwrap();
            let image_memory = allocate(
                device.get_image_memory_requirements(image),
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            );
            device.bind_image_memory(image, image_memory, 0).unwrap();
            let view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(range()),
                    None,
                )
                .unwrap();
            let compositor = hdr::Hdr::new(
                &instance,
                physical,
                &device,
                extent,
                format,
                space,
                &[view],
                vk::ImageLayout::GENERAL,
            )
            .unwrap();
            let attachments = [vk::AttachmentDescription::default()
                .format(hdr::UI_FORMAT)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let refs = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&refs)];
            let pass = device
                .create_render_pass(
                    &vk::RenderPassCreateInfo::default()
                        .attachments(&attachments)
                        .subpasses(&subpasses),
                    None,
                )
                .unwrap();
            let views = [compositor.ui_view()];
            let framebuffer = device
                .create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(pass)
                        .attachments(&views)
                        .width(extent.width)
                        .height(extent.height)
                        .layers(1),
                    None,
                )
                .unwrap();
            for (background, ui, expected_pq, expected_scrgb) in [
                (
                    [0.42, 0.31, 0.09, 1.],
                    [0., 0., 0., 0.],
                    [0.42, 0.31, 0.09],
                    [0.42, 0.31, 0.09],
                ),
                (
                    [0.8, 0.6, 0.2, 1.],
                    [1., 1., 1., 1.],
                    [0.579133; 3],
                    [2.5; 3],
                ),
                (
                    [0., 0., 0., 1.],
                    [0.5, 0.5, 0.5, 0.5],
                    [0.508078; 3],
                    [1.25; 3],
                ),
                (
                    [0.508078, 0.508078, 0.508078, 1.],
                    [0.5, 0.5, 0.5, 0.5],
                    [0.549302; 3],
                    [1.504039; 3],
                ),
                (
                    [0.2, 0.3, 0.4, 1.],
                    [1., 0., 0., 1.],
                    [0.531029, 0.325794, 0.219090],
                    [2.5, 0., 0.],
                ),
                ([0.6, 0.7, 0.8, 1.], [0., 0., 0., 1.], [0.; 3], [0.; 3]),
                (
                    [-0.1, 4., 2., 1.],
                    [0., 0., 0., 0.],
                    [0., 1., 1.],
                    [-0.1, 4., 2.],
                ),
            ] {
                device
                    .begin_command_buffer(
                        command,
                        &vk::CommandBufferBeginInfo::default()
                            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                    )
                    .unwrap();
                let barrier = |old, new, src, dst| {
                    vk::ImageMemoryBarrier::default()
                        .image(image)
                        .old_layout(old)
                        .new_layout(new)
                        .src_access_mask(src)
                        .dst_access_mask(dst)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .subresource_range(range())
                };
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::empty(),
                        vk::AccessFlags::TRANSFER_WRITE,
                    )],
                );
                device.cmd_clear_color_image(
                    command,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue {
                        float32: background,
                    },
                    &[range()],
                );
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::GENERAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::MEMORY_READ,
                    )],
                );
                let clears = [vk::ClearValue {
                    color: vk::ClearColorValue { float32: ui },
                }];
                device.cmd_begin_render_pass(
                    command,
                    &vk::RenderPassBeginInfo::default()
                        .render_pass(pass)
                        .framebuffer(framebuffer)
                        .render_area(vk::Rect2D {
                            offset: vk::Offset2D::default(),
                            extent,
                        })
                        .clear_values(&clears),
                    vk::SubpassContents::INLINE,
                );
                device.cmd_end_render_pass(command);
                compositor.composite(command, image, 0);
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier(
                        vk::ImageLayout::GENERAL,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                        vk::AccessFlags::TRANSFER_READ,
                    )],
                );
                device.cmd_copy_image_to_buffer(
                    command,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &[vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .image_extent(vk::Extent3D {
                            width: extent.width,
                            height: extent.height,
                            depth: 1,
                        })],
                );
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::HOST,
                    vk::DependencyFlags::empty(),
                    &[vk::MemoryBarrier::default()
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::HOST_READ)],
                    &[],
                    &[],
                );
                device.end_command_buffer(command).unwrap();
                device
                    .queue_submit(
                        queue,
                        &[vk::SubmitInfo::default().command_buffers(&[command])],
                        fence,
                    )
                    .unwrap();
                device
                    .wait_for_fences(&[fence], true, 10_000_000_000)
                    .unwrap();
                for pixel in 0..16 {
                    let actual: [f32; 3] = if format == vk::Format::A2B10G10R10_UNORM_PACK32 {
                        let bits = std::ptr::read_unaligned(pointer.add(pixel * 4).cast::<u32>());
                        [0, 10, 20].map(|shift| ((bits >> shift) & 1023) as f32 / 1023.)
                    } else {
                        [0, 1, 2].map(|channel| {
                            half::f16::from_bits(std::ptr::read_unaligned(
                                pointer.add(pixel * 8 + channel * 2).cast::<u16>(),
                            ))
                            .to_f32()
                        })
                    };
                    let expected = if format == vk::Format::A2B10G10R10_UNORM_PACK32 {
                        expected_pq
                    } else {
                        expected_scrgb
                    };
                    for (actual, expected) in actual.into_iter().zip(expected) {
                        assert!(
                            (actual - expected).abs() < 0.0011,
                            "{space:?} pixel {pixel}: {actual} != {expected}; UI {ui:?}"
                        );
                    }
                }
                device.reset_fences(&[fence]).unwrap();
                device
                    .reset_command_pool(pool, vk::CommandPoolResetFlags::empty())
                    .unwrap();
            }
            device.destroy_framebuffer(framebuffer, None);
            device.destroy_render_pass(pass, None);
            drop(compositor);
            device.destroy_image_view(view, None);
            device.destroy_image(image, None);
            device.free_memory(image_memory, None);
        }
        device.destroy_fence(fence, None);
        device.destroy_command_pool(pool, None);
        device.unmap_memory(buffer_memory);
        device.destroy_buffer(buffer, None);
        device.free_memory(buffer_memory, None);
        device.destroy_device(None);
        debug.destroy_debug_utils_messenger(messenger, None);
        instance.destroy_instance(None);
    }
    assert_eq!(
        VALIDATION_ERRORS.load(Ordering::Relaxed),
        0,
        "Vulkan validation errors"
    );
}
