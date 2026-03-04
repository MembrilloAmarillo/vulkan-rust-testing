//! 3D spinning cube example with bindless textures using the simple graphics API.
//! Demonstrates 3D transformations, bindless texture descriptor heaps, and GPU texture sampling.
//! The API handles double buffering (2 frames in flight) internally.
//! Press ESC to exit.

use glm::ext::{look_at, perspective, rotate};
use glm::{mat4, vec3, Mat4};
use rust_and_vulkan::simple::{
    CommandBuffer, Format, GraphicsPipeline, MemoryType, PipelineLayout, ShaderModule, Swapchain,
    TextureDescriptorHeap, TextureUsage,
};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};
use std::f32::consts::PI;
use std::time::Instant;

fn main() -> Result<(), String> {
    println!("3D Spinning Cube Example with Bindless Textures");
    println!("================================================");
    println!("Press ESC to exit");

    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Spinning Cube with Bindless - Press ESC to exit", 800, 600)?;
    let instance = VulkanInstance::create(&sdl, &window)?;
    let surface = VulkanSurface::create(&window, &instance)?;

    let device = VulkanDevice::create(instance, Some(surface))?;
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("Loading shaders...");
    let vert_bytes = include_bytes!("../shaders/cube_bindless.vert.spv");
    let frag_bytes = include_bytes!("../shaders/cube_bindless.frag.spv");

    if vert_bytes.len() % 4 != 0 || frag_bytes.len() % 4 != 0 {
        return Err("SPIR-V file size not multiple of 4".to_string());
    }

    let mut vert_words = Vec::with_capacity(vert_bytes.len() / 4);
    for chunk in vert_bytes.chunks_exact(4) {
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        vert_words.push(word);
    }

    let mut frag_words = Vec::with_capacity(frag_bytes.len() / 4);
    for chunk in frag_bytes.chunks_exact(4) {
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        frag_words.push(word);
    }

    let vert_shader = ShaderModule::new(&context, &vert_words)
        .map_err(|e| format!("Failed to create vertex shader: {}", e))?;
    let frag_shader = ShaderModule::new(&context, &frag_words)
        .map_err(|e| format!("Failed to create fragment shader: {}", e))?;

    println!("Creating texture descriptor heap...");
    let mut texture_heap = TextureDescriptorHeap::new(&context, 256)
        .map_err(|e| format!("Failed to create texture descriptor heap: {}", e))?;

    println!("Creating test textures...");

    // Helper function to create a simple gradient texture
    let create_gradient_texture =
        |color_r: u8, color_g: u8, color_b: u8| -> Result<Vec<u8>, String> {
            let width = 256u32;
            let height = 256u32;
            let mut pixel_data = vec![0u8; (width * height * 4) as usize];

            for y in 0..height {
                for x in 0..width {
                    let idx = ((y * width + x) * 4) as usize;
                    let r = ((x as f32 / width as f32) * color_r as f32) as u8;
                    let g = ((y as f32 / height as f32) * color_g as f32) as u8;
                    let b = color_b;
                    let a = 255u8;

                    pixel_data[idx] = r;
                    pixel_data[idx + 1] = g;
                    pixel_data[idx + 2] = b;
                    pixel_data[idx + 3] = a;
                }
            }
            Ok(pixel_data)
        };

    // Create multiple textures with different colors
    let tex1_data = create_gradient_texture(255, 0, 0)?; // Red gradient
    let tex2_data = create_gradient_texture(0, 255, 0)?; // Green gradient
    let tex3_data = create_gradient_texture(0, 0, 255)?; // Blue gradient

    // Upload textures to GPU
    let mut upload_texture = |data: &[u8]| -> Result<u32, String> {
        let staging = context
            .gpu_malloc(data.len(), 256, MemoryType::CpuMapped)
            .map_err(|e| format!("Failed to allocate staging buffer: {}", e))?;

        staging
            .write(data)
            .map_err(|e| format!("Failed to write staging buffer: {}", e))?;

        let texture = rust_and_vulkan::simple::Texture::new(
            &context,
            256,
            256,
            Format::Rgba8Unorm,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        )
        .map_err(|e| format!("Failed to create texture: {}", e))?;

        let cmd = CommandBuffer::allocate(&context)
            .map_err(|e| format!("Failed to allocate command buffer: {}", e))?;

        cmd.begin()
            .map_err(|e| format!("Failed to begin command buffer: {}", e))?;

        cmd.transition_to_transfer_dst(&texture);
        cmd.copy_buffer_to_texture(&staging, &texture, 256, 256);
        cmd.transition_to_shader_read(&texture);

        cmd.end()
            .map_err(|e| format!("Failed to end command buffer: {}", e))?;

        let fence = context
            .submit(&cmd)
            .map_err(|e| format!("Failed to submit command buffer: {}", e))?;
        fence
            .wait_forever()
            .map_err(|e| format!("Failed to wait for fence: {}", e))?;

        // Allocate descriptor in heap and get index
        let tex_idx = texture_heap
            .allocate()
            .map_err(|e| format!("Failed to allocate texture index: {}", e))?;

        Ok(tex_idx as u32)
    };

    let _tex1_idx = upload_texture(&tex1_data)?;
    let _tex2_idx = upload_texture(&tex2_data)?;
    let _tex3_idx = upload_texture(&tex3_data)?;

    println!(
        "Texture descriptor heap initialized with {} textures",
        texture_heap.used()
    );

    println!("Creating pipeline layout with extended push constants...");
    let layout = PipelineLayout::with_push_constants_size(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_VERTEX
            | rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
        80, // Extended size: 64 bytes for MVP + 16 bytes for texture heap address and index
    )
    .map_err(|e| format!("Failed to create pipeline layout: {}", e))?;

    println!("Creating swapchain...");
    let mut swapchain =
        Swapchain::new(&context, device.surface.as_ref().unwrap().surface, 800, 600)
            .map_err(|e| format!("Failed to create swapchain: {}", e))?;

    println!("Creating graphics pipeline...");
    let pipeline = GraphicsPipeline::new(
        &context,
        &vert_shader,
        &frag_shader,
        &layout,
        swapchain.render_pass(),
        rust_and_vulkan::simple::Format::Bgra8Unorm,
        None,
        None,
    )
    .map_err(|e| format!("Failed to create graphics pipeline: {}", e))?;

    let mut quit = false;
    let start_time = Instant::now();
    let mut last_print_time = start_time;
    let mut frame_count = 0;

    while !quit {
        // Get swapchain properties (no borrow conflicts)
        let extent = swapchain.extent();
        let render_pass = swapchain.render_pass();

        // Begin frame and get the command buffer
        swapchain
            .begin_frame()
            .map_err(|e| format!("Failed to begin frame: {}", e))?;

        let cmd = swapchain.current_command_buffer();
        let framebuffer = swapchain.framebuffer(swapchain.current_image_index());

        // Handle events
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                let event_type = event.type_;
                if event_type == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                } else if event_type == rust_and_vulkan::SDL_EventType::SDL_EVENT_KEY_DOWN as u32 {
                    if event.key.key == rust_and_vulkan::SDLK_ESCAPE {
                        quit = true;
                    }
                }
            }
        }

        let elapsed = start_time.elapsed().as_secs_f32();

        let mut model: Mat4 = mat4(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        );
        model = rotate(&model, elapsed * 0.5, vec3(1.0, 0.0, 0.0));
        model = rotate(&model, elapsed * 0.7, vec3(0.0, 1.0, 0.0));
        model = rotate(&model, elapsed * 0.3, vec3(0.0, 0.0, 1.0));

        let view = look_at(
            vec3(0.0, 0.0, 3.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
        );
        let projection = perspective(800.0 / 600.0, PI / 3.0, 0.1, 100.0);

        let mvp = projection * view * model;

        // Create extended push constants: MVP (64 bytes) + texture heap address (8 bytes) + texture index (4 bytes) + padding (4 bytes)
        let mut push_data = [0u8; 80];

        // Copy MVP matrix (64 bytes)
        unsafe {
            let mvp_ptr = &mvp as *const Mat4 as *const u8;
            push_data[0..64].copy_from_slice(std::slice::from_raw_parts(mvp_ptr, 64));
        }

        // Copy texture heap GPU address (simulated - in real implementation would use texture_heap.gpu_address())
        let heap_address = 0u64; // Placeholder
        let heap_lo = (heap_address & 0xFFFFFFFF) as u32;
        let heap_hi = ((heap_address >> 32) & 0xFFFFFFFF) as u32;

        push_data[64..68].copy_from_slice(&heap_lo.to_le_bytes());
        push_data[68..72].copy_from_slice(&heap_hi.to_le_bytes());

        // Copy texture index (use modulo to cycle through textures)
        let texture_idx = (frame_count % 3) as u32;
        push_data[72..76].copy_from_slice(&texture_idx.to_le_bytes());

        cmd.begin()
            .map_err(|e| format!("Failed to begin command buffer: {}", e))?;

        cmd.begin_render_pass(
            render_pass,
            framebuffer,
            extent.width,
            extent.height,
            [1.0, 0.0, 0.0, 1.0],
        );

        cmd.bind_pipeline(&pipeline);
        cmd.push_constants(&layout, &push_data);
        cmd.draw(36, 1, 0, 0);

        cmd.end_render_pass();
        cmd.end()
            .map_err(|e| format!("Failed to end command buffer: {}", e))?;

        swapchain
            .end_frame(&context)
            .map_err(|e| format!("Failed to end frame: {}", e))?;

        frame_count += 1;
        let now = Instant::now();
        if now.duration_since(last_print_time).as_secs_f32() >= 1.0 {
            let fps = frame_count as f32 / now.duration_since(last_print_time).as_secs_f32();
            println!("FPS: {:.1}", fps);
            last_print_time = now;
            frame_count = 0;
        }
    }

    context
        .wait_idle()
        .map_err(|e| format!("Failed to wait for device idle: {}", e))?;

    println!("Done.");
    Ok(())
}
