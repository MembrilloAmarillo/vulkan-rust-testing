//! egui Vulkan renderer using device addresses.
//!
//! Converts egui tessellated output into Vulkan draw calls.
//! Uses device address buffers for vertex/index data, and a traditional
//! descriptor set for the font texture (combined-image-sampler at set=0, binding=0).

use egui::ClippedPrimitive;

use crate::simple::{
    Buffer, BufferUsage, CommandBuffer, DescriptorPool, DescriptorSet, DescriptorSetLayout, Format,
    GraphicsContext, GraphicsPipeline, MemoryType, PipelineLayout, ShaderModule, Texture,
    TextureUsage,
};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct UIVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: u32, // pre-multiplied sRGB packed as ABGR (little-endian RGBA)
}

/// Push constants: vertex buffer device address + screen size (16 bytes total).
#[repr(C)]
#[derive(Clone, Copy)]
struct UIPushConstants {
    vertex_ptr: u64,    // 8 bytes
    window_width: f32,  // 4 bytes
    window_height: f32, // 4 bytes
}

pub struct EguiRenderer {
    pipeline: GraphicsPipeline,
    layout: PipelineLayout,
    device: crate::VkDevice,
    // Font texture + descriptor
    font_texture: Option<Texture>,
    font_sampler: crate::VkSampler,
    descriptor_pool: DescriptorPool,
    descriptor_set_layout: DescriptorSetLayout,
    descriptor_set: Option<DescriptorSet>,
    // Geometry buffers
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_count: usize,
    index_count: usize,
    vertex_capacity: usize,
    index_capacity: usize,
}

impl EguiRenderer {
    pub fn new(
        context: &GraphicsContext,
        render_pass: crate::VkRenderPass,
    ) -> Result<Self, String> {
        // Load shaders
        let vert_spv = load_spirv_u32("shaders/ui.vert.spv")?;
        let frag_spv = load_spirv_u32("shaders/ui.frag.spv")?;

        let vs = ShaderModule::new(context, &vert_spv).map_err(|e| e.to_string())?;
        let fs = ShaderModule::new(context, &frag_spv).map_err(|e| e.to_string())?;

        // Descriptor set layout: set=0, binding=0 = combined image sampler (font atlas)
        let descriptor_set_layout =
            DescriptorSetLayout::new_texture_array(context, 1).map_err(|e| e.to_string())?;

        // Descriptor pool: capacity for 1 set, 1 combined-image-sampler
        let descriptor_pool = DescriptorPool::new(context, 1, 1).map_err(|e| e.to_string())?;

        // Pipeline layout: descriptor set 0 + push constants.
        // We borrow descriptor_set_layout by reference; it remains valid after this call.
        let set_layouts = [descriptor_set_layout];
        let layout = PipelineLayout::with_descriptor_set_layouts_and_push_size(
            context,
            &set_layouts,
            crate::simple::SHADER_STAGE_VERTEX | crate::simple::SHADER_STAGE_FRAGMENT,
            std::mem::size_of::<UIPushConstants>() as u32,
        )
        .map_err(|e| e.to_string())?;
        // Destructure the array to reclaim ownership
        let [descriptor_set_layout] = set_layouts;

        // Create alpha-blend pipeline (also sets VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT
        // to be compatible with the descriptor-buffer scene pipeline in the same command buffer).
        let pipeline = GraphicsPipeline::new_with_blend_descriptor_buffer(
            context,
            &vs,
            &fs,
            &layout,
            render_pass,
            Format::Rgba8Unorm,
            None,
            None,
        )
        .map_err(|e| e.to_string())?;

        // Sampler for font atlas
        let font_sampler = context
            .create_default_sampler()
            .map_err(|e| e.to_string())?;

        Ok(EguiRenderer {
            pipeline,
            layout,
            device: context.vk_device(),
            font_texture: None,
            font_sampler,
            descriptor_pool,
            descriptor_set_layout,
            descriptor_set: None,
            vertex_buffer: None,
            index_buffer: None,
            vertex_count: 0,
            index_count: 0,
            vertex_capacity: 0,
            index_capacity: 0,
        })
    }

