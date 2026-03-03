//! Test of graphics pipeline with swapchain and root pointer.
//! Renders a rotating colored triangle with color from root pointer.
//! Press ESC to exit.

use rust_and_vulkan::simple::CommandBuffer;
use rust_and_vulkan::simple::Swapchain;
use rust_and_vulkan::simple::{GpuBumpAllocator, GraphicsPipeline, PipelineLayout, ShaderModule};
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

    // Initialize SDL3 and Vulkan
    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Rotating Triangle - Press ESC to exit", 800, 600)?;
    let instance = VulkanInstance::create(&sdl, &window)?;

    // Create surface
    let surface = VulkanSurface::create(&window, &instance)?;

    // Create Vulkan device
    let device = VulkanDevice::create(instance, Some(surface))?;

    // Create graphics context for simple API
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("Graphics context created successfully.");

    // Load SPIR-V shaders
    println!("Loading shaders...");
    let vert_bytes = include_bytes!("../shaders/triangle.vert.spv");
    let frag_bytes = include_bytes!("../shaders/triangle.frag.spv");

    // Ensure length is multiple of 4 (SPIR-V word size)
    if vert_bytes.len() % 4 != 0 || frag_bytes.len() % 4 != 0 {
        return Err("SPIR-V file size not multiple of 4".to_string());
    }

    // Convert bytes to u32 words (SPIR-V is little-endian)
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

    // Create pipeline layout with push constants for root pointer (graphics stages)
    println!("Creating pipeline layout...");
    let layout = PipelineLayout::with_root_argument(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_VERTEX
            | rust_and_vulkan::simple::SHADER_STAGE_FRAGMENT,
    )
    .map_err(|e| format!("Failed to create pipeline layout: {}", e))?;
    println!("Pipeline layout created.");

    // Create swapchain
    println!("Creating swapchain...");
    let swapchain = Swapchain::new(&context, device.surface.as_ref().unwrap().surface, 800, 600)
        .map_err(|e| format!("Failed to create swapchain: {}", e))?;
    println!("Swapchain created.");

    // Create graphics pipeline using swapchain's render pass
    println!("Creating graphics pipeline...");
    let pipeline = GraphicsPipeline::new(
        &context,
        &vert_shader,
        &frag_shader,
        &layout,
        swapchain.render_pass(),
        rust_and_vulkan::simple::Format::Bgra8Unorm, // matches swapchain format
        None,
        None,
    )
    .map_err(|e| format!("Failed to create graphics pipeline: {}", e))?;
    println!("Graphics pipeline created.");

    // Allocate root data (color)
    println!("Allocating root data...");
    let mut bump_alloc = GpuBumpAllocator::new(&context, 1024 * 1024)
        .map_err(|e| format!("Failed to create bump allocator: {}", e))?;

    #[repr(C, align(16))]
    struct RootData {
        color: [f32; 4],
    }

    let (cpu_ptr, gpu_ptr) = bump_alloc
        .allocate::<RootData>(1)
        .map_err(|e| format!("Failed to allocate root data: {}", e))?;
    println!("Root data allocated at GPU address: 0x{:x}", gpu_ptr);

    // Initialize color (red)
    unsafe {
        (*cpu_ptr).color = [1.0, 0.0, 0.0, 1.0];
    }

    // Allocate command buffer
    println!("Allocating command buffer...");
    let cmd = CommandBuffer::allocate(&context)
        .map_err(|e| format!("Failed to allocate command buffer: {}", e))?;

    // Create semaphore for image acquisition
    let acquire_semaphore = context
        .create_semaphore()
        .map_err(|e| format!("Failed to create semaphore: {}", e))?;

    // Main loop
    let mut quit = false;
    let start_time = Instant::now();
    let mut last_print_time = start_time;
    let mut frame_count = 0;
    let mut frames_rendered = 0;
    const MAX_TEST_FRAMES: u32 = 300;

    while !quit && (!TEST_MODE || frames_rendered < MAX_TEST_FRAMES) {
        // Handle events
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                let event_type = event.type_;
                if event_type == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                } else if event_type == rust_and_vulkan::SDL_EventType::SDL_EVENT_KEY_DOWN as u32 {
                    // Check for ESC key
                    if event.key.key == rust_and_vulkan::SDLK_ESCAPE {
                        quit = true;
                    }
                }
            }
        }

        // Update color based on elapsed time
        let elapsed = start_time.elapsed();
        let hue = (elapsed.as_secs_f32() * 60.0) % 360.0; // Complete color cycle every 6 seconds
        let color = hsv_to_rgb(hue, 1.0, 1.0);

        unsafe {
            (*cpu_ptr).color = color;
        }

        // Acquire next image
        let image_index = swapchain
            .acquire_next_image(acquire_semaphore)
            .map_err(|e| format!("Failed to acquire next image: {}", e))?;

        // Begin recording commands
        cmd.begin()
            .map_err(|e| format!("Failed to begin command buffer: {}", e))?;

        // Begin render pass
        let framebuffer = swapchain.framebuffer(image_index);
        let extent = swapchain.extent();
        cmd.begin_render_pass(
            swapchain.render_pass(),
            framebuffer,
            extent.width,
            extent.height,
            [0.0, 0.0, 0.0, 1.0], // clear color
        );

        // Bind graphics pipeline
        cmd.bind_pipeline(&pipeline);

        // Set root pointer via push constants
        cmd.push_constants(&layout, &gpu_ptr.to_ne_bytes());

        // Draw triangle (3 vertices, no vertex buffer)
        cmd.draw(3, 1, 0, 0);

        // End render pass
        cmd.end_render_pass();

        // End recording
        cmd.end()
            .map_err(|e| format!("Failed to end command buffer: {}", e))?;

        // Submit command buffer and wait for fence (synchronization simplified)
        let fence = context
            .submit(&cmd)
            .map_err(|e| format!("Failed to submit command buffer: {}", e))?;
        fence
            .wait_forever()
            .map_err(|e| format!("Failed to wait for fence: {}", e))?;

        // Present image
        swapchain
            .present(image_index, acquire_semaphore)
            .map_err(|e| format!("Failed to present: {}", e))?;

        // Print FPS every second
        frame_count += 1;
        frames_rendered += 1;
        let now = Instant::now();
        if now.duration_since(last_print_time).as_secs_f32() >= 1.0 {
            let fps = frame_count as f32 / now.duration_since(last_print_time).as_secs_f32();
            println!("FPS: {:.1}, Color hue: {:.0}°", fps, hue);
            last_print_time = now;
            frame_count = 0;
        }
    }

    if TEST_MODE && frames_rendered >= MAX_TEST_FRAMES {
        println!("Test completed after {} frames.", frames_rendered);
    }

    // Cleanup semaphore
    context.destroy_semaphore(acquire_semaphore);

    println!("Graphics test completed successfully.");
    Ok(())
}
