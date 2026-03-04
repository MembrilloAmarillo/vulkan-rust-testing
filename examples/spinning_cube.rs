//! 3D spinning cube example using the simple graphics API with GLM math.
//! Demonstrates 3D transformations with model-view-projection matrix.
//! Press ESC to exit.

use glm::ext::{look_at, perspective, rotate};
use glm::{mat4, vec3, Mat4};
use rust_and_vulkan::simple::{
    CommandBuffer, GraphicsPipeline, PipelineLayout, ShaderModule, Swapchain,
};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};
use std::f32::consts::PI;
use std::time::Instant;

fn main() -> Result<(), String> {
    println!("3D Spinning Cube Example");
    println!("========================");
    println!("Press ESC to exit");

    // Initialize SDL3 and Vulkan
    let sdl = SdlContext::init()?;
    eprintln!("SDL initialized");
    let window = SdlWindow::new("Spinning Cube - Press ESC to exit", 800, 600)?;
    eprintln!("Window created");
    let instance = VulkanInstance::create(&sdl, &window)?;
    eprintln!("Vulkan instance created");
    // Create surface
    let surface = VulkanSurface::create(&window, &instance)?;
    eprintln!("Vulkan surface created");

    // Create Vulkan device
    eprintln!("Creating Vulkan device...");
    let device = VulkanDevice::create(instance, Some(surface))?;
    eprintln!("Vulkan device created");
    // Create graphics context for simple API
    eprintln!("Creating graphics context...");
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;
    eprintln!("Graphics context created successfully.");

    println!("Graphics context created successfully.");

    // Load SPIR-V shaders
    println!("Loading shaders...");
    let vert_bytes = include_bytes!("../shaders/cube.vert.spv");
    let frag_bytes = include_bytes!("../shaders/cube.frag.spv");

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

    // Create pipeline layout with push constants for MVP matrix (graphics stages)
    println!("Creating pipeline layout...");
    let layout = PipelineLayout::with_mat4_push_constants(
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

    // Allocate command buffer
    println!("Allocating command buffer...");
    let cmd = CommandBuffer::allocate(&context)
        .map_err(|e| format!("Failed to allocate command buffer: {}", e))?;

    // Create semaphores for image acquisition (one per swapchain image to avoid conflicts)
    let swapchain_image_count = swapchain.image_count() as usize;
    let mut image_available_semaphores = Vec::new();
    let mut render_finished_semaphores = Vec::new();

    for _ in 0..swapchain_image_count {
        image_available_semaphores.push(
            context
                .create_semaphore()
                .map_err(|e| format!("Failed to create semaphore: {}", e))?,
        );
        render_finished_semaphores.push(
            context
                .create_semaphore()
                .map_err(|e| format!("Failed to create semaphore: {}", e))?,
        );
    }

    // Main loop
    let mut quit = false;
    let start_time = Instant::now();
    let mut last_print_time = start_time;
    let mut frame_count = 0;

    while !quit {
        // Handle events
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                let event_type = event.type_;
                if event_type == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    println!("SDL_EVENT_QUIT received");
                    quit = true;
                } else if event_type == rust_and_vulkan::SDL_EventType::SDL_EVENT_KEY_DOWN as u32 {
                    // Check for ESC key
                    if event.key.key == rust_and_vulkan::SDLK_ESCAPE {
                        println!("ESC key pressed");
                        quit = true;
                    }
                }
            }
        }

        // Update MVP matrix based on elapsed time
        let elapsed = start_time.elapsed().as_secs_f32();

        // Create model matrix with rotation using GLM
        let mut model: Mat4 = mat4(
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        );
        model = rotate(&model, elapsed * 0.5, vec3(1.0, 0.0, 0.0));
        model = rotate(&model, elapsed * 0.7, vec3(0.0, 1.0, 0.0));
        model = rotate(&model, elapsed * 0.3, vec3(0.0, 0.0, 1.0));

        // Create view matrix (camera) using GLM
        let eye = vec3(0.0, 0.0, 3.0);
        let center = vec3(0.0, 0.0, 0.0);
        let up = vec3(0.0, 1.0, 0.0);
        let view = look_at(eye, center, up);

        // Create projection matrix using GLM
        let projection = perspective(
            800.0 / 600.0, // aspect ratio
            PI / 3.0,      // 60 degrees FOV
            0.1,           // near
            100.0,         // far
        );

        // MVP = projection * view * model
        let mvp = projection * view * model;

        // Debug: Print MVP matrix on first frame
        if frame_count == 0 && elapsed < 0.1 {
            println!("\nMVP Matrix (first frame):");
            for row in 0..4 {
                println!(
                    "  [{:.4}, {:.4}, {:.4}, {:.4}]",
                    mvp.c0[row], mvp.c1[row], mvp.c2[row], mvp.c3[row]
                );
            }
            println!();
        }

        // Prepare MVP matrix bytes for push constants
        // GLM stores matrices in column-major order, so we can pack directly
        let mut mvp_bytes = [0u8; 64]; // mat4 = 4x4 f32 = 64 bytes
        unsafe {
            let mvp_ptr = &mvp as *const Mat4 as *const u8;
            mvp_bytes.copy_from_slice(std::slice::from_raw_parts(mvp_ptr, 64));
        }

        // Acquire next image (signals image_available_semaphore when ready)
        let image_index = swapchain
            .acquire_next_image(image_available_semaphores[0])
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
            [1.0, 0.0, 0.0, 1.0], // red clear color
        );

        // Bind graphics pipeline
        cmd.bind_pipeline(&pipeline);

        // Set MVP matrix via push constants (64 bytes = mat4)
        cmd.push_constants(&layout, &mvp_bytes);

        // Draw cube (36 vertices = 12 triangles * 3 vertices)
        cmd.draw(36, 1, 0, 0);

        // End render pass
        cmd.end_render_pass();

        // End recording
        cmd.end()
            .map_err(|e| format!("Failed to end command buffer: {}", e))?;

        // Submit command buffer with proper semaphore synchronization
        // Use semaphores indexed by image_index to avoid conflicts
        let fence = context
            .submit_with_semaphores(
                &cmd,
                &[image_available_semaphores[0]],
                &[render_finished_semaphores[image_index as usize]],
            )
            .map_err(|e| format!("Failed to submit command buffer: {}", e))?;
        fence
            .wait_forever()
            .map_err(|e| format!("Failed to wait for fence: {}", e))?;

        // Present image
        swapchain
            .present(
                image_index,
                render_finished_semaphores[image_index as usize],
            )
            .map_err(|e| format!("Failed to present: {}", e))?;

        // Print FPS every second
        frame_count += 1;
        let now = Instant::now();
        if now.duration_since(last_print_time).as_secs_f32() >= 1.0 {
            let fps = frame_count as f32 / now.duration_since(last_print_time).as_secs_f32();
            println!("FPS: {:.1}, Rotation time: {:.1}s", fps, elapsed);
            last_print_time = now;
            frame_count = 0;
        }
    }

    // Wait for GPU to finish all operations before cleanup
    context
        .wait_idle()
        .map_err(|e| format!("Failed to wait for device idle: {}", e))?;

    // Cleanup semaphores
    for sem in image_available_semaphores {
        context.destroy_semaphore(sem);
    }
    for sem in render_finished_semaphores {
        context.destroy_semaphore(sem);
    }

    println!("Spinning cube example completed successfully.");
    Ok(())
}
