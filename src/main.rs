use rust_and_vulkan::{EguiManager, EguiRenderer};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};

use rust_and_vulkan::simple::{
    Buffer, CommandBuffer, DescriptorPool, DescriptorSet, DescriptorSetLayout, Format,
    GraphicsContext, GraphicsPipeline, PipelineLayout, ShaderModule, Swapchain,
    TextureDescriptorHeap, TextureUsage,
};

use glm as gl;

#[repr(C)]
#[derive(Clone, Copy)]
struct PushConstants {
    // Device address of vertex buffer (used by the vertex shader)
    vertex_ptr: u64,
    // Device address of MPV buffer (mat4) (used by the vertex shader)
    mpv_ptr: u64,

    // Bindless texture selection (used by the fragment shader).
    // The shader expects a 16-byte push-constant block (std430),
    // so we pad to 16 bytes.
    texture_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MPV_PushConstants {
    // Column-major mat4 as expected by GLSL.
    mpv: gl::Mat4,
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

// Vertex data used for device-address vertex pulling.
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    normal: [f32; 3],
    uv: [f32; 2],
}

fn main() -> Result<(), String> {
    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Rotating Square (bindless textures + fallback)", 800, 600)?;

    // Instance + Surface + Device
    let instance = VulkanInstance::create(&sdl, &window)?;
    let surface = VulkanSurface::create(&window, &instance)?;
    let device = VulkanDevice::create(instance, Some(surface))?;

    // Bridge into simple GraphicsContext
    let context = GraphicsContext::new(
        device.instance.instance,
        device.physical_device,
        device.device,
        device.graphics_queue,
        device.present_queue,
        device.command_pool,
        device.descriptor_buffer_supported,
    )
    .map_err(|e| e.to_string())?;

    // Create swapchain
    let surface_khr = device.surface.as_ref().expect("surface expected").surface;
    let mut swapchain =
        Swapchain::new(&context, surface_khr, 800, 600).map_err(|e| e.to_string())?;

    // Shaders
    let vert_spv = load_spirv_u32("shaders/simple_square.vert.spv")?;
    let frag_spv = load_spirv_u32("shaders/simple_square.frag.spv")?;

    let vs = ShaderModule::new(&context, &vert_spv).map_err(|e| e.to_string())?;
    let fs = ShaderModule::new(&context, &frag_spv).map_err(|e| e.to_string())?;

    // Geometry (vertex pulling via buffer device address)
    let square_vertices = vec![
        Vertex {
            pos: [-0.5, -0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: [0.5, -0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            pos: [0.5, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            pos: [0.5, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            pos: [-0.5, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
        },
        Vertex {
            pos: [-0.5, -0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
    ];
    let vertex_buffer = Buffer::vertex_buffer(&context, &square_vertices)
        .map_err(|e| format!("Failed to create square vertex buffer: {}", e))?;

    // Create a buffer for MVP matrix (will be updated each frame)
    let mpv_buffer = {
        let mpv = MPV_PushConstants {
            mpv: num_traits::one(), // identity initially
        };
        Buffer::from_device_address(&context, &[mpv])
            .map_err(|e| format!("Failed to create MPV buffer: {}", e))?
    };

    // Create a tiny 2x2 RGBA texture (used for both bindless and fallback paths).
    let tex_pixels: Vec<u8> = vec![
        255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
    ];
    let texture = context
        .upload_texture(
            &CommandBuffer::allocate(&context).map_err(|e| e.to_string())?,
            &tex_pixels,
            2,
            2,
            Format::Rgba8Unorm,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        )
        .map_err(|e| e.to_string())?;

    let sampler = context
        .create_default_sampler()
        .map_err(|e| e.to_string())?;

    // Bindless heap path (if supported)
    let mut bindless_heap: Option<TextureDescriptorHeap> = None;
    let mut bindless_texture_index: u32 = 0;

    if context.descriptor_buffer_supported() {
        let mut heap = TextureDescriptorHeap::new(&context, 64).map_err(|e| e.to_string())?;
        let idx = heap.allocate().map_err(|e| e.to_string())?;
        heap.write_descriptor(&context, idx, &texture, sampler)
            .map_err(|e| e.to_string())?;
        bindless_texture_index = idx;
        bindless_heap = Some(heap);
    }

    // Fallback descriptor-set path (always available in this demo)
    let set_layout =
        DescriptorSetLayout::new_texture_array(&context, 1).map_err(|e| e.to_string())?;
    let pool = DescriptorPool::new(&context, 1, 1).map_err(|e| e.to_string())?;
    let fallback_set: DescriptorSet = pool.allocate(&set_layout).map_err(|e| e.to_string())?;
    fallback_set
        .write_textures(&context, &[&texture], sampler)
        .map_err(|e| e.to_string())?;

    // Pipeline layout for descriptor-buffer bindless textures:
    //
    // IMPORTANT:
    // Even with VK_EXT_descriptor_buffer, the pipeline layout must still declare a descriptor set
    // layout that matches the shader's set/binding usage. We create a descriptor-buffer-compatible
    // set layout (set=0, binding=0) with a runtime array of combined image samplers, and include it
    // in the pipeline layout.
    //
    // Push constants:
    // The shaders use a 16-byte push-constant block (two u64 in VS, u32+padding in FS). We set 16.
    let bindless_set_layout =
        DescriptorSetLayout::new_bindless_textures(&context, 64).map_err(|e| e.to_string())?;
    let layout = PipelineLayout::with_descriptor_set_layouts_and_push_size(
        &context,
        &[bindless_set_layout],
        rust_and_vulkan::simple::SHADER_STAGE_VERTEX
            | rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
        size_of::<PushConstants>() as u32,
    )
    .map_err(|e| e.to_string())?;

    let pipeline = if context.descriptor_buffer_supported() {
        GraphicsPipeline::new_descriptor_buffer(
            &context,
            &vs,
            &fs,
            &layout,
            swapchain.render_pass(),
            Format::Bgra8Unorm,
            None,
            None,
        )
        .map_err(|e| e.to_string())?
    } else {
        // Fallback path: traditional descriptor sets. This requires a different pipeline layout,
        // but we keep the demo simple by erroring out when bindless is unavailable.
        return Err(
            "Descriptor buffer bindless textures are not supported on this device".to_string(),
        );
    };

    let start = std::time::Instant::now();

    // Initialize egui
    let mut egui_manager = EguiManager::new();
    let mut egui_renderer =
        EguiRenderer::new(&context, swapchain.render_pass()).map_err(|e| e.to_string())?;

    // Event + render loop
    let mut quit = false;
    while !quit {
        // Poll events
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                }
                // Feed events to egui
                egui_manager.handle_event(&event);
            }
        }

        // Begin frame (acquire image, sync, reset cmd)
        if let Err(e) = swapchain.begin_frame() {
            eprintln!("begin_frame failed: {e:?}");
            // For now just continue; in a real app you'd recreate swapchain on OUT_OF_DATE.
            continue;
        }

        let _frame_index = swapchain.current_frame_index();
        let image_index = swapchain.current_image_index();
        let extent = swapchain.extent();
        let cmd: &CommandBuffer = swapchain.current_command_buffer();

        // Begin egui frame
        egui_manager.begin_frame(extent.width as f32, extent.height as f32);

        // Build UI panels
        {
            egui::Window::new("Options")
                .vscroll(true)
                .show(&egui_manager.ctx, |ui| {
                    ui.label("Select an option:");
                    if ui.button("Option 1").clicked() {
                        egui_manager.selected_option = "Option 1".to_string();
                    }
                    if ui.button("Option 2").clicked() {
                        egui_manager.selected_option = "Option 2".to_string();
                    }
                    if ui.button("Option 3").clicked() {
                        egui_manager.selected_option = "Option 3".to_string();
                    }
                });

            egui::Window::new("Data Display")
                .vscroll(true)
                .show(&egui_manager.ctx, |ui| {
                    ui.label("Selected:");
                    ui.label(&egui_manager.selected_option);
                    ui.separator();
                    ui.label("Status:");
                    ui.label(&egui_manager.data_display);
                });
        }

        // End egui frame and get tessellated primitives
        let (clipped_primitives, _textures_delta) = egui_manager.end_frame();

        // Prepare egui renderer with tessellated output
        egui_renderer
            .prepare(&context, clipped_primitives)
            .map_err(|e| e.to_string())?;

        // Record commands
        cmd.begin().map_err(|e| e.to_string())?;

        cmd.begin_render_pass(
            swapchain.render_pass(),
            swapchain.framebuffer(image_index),
            extent.width,
            extent.height,
            [0.05, 0.05, 0.08, 1.0],
        );

        cmd.bind_pipeline(&pipeline);

        // Bind bindless texture heap (descriptor buffer) at set=0.
        // This requires a pipeline created with VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT.
        if let Some(ref heap) = bindless_heap {
            cmd.bind_texture_heap_graphics(heap, &layout, 0);
        } else {
            // If bindless isn't available, we currently don't draw.
            // (You can extend this later to build a descriptor-set-compatible pipeline layout.)
            cmd.end_render_pass();
            cmd.end().map_err(|e| e.to_string())?;
            if let Err(e) = swapchain.end_frame(&context) {
                eprintln!("end_frame failed: {e:?}");
            }
            continue;
        }

        // Update MVP matrix each frame: rotate around Z.
        let t = start.elapsed().as_secs_f32();
        let ident: gl::Mat4 = num_traits::one();
        let rot = gl::ext::rotate(&ident, t, gl::vec3(0.0, 0.0, 1.0));
        let proj: gl::Mat4 = num_traits::one();
        let mvp = proj * rot;

        // Update the MPV buffer with the new matrix
        let mpv_data = MPV_PushConstants { mpv: mvp };
        let mpv_bytes = unsafe {
            std::slice::from_raw_parts(
                (&mpv_data as *const MPV_PushConstants) as *const u8,
                std::mem::size_of::<MPV_PushConstants>(),
            )
        };
        mpv_buffer.write(mpv_bytes).map_err(|e| e.to_string())?;

        // Push the device addresses + texture index for the fragment shader
        let pc = PushConstants {
            vertex_ptr: vertex_buffer.device_address(),
            mpv_ptr: mpv_buffer.device_address(),
            texture_index: bindless_texture_index,
        };

        let pc_bytes = unsafe {
            std::slice::from_raw_parts(
                (&pc as *const PushConstants) as *const u8,
                std::mem::size_of::<PushConstants>(),
            )
        };
        cmd.push_constants(&layout, pc_bytes);

        // Draw 2 triangles (6 vertices)
        cmd.draw(6, 1, 0, 0);

        // Render egui UI on top of scene
        egui_renderer
            .render(cmd, extent.width as f32, extent.height as f32)
            .map_err(|e| e.to_string())?;

        cmd.end_render_pass();
        cmd.end().map_err(|e| e.to_string())?;

        // Submit + present
        if let Err(e) = swapchain.end_frame(&context) {
            eprintln!("end_frame failed: {e:?}");
        }
    }

    // Ensure device idle before drop order tears things down.
    context.wait_idle().map_err(|e| e.to_string())?;

    // Vulkan requires all child objects to be destroyed before destroying the device.
    context.destroy_sampler(sampler);

    Ok(())
}
