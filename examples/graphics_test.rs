//! Test of graphics pipeline with swapchain.
//! Renders a rotating colored triangle.
//! Press ESC to exit.

use rust_and_vulkan::simple::Swapchain;
use rust_and_vulkan::simple::{GraphicsPipeline, PipelineLayout, ShaderModule};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};
use std::time::Instant;

/// Convert HSV color to RGB (h: 0-360, s: 0-1, v: 0-1)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 4] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = match h {
        h if h >= 0.0 && h < 60.0 => (c, x, 0.0),
        h if h >= 60.0 && h < 120.0 => (x, c, 0.0),
        h if h >= 120.0 && h < 180.0 => (0.0, c, x),
        h if h >= 180.0 && h < 240.0 => (0.0, x, c),
        h if h >= 240.0 && h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    [r1 + m, g1 + m, b1 + m, 1.0]
}

// Set to true for automated testing (exits after 300 frames)
const TEST_MODE: bool = false;

fn main() -> Result<(), String> {
    println!("Graphics Pipeline Test - Rotating Triangle");
    println!("==========================================");
    println!("Press ESC to exit");

    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Rotating Triangle - Press ESC to exit", 800, 600)?;
    let instance = VulkanInstance::create(&sdl, &window)?;
    let surface = VulkanSurface::create(&window, &instance)?;

    let device = VulkanDevice::create(instance, Some(surface))?;
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("Loading shaders...");
    let vert_bytes = include_bytes!("../shaders/triangle.vert.spv");
    let frag_bytes = include_bytes!("../shaders/triangle.frag.spv");

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
    println!("Shader modules created.");

    println!("Creating pipeline layout...");
    let layout = PipelineLayout::with_vec4_push_constants(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
    )
    .map_err(|e| format!("Failed to create pipeline layout: {}", e))?;
    println!("Pipeline layout created.");

    println!("Creating swapchain...");
    let mut swapchain = Swapchain::new(&context, device.surface.as_ref().unwrap().surface, 800, 600)
        .map_err(|e| format!("Failed to create swapchain: {}", e))?;
    println!("Swapchain created with internal double buffering.");

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
    println!("Graphics pipeline created.");

    let mut quit = false;
    let start_time = Instant::now();
    let mut last_print_time = start_time;
    let mut frame_count = 0;
    let mut frames_rendered = 0;

    while !quit {
        // Get swapchain properties
        let extent = swapchain.extent();
        let render_pass = swapchain.render_pass();

        // Begin frame (handles synchronization and image acquisition internally)
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

        // Update color based on elapsed time
        let elapsed = start_time.elapsed();
        let hue = (elapsed.as_secs_f32() * 60.0) % 360.0;
        let color = hsv_to_rgb(hue, 1.0, 1.0);

        let mut color_bytes = [0u8; 16];
        for i in 0..4 {
            let bytes = color[i].to_ne_bytes();
            color_bytes[i * 4..(i + 1) * 4].copy_from_slice(&bytes);
        }

        cmd.begin()
            .map_err(|e| format!("Failed to begin command buffer: {}", e))?;

        cmd.begin_render_pass(
            render_pass,
            framebuffer,
            extent.width,
            extent.height,
            [0.0, 0.0, 0.0, 1.0],
        );

        cmd.bind_pipeline(&pipeline);
        cmd.push_constants(&layout, &color_bytes);
        cmd.draw(3, 1, 0, 0);

        cmd.end_render_pass();
        cmd.end()
            .map_err(|e| format!("Failed to end command buffer: {}", e))?;

        // End frame (submits command buffer and presents image)
        swapchain
            .end_frame(&context)
            .map_err(|e| format!("Failed to end frame: {}", e))?;

        frame_count += 1;
        frames_rendered += 1;
        let now = Instant::now();
        if now.duration_since(last_print_time).as_secs_f32() >= 1.0 {
            let fps = frame_count as f32 / now.duration_since(last_print_time).as_secs_f32();
            println!("FPS: {:.1}, Color hue: {:.0}°", fps, hue);
            last_print_time = now;
            frame_count = 0;
        }

        if TEST_MODE && frames_rendered >= 300 {
            quit = true;
        }
    }

    context
        .wait_idle()
        .map_err(|e| format!("Failed to wait for device idle: {}", e))?;

    println!("Graphics test completed successfully.");
    Ok(())
}