    /// Upload (or re-upload) the egui texture atlas.
    /// Should be called each frame before `render()`, passing the `TexturesDelta`
    /// returned by `egui::Context::end_frame()`.
    pub fn update_textures(
        &mut self,
        context: &GraphicsContext,
        textures_delta: &egui::TexturesDelta,
    ) -> Result<(), String> {
        for (id, delta) in &textures_delta.set {
            // We only handle the built-in font atlas (TextureId::default() == Managed(0))
            if *id != egui::TextureId::default() {
                continue;
            }
            // Partial updates (sub-rect) not yet supported
            if delta.pos.is_some() {
                continue;
            }

            let (width, height, rgba_bytes) = image_delta_to_rgba(&delta.image);

            let cmd = CommandBuffer::allocate(context).map_err(|e| e.to_string())?;
            let texture = context
                .upload_texture(
                    &cmd,
                    &rgba_bytes,
                    width,
                    height,
                    Format::Rgba8Unorm,
                    TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
                )
                .map_err(|e| e.to_string())?;

            // Write descriptor (only allocate once — the pool holds capacity for 1 set)
            if self.descriptor_set.is_none() {
                let ds = self
                    .descriptor_pool
                    .allocate(&self.descriptor_set_layout)
                    .map_err(|e| e.to_string())?;
                ds.write_textures(context, &[&texture], self.font_sampler)
                    .map_err(|e| e.to_string())?;
                self.descriptor_set = Some(ds);
            } else if let Some(ref ds) = self.descriptor_set {
                // Re-write the existing descriptor set to point at the new texture
                ds.write_textures(context, &[&texture], self.font_sampler)
                    .map_err(|e| e.to_string())?;
            }

            self.font_texture = Some(texture);
        }
        Ok(())
    }

    /// Update vertex/index buffers with new egui tessellated output.
    pub fn prepare(
        &mut self,
        context: &GraphicsContext,
        clipped_primitives: Vec<ClippedPrimitive>,
    ) -> Result<(), String> {
        let mut vertices: Vec<UIVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for ClippedPrimitive { primitive, .. } in clipped_primitives {
            match primitive {
                egui::epaint::Primitive::Mesh(mesh) => {
                    let index_offset = vertices.len() as u32;

                    for vertex in &mesh.vertices {
                        let [r, g, b, a] = vertex.color.to_srgba_unmultiplied();
                        // Pre-multiply alpha for (srcFactor=ONE, dstFactor=ONE_MINUS_SRC_ALPHA)
                        let a_f = a as f32 / 255.0;
                        let pr = ((r as f32 / 255.0) * a_f * 255.0 + 0.5) as u8;
                        let pg = ((g as f32 / 255.0) * a_f * 255.0 + 0.5) as u8;
                        let pb = ((b as f32 / 255.0) * a_f * 255.0 + 0.5) as u8;
                        let packed = ((a as u32) << 24)
                            | ((pb as u32) << 16)
                            | ((pg as u32) << 8)
                            | (pr as u32);

                        vertices.push(UIVertex {
                            position: [vertex.pos.x, vertex.pos.y],
                            uv: [vertex.uv.x, vertex.uv.y],
                            color: packed,
                        });
                    }

                    for index in &mesh.indices {
                        indices.push(index_offset + index);
                    }
                }
                egui::epaint::Primitive::Callback(_) => {}
            }
        }

        self.vertex_count = vertices.len();
        self.index_count = indices.len();

        if !vertices.is_empty() {
            let needed = vertices.len() * std::mem::size_of::<UIVertex>();
            if self.vertex_capacity < needed || self.vertex_capacity > needed * 2 {
                self.vertex_capacity = (needed as f32 * 1.2) as usize;
                let buf = Buffer::new(
                    context,
                    self.vertex_capacity,
                    BufferUsage::VERTEX,
                    MemoryType::CpuMapped,
                )
                .map_err(|e| format!("vertex buffer: {e}"))?;
                buf.write(as_bytes(&vertices))
                    .map_err(|e| format!("write vertices: {e}"))?;
                self.vertex_buffer = Some(buf);
            } else if let Some(ref buf) = self.vertex_buffer {
                buf.write(as_bytes(&vertices))
                    .map_err(|e| format!("write vertices: {e}"))?;
            }
        }

        if !indices.is_empty() {
            let needed = indices.len() * std::mem::size_of::<u32>();
            if self.index_capacity < needed || self.index_capacity > needed * 2 {
                self.index_capacity = (needed as f32 * 1.2) as usize;
                let buf = Buffer::new(
                    context,
                    self.index_capacity,
                    BufferUsage::INDEX,
                    MemoryType::CpuMapped,
                )
                .map_err(|e| format!("index buffer: {e}"))?;
                buf.write(as_bytes(&indices))
                    .map_err(|e| format!("write indices: {e}"))?;
                self.index_buffer = Some(buf);
            } else if let Some(ref buf) = self.index_buffer {
                buf.write(as_bytes(&indices))
                    .map_err(|e| format!("write indices: {e}"))?;
            }
        }

        Ok(())
    }

