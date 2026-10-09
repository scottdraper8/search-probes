use super::{Chain, Device, hdr, input};
use ash::vk;
use egui_ash_renderer::{Options, RenderMode, Renderer, allocator::DefaultAllocator};
use search_probes_model::controls::Controls;
use search_probes_ui::{App, egui};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

type Result<T> = std::result::Result<T, String>;
fn vk_error(error: impl std::fmt::Debug) -> String {
    format!("Vulkan rendering failed: {error:?}")
}
static UI: OnceLock<Mutex<Interface>> = OnceLock::new();

struct Interface {
    context: egui::Context,
    app: App,
    started: Instant,
    textures: HashMap<egui::TextureId, (egui::ColorImage, egui::TextureOptions)>,
}
impl Interface {
    fn new(controls: Arc<Controls>) -> Self {
        let context = egui::Context::default();
        let app = App::new(&context, controls);
        Self {
            context,
            app,
            started: Instant::now(),
            textures: HashMap::new(),
        }
    }
    fn remember(&mut self, id: egui::TextureId, delta: &egui::epaint::ImageDelta) {
        let egui::ImageData::Color(image) = &delta.image;
        if let Some([x, y]) = delta.pos {
            if let Some((stored, options)) = self.textures.get_mut(&id) {
                for row in 0..image.height() {
                    let start = (y + row) * stored.width() + x;
                    stored.pixels[start..start + image.width()].copy_from_slice(
                        &image.pixels[row * image.width()..(row + 1) * image.width()],
                    );
                }
                *options = delta.options;
            }
        } else {
            self.textures.insert(id, ((**image).clone(), delta.options));
        }
    }
}

