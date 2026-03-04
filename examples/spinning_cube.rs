//! 3D spinning cube example using the simple graphics API with GLM math.
//! Demonstrates 3D transformations with model-view-projection matrix.
//! The API handles double buffering (2 frames in flight) internally.
//! Press ESC to exit.

use glm::ext::{look_at, perspective, rotate};
use glm::{mat4, vec3, Mat4};
use rust_and_vulkan::simple::{
    GraphicsPipeline, PipelineLayout, ShaderModule, Swapchain,
};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};
use std::f32::consts::PI;
use std::time::Instant;

fn main() -> Result<(), String> {
    println!("3D Spinning Cube Example");
    println!("========================");
    println!("Press ESC to exit");

    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Spinning Cube - Press ESC to exit", 800, 600)?;
    let instance = VulkanInstance::create(&sdl, &window)?;
    let surface = VulkanSurface::create(&window, &instance)?;

    let device = VulkanDevice::create(instance, Some(surface))?;
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("Loading shaders...");
    let vert_bytes = include_bytes!("../shaders/cube.vert.spv");
    let frag_bytes = include_bytes!("../shaders/cube.frag.spv");

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

    println!("Creating pipeline layout...");
    let layout = PipelineLayout::with_mat4_push_constants(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_VERTEX
            | rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
    )
    .map_err(|e| format!("Failed to create pipeline layout: {}", e))?;

    println!("Creating swapchain...");
    let mut swapchain = Swapchain::new(&context, device.surface.as_ref().unwrap().surface, 800, 600)
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

        let view = look_at(vec3(0.0, 0.0, 3.0), vec3(0.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0));
        let projection = perspective(800.0 / 600.0, PI / 3.0, 0.1, 100.0);

        let mvp = projection * view * model;

        let mut mvp_bytes = [0u8; 64];
        unsafe {
            let mvp_ptr = &mvp as *const Mat4 as *const u8;
            mvp_bytes.copy_from_slice(std::slice::from_raw_parts(mvp_ptr, 64));
        }

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
        cmd.push_constants(&layout, &mvp_bytes);
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
