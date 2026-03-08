//! egui Vulkan renderer using device addresses and descriptor-buffer bindless textures.
//!
//! Converts egui tessellated output into Vulkan draw calls.
//! Uses device address buffers for vertex/index data, and a `TextureDescriptorHeap`
//! for the font texture (compatible with VK_EXT_descriptor_buffer pipelines).

use egui::ClippedPrimitive;

use crate::simple::{
    Buffer, BufferUsage, CommandBuffer, DescriptorSetLayout, Format, GraphicsContext,
    GraphicsPipeline, MemoryType, PipelineLayout, ShaderModule, Texture, TextureDescriptorHeap,
    TextureUsage,
};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct UIVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: u32, // pre-multiplied sRGB packed as ABGR (little-endian RGBA)
}

/// Push constants for the egui pipeline (20 bytes, same layout in both shaders).
///
/// Layout (std430):
///   offset  0: vertex_ptr    (u64, 8 bytes)
///   offset  8: window_width  (f32, 4 bytes)
///   offset 12: window_height (f32, 4 bytes)
///   offset 16: texture_index (u32, 4 bytes)
///   total: 20 bytes
#[repr(C)]
#[derive(Clone, Copy)]
struct UIPushConstants {
    vertex_ptr: u64,    // 8 bytes
    window_width: f32,  // 4 bytes
    window_height: f32, // 4 bytes
    texture_index: u32, // 4 bytes — bindless heap index for font atlas
}

/// One scissored draw call produced by `prepare()` and consumed by `render()`.
#[derive(Clone, Copy)]
struct DrawCall {
    /// First index in the index buffer.
    first_index: u32,
    /// Number of indices to draw.
    index_count: u32,
    /// Scissor rect in screen-space pixels (already clamped to the viewport).
    scissor_x: i32,
    scissor_y: i32,
    scissor_w: u32,
    scissor_h: u32,
}

pub struct EguiRenderer {
    pipeline: GraphicsPipeline,
    layout: PipelineLayout,
    device: crate::VkDevice,
    // Font texture + bindless descriptor heap
    font_texture: Option<Texture>,
    font_sampler: crate::VkSampler,
    font_heap: TextureDescriptorHeap,
    font_texture_index: u32,
    font_heap_written: bool,
    // Geometry buffers
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_capacity: usize,
    index_capacity: usize,
    // Reused CPU-side scratch buffers to avoid per-frame Vec allocations.
    scratch_vertices: Vec<UIVertex>,
    scratch_indices: Vec<u32>,
    // Per-primitive draw calls (populated by prepare, consumed by render)
    draws: Vec<DrawCall>,
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

        // Descriptor set layout: bindless combined-image-sampler array (descriptor-buffer compatible).
        let set_layout =
            DescriptorSetLayout::new_bindless_textures(context, 1).map_err(|e| e.to_string())?;

        // Pipeline layout: descriptor set 0 + push constants (20 bytes).
        let layout = PipelineLayout::with_descriptor_set_layouts_and_push_size(
            context,
            &[set_layout],
            crate::simple::SHADER_STAGE_VERTEX | crate::simple::SHADER_STAGE_FRAGMENT,
            std::mem::size_of::<UIPushConstants>() as u32,
        )
        .map_err(|e| e.to_string())?;

        // Alpha-blend pipeline with VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT.
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

        // Descriptor heap: capacity 1 (only the font atlas).
        let mut font_heap = TextureDescriptorHeap::new(context, 1).map_err(|e| e.to_string())?;
        let font_texture_index = font_heap.allocate().map_err(|e| e.to_string())?;

        Ok(EguiRenderer {
            pipeline,
            layout,
            device: context.vk_device(),
            font_texture: None,
            font_sampler,
            font_heap,
            font_texture_index,
            font_heap_written: false,
            vertex_buffer: None,
            index_buffer: None,
            vertex_capacity: 0,
            index_capacity: 0,
            scratch_vertices: Vec::new(),
            scratch_indices: Vec::new(),
            draws: Vec::new(),
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
        for id in &textures_delta.free {
            // We only own the default egui atlas texture in this renderer.
            if *id == egui::TextureId::default() {
                self.font_texture = None;
                self.font_heap_written = false;
            }
        }

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

            let texture = context
                .upload_texture(
                    &rgba_bytes,
                    width,
                    height,
                    Format::Rgba8Unorm,
                    TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
                )
                .map_err(|e| e.to_string())?;

            // Write (or re-write) the descriptor in the heap.
            self.font_heap
                .write_descriptor(
                    context,
                    self.font_texture_index,
                    &texture,
                    self.font_sampler,
                )
                .map_err(|e| e.to_string())?;
            self.font_heap_written = true;

            self.font_texture = Some(texture);
        }
        Ok(())
    }