pub struct Painter {
    device: ash::Device,
    render_pass: vk::RenderPass,
    views: Vec<vk::ImageView>,
    images: Vec<vk::Image>,
    hdr: Option<hdr::Hdr>,
    framebuffers: Vec<vk::Framebuffer>,
    semaphores: Vec<vk::Semaphore>,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    pending: bool,
    renderer: Option<Renderer<DefaultAllocator>>,
    free: Vec<egui::TextureId>,
    extent: vk::Extent2D,
    textures_loaded: bool,
}
impl Painter {
    pub fn new(
        context: &Device,
        swapchain: vk::SwapchainKHR,
        chain: &Chain,
        family: u32,
        controls: Arc<Controls>,
    ) -> Result<Self> {
        UI.get_or_init(|| Mutex::new(Interface::new(controls)));
        let mut painter = Self {
            device: context.device.clone(),
            render_pass: vk::RenderPass::null(),
            views: Vec::new(),
            images: Vec::new(),
            hdr: None,
            framebuffers: Vec::new(),
            semaphores: Vec::new(),
            pool: vk::CommandPool::null(),
            command: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            pending: false,
            renderer: None,
            free: Vec::new(),
            extent: chain.extent,
            textures_loaded: false,
        };
        let is_hdr = hdr::encoding(chain.colour_space).is_some();
        if !is_hdr && chain.colour_space != vk::ColorSpaceKHR::SRGB_NONLINEAR {
            return Err(format!(
                "unsupported swapchain colour space {:?}",
                chain.colour_space
            ));
        }
        if is_hdr && !chain.usage.contains(vk::ImageUsageFlags::TRANSFER_SRC) {
            return Err("HDR surface does not support background copying".into());
        }
        let images =
            unsafe { context.swapchain.get_swapchain_images(swapchain) }.map_err(vk_error)?;
        if images.is_empty() || images.len() > 16 {
            return Err("invalid swapchain image count".into());
        }
        painter.images = images.clone();
        unsafe {
            for &image in &images {
                painter.views.push(
                    context
                        .device
                        .create_image_view(
                            &vk::ImageViewCreateInfo::default()
                                .image(image)
                                .view_type(vk::ImageViewType::TYPE_2D)
                                .format(chain.format)
                                .subresource_range(
                                    vk::ImageSubresourceRange::default()
                                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                                        .level_count(1)
                                        .layer_count(1),
                                ),
                            None,
                        )
                        .map_err(vk_error)?,
                );
            }
        }
        if is_hdr {
            painter.hdr = Some(hdr::Hdr::new(
                &context.instance,
                context.physical,
                &context.device,
                chain.extent,
                chain.format,
                chain.colour_space,
                &painter.views,
                vk::ImageLayout::PRESENT_SRC_KHR,
            )?);
        }
        let attachment = [vk::AttachmentDescription::default()
            .format(if is_hdr { hdr::UI_FORMAT } else { chain.format })
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(if is_hdr {
                vk::AttachmentLoadOp::CLEAR
            } else {
                vk::AttachmentLoadOp::LOAD
            })
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(if is_hdr {
                vk::ImageLayout::UNDEFINED
            } else {
                vk::ImageLayout::PRESENT_SRC_KHR
            })
            .final_layout(if is_hdr {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            } else {
                vk::ImageLayout::PRESENT_SRC_KHR
            })];
        let reference = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&reference)];
        let dependencies = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::ALL_COMMANDS)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                ),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::BOTTOM_OF_PIPE)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        ];
        unsafe {
            painter.render_pass = context
                .device
                .create_render_pass(
                    &vk::RenderPassCreateInfo::default()
                        .attachments(&attachment)
                        .subpasses(&subpass)
                        .dependencies(&dependencies),
                    None,
                )
                .map_err(vk_error)?;
            painter.pool = context
                .device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(vk_error)?;
            painter.command = context
                .device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(painter.pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(vk_error)?[0];
            painter.fence = context
                .device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(vk_error)?;
            for &view in &painter.views {
                let attachments = [painter.hdr.as_ref().map_or(view, |hdr| hdr.ui_view())];
                let framebuffer = context
                    .device
                    .create_framebuffer(
                        &vk::FramebufferCreateInfo::default()
                            .render_pass(painter.render_pass)
                            .attachments(&attachments)
                            .width(chain.extent.width)
                            .height(chain.extent.height)
                            .layers(1),
                        None,
                    )
                    .map_err(vk_error)?;
                painter.framebuffers.push(framebuffer);
                painter.semaphores.push(
                    context
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                        .map_err(vk_error)?,
                );
            }
        }
        painter.renderer = Some(
            Renderer::with_default_allocator(
                &context.instance,
                context.physical,
                context.device.clone(),
                RenderMode::RenderPass(painter.render_pass),
                Options {
                    srgb_framebuffer: is_hdr
                        || matches!(
                            chain.format,
                            vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
                        ),
                    ..Default::default()
                },
            )
            .map_err(vk_error)?,
        );
        Ok(painter)
    }
    pub fn present(
        &mut self,
        queue: vk::Queue,
        index: u32,
        present: &vk::PresentInfoKHR<'_>,
        hwnd: usize,
    ) -> Result<Option<vk::Semaphore>> {
        let mut ui = UI.get().unwrap().lock().map_err(|_| "UI poisoned")?;
        let elapsed = ui.started.elapsed().as_secs_f64();
        let Some(raw) = input::take(hwnd, self.extent, elapsed) else {
            return Ok(None);
        };
        let index = index as usize;
        if index >= self.framebuffers.len() {
            return Err("invalid presented image index".into());
        }
        if self.pending {
            unsafe {
                self.device
                    .wait_for_fences(&[self.fence], true, 5_000_000_000)
            }
            .map_err(vk_error)?;
            self.pending = false;
        }
        let renderer = self.renderer.as_mut().unwrap();
        for id in self.free.drain(..) {
            renderer.free_texture(id).map_err(vk_error)?;
            ui.textures.remove(&id);
        }
        if !self.textures_loaded {
            for (&id, (image, options)) in &ui.textures {
                renderer
                    .set_texture(
                        queue,
                        self.pool,
                        id,
                        &egui::epaint::ImageDelta::full(image.clone(), *options),
                    )
                    .map_err(vk_error)?;
            }
            self.textures_loaded = true;
        }
        let context = ui.context.clone();
        ui.app.refresh();
        let mut output = context.run_ui(raw, |root| {
            egui::Window::new("Search Probes")
                .id(egui::Id::new("search_probes_overlay"))
                .default_size([1100.0, 840.0])
                .min_size([380.0, 240.0])
                .resizable(true)
                .collapsible(false)
                .show(root.ctx(), |ui_root| ui.app.ui(ui_root));
        });
        input::output(hwnd, &output.platform_output);
        let mut texture_error = None;
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in deltas {
                ui.remember(id, &delta);
                if let Err(error) = renderer.set_texture(queue, self.pool, id, &delta) {
                    texture_error = Some(vk_error(error));
                }
            }
        }
        self.free.extend(output.textures_delta.free.drain());
        output.textures_delta.clear();
        if let Some(error) = texture_error {
            return Err(error);
        }
        let primitives = context.tessellate(output.shapes, output.pixels_per_point);
        unsafe {
            self.device
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())
                .map_err(vk_error)?;
            self.device
                .begin_command_buffer(
                    self.command,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(vk_error)?;
            let clears = [vk::ClearValue {
                color: vk::ClearColorValue { float32: [0.; 4] },
            }];
            self.device.cmd_begin_render_pass(
                self.command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(self.framebuffers[index])
                    .clear_values(&clears)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent: self.extent,
                    }),
                vk::SubpassContents::INLINE,
            );
            renderer
                .cmd_draw(
                    self.command,
                    self.extent,
                    output.pixels_per_point,
                    &primitives,
                )
                .map_err(vk_error)?;
            self.device.cmd_end_render_pass(self.command);
            if let Some(hdr) = &self.hdr {
                hdr.composite(self.command, self.images[index], index);
            }
            self.device
                .end_command_buffer(self.command)
                .map_err(vk_error)?;
            self.device.reset_fences(&[self.fence]).map_err(vk_error)?;
            let waits = if present.wait_semaphore_count == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(
                    present.p_wait_semaphores,
                    present.wait_semaphore_count as usize,
                )
            };
            let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
            let commands = [self.command];
            // Presentation completion is tracked per acquired swapchain image, independently of the draw fence.
            let signals = [self.semaphores[index]];
            self.device
                .queue_submit(
                    queue,
                    &[vk::SubmitInfo::default()
                        .wait_semaphores(waits)
                        .wait_dst_stage_mask(&stages)
                        .command_buffers(&commands)
                        .signal_semaphores(&signals)],
                    self.fence,
                )
                .map_err(vk_error)?;
        }
        self.pending = true;
        Ok(Some(self.semaphores[index]))
    }
}
impl Drop for Painter {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.renderer.take();
            for framebuffer in self.framebuffers.drain(..) {
                self.device.destroy_framebuffer(framebuffer, None);
            }
            self.hdr.take();
            for view in self.views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            for semaphore in self.semaphores.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
            if self.fence != vk::Fence::null() {
                self.device.destroy_fence(self.fence, None);
            }
            if self.pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.pool, None);
            }
            if self.render_pass != vk::RenderPass::null() {
                self.device.destroy_render_pass(self.render_pass, None);
            }
        }
    }
}