    pub fn render(
        &self,
        cmd: &CommandBuffer,
        screen_width: f32,
        screen_height: f32,
    ) -> Result<(), String> {
        if self.vertex_buffer.is_none() || self.index_buffer.is_none() || self.index_count == 0 {
            return Ok(());
        }

        let Some(ref ds) = self.descriptor_set else {
            // Font texture not yet uploaded — nothing to draw
            return Ok(());
        };

        cmd.bind_pipeline(&self.pipeline);

        // Bind font texture (set=0, binding=0)
        unsafe {
            crate::vkCmdBindDescriptorSets(
                cmd.vk_buffer(),
                crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
                self.layout.vk_layout(),
                0,
                1,
                &ds.vk_set(),
                0,
                std::ptr::null(),
            );
        }

        let pc = UIPushConstants {
            vertex_ptr: self.vertex_buffer.as_ref().unwrap().device_address(),
            window_width: screen_width,
            window_height: screen_height,
        };
        cmd.push_constants(&self.layout, as_bytes(std::slice::from_ref(&pc)));

        cmd.bind_index_buffer(
            self.index_buffer.as_ref().unwrap(),
            0,
            crate::simple::IndexType::U32,
        );
        cmd.draw_indexed(self.index_count as u32, 1, 0, 0, 0);

        Ok(())
    }

    pub fn pipeline(&self) -> &GraphicsPipeline {
        &self.pipeline
    }
}

impl Drop for EguiRenderer {
    fn drop(&mut self) {
        unsafe {
            crate::vkDestroySampler(self.device, self.font_sampler, std::ptr::null());
        }
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            slice.len() * std::mem::size_of::<T>(),
        )
    }
}

/// Convert an egui `ImageData` to `(width, height, rgba8_bytes)`.
fn image_delta_to_rgba(image: &egui::ImageData) -> (u32, u32, Vec<u8>) {
    match image {
        egui::ImageData::Color(img) => {
            let w = img.size[0] as u32;
            let h = img.size[1] as u32;
            let bytes = img
                .pixels
                .iter()
                .flat_map(|c| {
                    let [r, g, b, a] = c.to_srgba_unmultiplied();
                    [r, g, b, a]
                })
                .collect();
            (w, h, bytes)
        }
        egui::ImageData::Font(img) => {
            let w = img.size[0] as u32;
            let h = img.size[1] as u32;
            // srgba_pixels bakes coverage into the alpha channel
            let bytes = img
                .srgba_pixels(None)
                .flat_map(|c| {
                    let [r, g, b, a] = c.to_srgba_unmultiplied();
                    [r, g, b, a]
                })
                .collect();
            (w, h, bytes)
        }
    }
}

fn load_spirv_u32(path: &str) -> Result<Vec<u32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    if bytes.len() % 4 != 0 {
        return Err(format!("SPIR-V file not u32-aligned: {path}"));
    }
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(words)
}