    /// Update vertex/index buffers with new egui tessellated output.
    pub fn prepare(
        &mut self,
        context: &GraphicsContext,
        clipped_primitives: Vec<ClippedPrimitive>,
        screen_width: f32,
        screen_height: f32,
    ) -> Result<(), String> {
        self.scratch_vertices.clear();
        self.scratch_indices.clear();
        self.draws.clear();

        for ClippedPrimitive {
            primitive,
            clip_rect,
        } in clipped_primitives
        {
            match primitive {
                egui::epaint::Primitive::Mesh(mesh) => {
                    if mesh.indices.is_empty() {
                        continue;
                    }

                    let index_offset = self.scratch_vertices.len() as u32;
                    let first_index = self.scratch_indices.len() as u32;

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

                        self.scratch_vertices.push(UIVertex {
                            position: [vertex.pos.x, vertex.pos.y],
                            uv: [vertex.uv.x, vertex.uv.y],
                            color: packed,
                        });
                    }

                    for index in &mesh.indices {
                        self.scratch_indices.push(index_offset + index);
                    }

                    // Clamp clip_rect to the viewport and convert to integer pixels.
                    let x0 = clip_rect.min.x.max(0.0).floor() as i32;
                    let y0 = clip_rect.min.y.max(0.0).floor() as i32;
                    let x1 = clip_rect.max.x.min(screen_width).ceil() as i32;
                    let y1 = clip_rect.max.y.min(screen_height).ceil() as i32;
                    let w = (x1 - x0).max(0) as u32;
                    let h = (y1 - y0).max(0) as u32;

                    self.draws.push(DrawCall {
                        first_index,
                        index_count: mesh.indices.len() as u32,
                        scissor_x: x0,
                        scissor_y: y0,
                        scissor_w: w,
                        scissor_h: h,
                    });
                }
                egui::epaint::Primitive::Callback(_) => {}
            }
        }

        if !self.scratch_vertices.is_empty() {
            let needed = self.scratch_vertices.len() * std::mem::size_of::<UIVertex>();
            // Only reallocate if needed grows beyond capacity OR shrinks to less than 1/4 of capacity
            // This prevents thrashing when UI size oscillates around the threshold
            if self.vertex_capacity < needed || self.vertex_capacity > needed * 4 {
                self.vertex_capacity = (needed as f32 * 1.5) as usize;
                let buf = Buffer::new(
                    context,
                    self.vertex_capacity,
                    BufferUsage::VERTEX,
                    MemoryType::CpuMapped,
                )
                .map_err(|e| format!("vertex buffer: {e}"))?;
                buf.write(as_bytes(&self.scratch_vertices))
                    .map_err(|e| format!("write vertices: {e}"))?;
                self.vertex_buffer = Some(buf);
            } else if let Some(ref buf) = self.vertex_buffer {
                buf.write(as_bytes(&self.scratch_vertices))
                    .map_err(|e| format!("write vertices: {e}"))?;
            }
        }

        if !self.scratch_indices.is_empty() {
            let needed = self.scratch_indices.len() * std::mem::size_of::<u32>();
            // Only reallocate if needed grows beyond capacity OR shrinks to less than 1/4 of capacity
            // This prevents thrashing when UI size oscillates around the threshold
            if self.index_capacity < needed || self.index_capacity > needed * 4 {                eprintln!("ALLOC: Reallocating index buffer {} -> {} bytes", self.index_capacity, needed);                self.index_capacity = (needed as f32 * 1.5) as usize;
                let buf = Buffer::new(
                    context,
                    self.index_capacity,
                    BufferUsage::INDEX,
                    MemoryType::CpuMapped,
                )
                .map_err(|e| format!("index buffer: {e}"))?;
                buf.write(as_bytes(&self.scratch_indices))
                    .map_err(|e| format!("write indices: {e}"))?;
                self.index_buffer = Some(buf);
            } else if let Some(ref buf) = self.index_buffer {
                buf.write(as_bytes(&self.scratch_indices))
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
        if self.vertex_buffer.is_none() || self.index_buffer.is_none() || self.draws.is_empty() {
            return Ok(());
        }

        if !self.font_heap_written {
            return Ok(());
        }

        cmd.bind_pipeline(&self.pipeline);

        // Bind font texture heap via descriptor buffer (set=0)
        cmd.bind_texture_heap(
            &self.font_heap,
            &self.layout,
            0,
            crate::VkPipelineBindPoint::VK_PIPELINE_BIND_POINT_GRAPHICS,
        );

        let pc = UIPushConstants {
            vertex_ptr: self.vertex_buffer.as_ref().unwrap().device_address(),
            window_width: screen_width,
            window_height: screen_height,
            texture_index: self.font_texture_index,
        };
        cmd.push_constants(&self.layout, as_bytes(std::slice::from_ref(&pc)));

        cmd.bind_index_buffer(
            self.index_buffer.as_ref().unwrap(),
            0,
            crate::simple::IndexType::U32,
        );

        // Issue one draw call per clipped primitive with its scissor rect.
        for draw in &self.draws {
            cmd.set_scissor(
                draw.scissor_x,
                draw.scissor_y,
                draw.scissor_w,
                draw.scissor_h,
            );
            cmd.draw_indexed(draw.index_count, 1, draw.first_index, 0, 0);
        }

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
