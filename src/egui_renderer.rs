//! egui Vulkan renderer using device addresses.
//!
//! Converts egui tessellated output into Vulkan draw calls.
//! Uses device address buffers for minimal overhead.

use egui::ClippedPrimitive;

use crate::simple::{
    Buffer, CommandBuffer, Format, GraphicsContext, GraphicsPipeline, PipelineLayout, ShaderModule,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct UIVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: u32, // sRGB packed as RGBA
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UIPushConstants {
    vertex_ptr: u64,
    projection: [[f32; 4]; 4],
}

pub struct EguiRenderer {
    pipeline: GraphicsPipeline,
    layout: PipelineLayout,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_count: usize,
    index_count: usize,
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

        // Create pipeline layout with push constants
        let layout = PipelineLayout::with_push_constants_size(
            context,
            crate::simple::SHADER_STAGE_VERTEX,
            std::mem::size_of::<UIPushConstants>() as u32,
        )
        .map_err(|e| e.to_string())?;

        // Create graphics pipeline
        let pipeline = GraphicsPipeline::new(
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

        Ok(EguiRenderer {
            pipeline,
            layout,
            vertex_buffer: None,
            index_buffer: None,
            vertex_count: 0,
            index_count: 0,
        })
    }

    /// Update buffers with new egui output
    pub fn prepare(
        &mut self,
        context: &GraphicsContext,
        clipped_primitives: Vec<ClippedPrimitive>,
    ) -> Result<(), String> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for ClippedPrimitive { primitive, .. } in clipped_primitives {
            match primitive {
                egui::epaint::Primitive::Mesh(mesh) => {
                    let index_offset = vertices.len() as u32;

                    // Convert egui mesh to our vertex format
                    for vertex in &mesh.vertices {
                        let color = vertex.color;
                        let [r, g, b, a] = color.to_srgba_unmultiplied();
                        let packed_color = ((a as u32) << 24)
                            | ((b as u32) << 16)
                            | ((g as u32) << 8)
                            | (r as u32);

                        vertices.push(UIVertex {
                            position: [vertex.pos.x, vertex.pos.y],
                            uv: [vertex.uv.x, vertex.uv.y],
                            color: packed_color,
                        });
                    }

                    // Add indices with offset
                    for index in &mesh.indices {
                        indices.push(index_offset + index);
                    }
                }
                egui::epaint::Primitive::Callback(_) => {
                    // Skip callbacks for now
                }
            }
        }

        self.vertex_count = vertices.len();
        self.index_count = indices.len();

        // Update or create vertex buffer
        if !vertices.is_empty() {
            self.vertex_buffer = Some(
                Buffer::from_device_address(context, &vertices)
                    .map_err(|e| format!("Failed to create vertex buffer: {}", e))?,
            );
        }

        // Update or create index buffer
        if !indices.is_empty() {
            self.index_buffer = Some(
                Buffer::from_device_address(context, &indices)
                    .map_err(|e| format!("Failed to create index buffer: {}", e))?,
            );
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
            return Ok(()); // Nothing to render
        }

        cmd.bind_pipeline(&self.pipeline);

        // Orthographic projection matrix
        let proj = ortho_projection(0.0, screen_width, screen_height, 0.0);

        let pc = UIPushConstants {
            vertex_ptr: self.vertex_buffer.as_ref().unwrap().device_address(),
            projection: proj,
        };

        let pc_bytes = unsafe {
            std::slice::from_raw_parts(
                (&pc as *const UIPushConstants) as *const u8,
                std::mem::size_of::<UIPushConstants>(),
            )
        };
        cmd.push_constants(&self.layout, pc_bytes);

        // Draw vertices
        if let (Some(vbuf), Some(ibuf)) = (&self.vertex_buffer, &self.index_buffer) {
            cmd.bind_vertex_buffer(0, vbuf, 0);
            cmd.bind_index_buffer(ibuf, 0, crate::simple::IndexType::U32);
            cmd.draw_indexed(self.index_count as u32, 1, 0, 0, 0);
        }

        Ok(())
    }

    pub fn pipeline(&self) -> &GraphicsPipeline {
        &self.pipeline
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

fn ortho_projection(left: f32, right: f32, bottom: f32, top: f32) -> [[f32; 4]; 4] {
    [
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
        [
            -(right + left) / (right - left),
            -(top + bottom) / (top - bottom),
            0.0,
            1.0,
        ],
    ]
}
