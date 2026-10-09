use ash::vk;

type Result<T> = std::result::Result<T, String>;
fn error(value: impl std::fmt::Debug) -> String {
    format!("HDR rendering: {value:?}")
}
pub const UI_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
pub fn encoding(space: vk::ColorSpaceKHR) -> Option<u32> {
    match space {
        vk::ColorSpaceKHR::HDR10_ST2084_EXT => Some(0),
        vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT => Some(1),
        _ => None,
    }
}
fn range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}
fn layers() -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .layer_count(1)
}
struct Image {
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}
impl Image {
    fn new(
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
        device: &ash::Device,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
    ) -> Result<Self> {
        let mut image = Self {
            device: device.clone(),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        };
        unsafe {
            image.image = device
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
                        .usage(usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .map_err(error)?;
            let requirements = device.get_image_memory_requirements(image.image);
            let memory = instance.get_physical_device_memory_properties(physical);
            let index = (0..memory.memory_type_count)
                .find(|&i| {
                    requirements.memory_type_bits & (1 << i) != 0
                        && memory.memory_types[i as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .ok_or("no HDR image memory type")?;
            image.memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(index),
                    None,
                )
                .map_err(error)?;
            device
                .bind_image_memory(image.image, image.memory, 0)
                .map_err(error)?;
            image.view = device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(range()),
                    None,
                )
                .map_err(error)?;
        }
        Ok(image)
    }
}
impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            if self.view != vk::ImageView::null() {
                self.device.destroy_image_view(self.view, None);
            }
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}
pub struct Hdr {
    device: ash::Device,
    ui: Image,
    background: Image,
    pass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    descriptor_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor: vk::DescriptorSet,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    modules: Vec<vk::ShaderModule>,
    extent: vk::Extent2D,
    encoding: u32,
    presentation_layout: vk::ImageLayout,
}
impl Hdr {
    pub fn new(
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
        device: &ash::Device,
        extent: vk::Extent2D,
        format: vk::Format,
        colour_space: vk::ColorSpaceKHR,
        views: &[vk::ImageView],
        presentation_layout: vk::ImageLayout,
    ) -> Result<Self> {
        let features =
            unsafe { instance.get_physical_device_format_properties(physical, UI_FORMAT) }
                .optimal_tiling_features;
        if !features.contains(
            vk::FormatFeatureFlags::COLOR_ATTACHMENT_BLEND | vk::FormatFeatureFlags::SAMPLED_IMAGE,
        ) {
            return Err("HDR UI floating-point blending is unavailable".into());
        }
        let ui = Image::new(
            instance,
            physical,
            device,
            extent,
            UI_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
        )?;
        let background = Image::new(
            instance,
            physical,
            device,
            extent,
            format,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        )?;
        let mut hdr = Self {
            device: device.clone(),
            ui,
            background,
            pass: vk::RenderPass::null(),
            framebuffers: Vec::new(),
            descriptor_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            descriptor: vk::DescriptorSet::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            modules: Vec::new(),
            extent,
            encoding: encoding(colour_space).ok_or("unsupported HDR encoding")?,
            presentation_layout,
        };
        unsafe {
            let attachments = [vk::AttachmentDescription::default()
                .format(format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .final_layout(presentation_layout)];
            let colour = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&colour)];
            let dependencies = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(
                    vk::PipelineStageFlags::TRANSFER
                        | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                )
                .dst_stage_mask(
                    vk::PipelineStageFlags::FRAGMENT_SHADER
                        | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                )
                .src_access_mask(
                    vk::AccessFlags::TRANSFER_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                )
                .dst_access_mask(
                    vk::AccessFlags::SHADER_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                )];
            hdr.pass = device
                .create_render_pass(
                    &vk::RenderPassCreateInfo::default()
                        .attachments(&attachments)
                        .subpasses(&subpasses)
                        .dependencies(&dependencies),
                    None,
                )
                .map_err(error)?;
            for &view in views {
                let attachments = [view];
                hdr.framebuffers.push(
                    device
                        .create_framebuffer(
                            &vk::FramebufferCreateInfo::default()
                                .render_pass(hdr.pass)
                                .attachments(&attachments)
                                .width(extent.width)
                                .height(extent.height)
                                .layers(1),
                            None,
                        )
                        .map_err(error)?,
                );
            }
            let bindings = [0, 1].map(|binding| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(binding)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            });
            hdr.descriptor_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .map_err(error)?;
            let sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(2)];
            hdr.descriptor_pool = device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(1)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(error)?;
            let layouts = [hdr.descriptor_layout];
            hdr.descriptor = device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(hdr.descriptor_pool)
                        .set_layouts(&layouts),
                )
                .map_err(error)?[0];
            let image_infos = [hdr.background.view, hdr.ui.view].map(|view| {
                [vk::DescriptorImageInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
            });
            let writes = [0usize, 1].map(|i| {
                vk::WriteDescriptorSet::default()
                    .dst_set(hdr.descriptor)
                    .dst_binding(i as u32)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&image_infos[i])
            });
            device.update_descriptor_sets(&writes, &[]);
            let pushes = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .size(4)];
            hdr.layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&layouts)
                        .push_constant_ranges(&pushes),
                    None,
                )
                .map_err(error)?;
            for bytes in [
                include_bytes!(concat!(env!("OUT_DIR"), "/hdr.vertex.spv")).as_slice(),
                include_bytes!(concat!(env!("OUT_DIR"), "/hdr.fragment.spv")).as_slice(),
            ] {
                let words: Vec<_> = bytes
                    .chunks_exact(4)
                    .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                    .collect();
                hdr.modules.push(
                    device
                        .create_shader_module(
                            &vk::ShaderModuleCreateInfo::default().code(&words),
                            None,
                        )
                        .map_err(error)?,
                );
            }
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(hdr.modules[0])
                    .name(c"vertex"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(hdr.modules[1])
                    .name(c"fragment"),
            ];
            let vertex = vk::PipelineVertexInputStateCreateInfo::default();
            let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
                .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
            let viewport = vk::PipelineViewportStateCreateInfo::default()
                .viewport_count(1)
                .scissor_count(1);
            let raster = vk::PipelineRasterizationStateCreateInfo::default()
                .polygon_mode(vk::PolygonMode::FILL)
                .cull_mode(vk::CullModeFlags::NONE)
                .line_width(1.0);
            let samples = vk::PipelineMultisampleStateCreateInfo::default()
                .rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let attachments = [vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA)];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
            let dynamics = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamics);
            hdr.pipeline = match device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[vk::GraphicsPipelineCreateInfo::default()
                    .stages(&stages)
                    .vertex_input_state(&vertex)
                    .input_assembly_state(&assembly)
                    .viewport_state(&viewport)
                    .rasterization_state(&raster)
                    .multisample_state(&samples)
                    .color_blend_state(&blend)
                    .dynamic_state(&dynamic)
                    .layout(hdr.layout)
                    .render_pass(hdr.pass)],
                None,
            ) {
                Ok(pipelines) => pipelines[0],
                Err((pipelines, e)) => {
                    for p in pipelines {
                        device.destroy_pipeline(p, None);
                    }
                    return Err(error(e));
                }
            };
            for module in hdr.modules.drain(..) {
                device.destroy_shader_module(module, None);
            }
        }
        Ok(hdr)
    }
    pub fn ui_view(&self) -> vk::ImageView {
        self.ui.view
    }
    pub unsafe fn composite(&self, command: vk::CommandBuffer, image: vk::Image, index: usize) {
        let barrier = |image, old, new, src, dst| {
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
        unsafe {
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[
                    barrier(
                        image,
                        self.presentation_layout,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::AccessFlags::MEMORY_WRITE,
                        vk::AccessFlags::TRANSFER_READ,
                    ),
                    barrier(
                        self.background.image,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::AccessFlags::empty(),
                        vk::AccessFlags::TRANSFER_WRITE,
                    ),
                ],
            );
            self.device.cmd_copy_image(
                command,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.background.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy::default()
                    .src_subresource(layers())
                    .dst_subresource(layers())
                    .extent(vk::Extent3D {
                        width: self.extent.width,
                        height: self.extent.height,
                        depth: 1,
                    })],
            );
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::FRAGMENT_SHADER
                    | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[
                    barrier(
                        self.background.image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::SHADER_READ,
                    ),
                    barrier(
                        image,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        vk::AccessFlags::TRANSFER_READ,
                        vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    ),
                    barrier(
                        self.ui.image,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                        vk::AccessFlags::SHADER_READ,
                    ),
                ],
            );
            self.device.cmd_begin_render_pass(
                command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.pass)
                    .framebuffer(self.framebuffers[index])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: self.extent,
                    }),
                vk::SubpassContents::INLINE,
            );
            self.device
                .cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            self.device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[self.descriptor],
                &[],
            );
            self.device.cmd_push_constants(
                command,
                self.layout,
                vk::ShaderStageFlags::FRAGMENT,
                0,
                &self.encoding.to_ne_bytes(),
            );
            self.device.cmd_set_viewport(
                command,
                0,
                &[vk::Viewport {
                    x: 0.,
                    y: 0.,
                    width: self.extent.width as f32,
                    height: self.extent.height as f32,
                    min_depth: 0.,
                    max_depth: 1.,
                }],
            );
            self.device.cmd_set_scissor(
                command,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: self.extent,
                }],
            );
            self.device.cmd_draw(command, 3, 1, 0, 0);
            self.device.cmd_end_render_pass(command);
        }
    }
}
impl Drop for Hdr {
    fn drop(&mut self) {
        unsafe {
            if self.pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(self.pipeline, None);
            }
            for module in self.modules.drain(..) {
                self.device.destroy_shader_module(module, None);
            }
            if self.layout != vk::PipelineLayout::null() {
                self.device.destroy_pipeline_layout(self.layout, None);
            }
            if self.descriptor_pool != vk::DescriptorPool::null() {
                self.device
                    .destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if self.descriptor_layout != vk::DescriptorSetLayout::null() {
                self.device
                    .destroy_descriptor_set_layout(self.descriptor_layout, None);
            }
            for framebuffer in self.framebuffers.drain(..) {
                self.device.destroy_framebuffer(framebuffer, None);
            }
            if self.pass != vk::RenderPass::null() {
                self.device.destroy_render_pass(self.pass, None);
            }
        }
    }
}
