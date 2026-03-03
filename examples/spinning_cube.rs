//! 3D spinning cube example using the simple graphics API.
//! Demonstrates 3D transformations with model-view-projection matrix.
//! Press ESC to exit.

use rust_and_vulkan::simple::CommandBuffer;
use rust_and_vulkan::simple::Swapchain;
use rust_and_vulkan::simple::{GpuBumpAllocator, GraphicsPipeline, PipelineLayout, ShaderModule};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};
use std::f32::consts::PI;
use std::time::Instant;

// Simple 4x4 matrix for 3D transformations
#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct Mat4 {
    data: [[f32; 4]; 4],
}

impl Mat4 {
    fn identity() -> Self {
        Self {
            data: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov_y * 0.5).tan();
        let nf = 1.0 / (near - far);

        Self {
            data: [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [0.0, 0.0, (far + near) * nf, -1.0],
                [0.0, 0.0, (2.0 * far * near) * nf, 0.0],
            ],
        }
    }

    fn look_at(eye: [f32; 3], center: [f32; 3], up: [f32; 3]) -> Self {
        let f = {
            let mut f = [0.0; 3];
            for i in 0..3 {
                f[i] = center[i] - eye[i];
            }
            let len = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            for i in 0..3 {
                f[i] /= len;
            }
            f
        };

        let s = {
            let mut s = [0.0; 3];
            s[0] = f[1] * up[2] - f[2] * up[1];
            s[1] = f[2] * up[0] - f[0] * up[2];
            s[2] = f[0] * up[1] - f[1] * up[0];
            let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
            for i in 0..3 {
                s[i] /= len;
            }
            s
        };

        let u = {
            let mut u = [0.0; 3];
            u[0] = s[1] * f[2] - s[2] * f[1];
            u[1] = s[2] * f[0] - s[0] * f[2];
            u[2] = s[0] * f[1] - s[1] * f[0];
            u
        };

        Self {
            data: [
                [s[0], s[1], s[2], 0.0],
                [u[0], u[1], u[2], 0.0],
                [-f[0], -f[1], -f[2], 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
        .translate([-eye[0], -eye[1], -eye[2]])
    }

    fn translate(&self, v: [f32; 3]) -> Self {
        let mut result = *self;
        result.data[3][0] = self.data[0][0] * v[0]
            + self.data[1][0] * v[1]
            + self.data[2][0] * v[2]
            + self.data[3][0];
        result.data[3][1] = self.data[0][1] * v[0]
            + self.data[1][1] * v[1]
            + self.data[2][1] * v[2]
            + self.data[3][1];
        result.data[3][2] = self.data[0][2] * v[0]
            + self.data[1][2] * v[1]
            + self.data[2][2] * v[2]
            + self.data[3][2];
        result.data[3][3] = self.data[0][3] * v[0]
            + self.data[1][3] * v[1]
            + self.data[2][3] * v[2]
            + self.data[3][3];
        result
    }

    fn rotate_x(&self, angle: f32) -> Self {
        let s = angle.sin();
        let c = angle.cos();
        let rotation = Self {
            data: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, c, s, 0.0],
                [0.0, -s, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        self.mul(&rotation)
    }

    fn rotate_y(&self, angle: f32) -> Self {
        let s = angle.sin();
        let c = angle.cos();
        let rotation = Self {
            data: [
                [c, 0.0, -s, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [s, 0.0, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        self.mul(&rotation)
    }

    fn rotate_z(&self, angle: f32) -> Self {
        let s = angle.sin();
        let c = angle.cos();
        let rotation = Self {
            data: [
                [c, s, 0.0, 0.0],
                [-s, c, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        self.mul(&rotation)
    }

    fn mul(&self, other: &Self) -> Self {
        let mut result = Self::identity();
        for i in 0..4 {
            for j in 0..4 {
                result.data[i][j] = self.data[i][0] * other.data[0][j]
                    + self.data[i][1] * other.data[1][j]
                    + self.data[i][2] * other.data[2][j]
                    + self.data[i][3] * other.data[3][j];
            }
        }
        result
    }
}

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

    // Allocate root data (MVP matrix)
    println!("Allocating root data...");
    let mut bump_alloc = GpuBumpAllocator::new(&context, 1024 * 1024)
        .map_err(|e| format!("Failed to create bump allocator: {}", e))?;

    #[repr(C, align(16))]
    struct Uniforms {
        mvp: Mat4,
    }

    let (cpu_ptr, gpu_ptr) = bump_alloc
        .allocate::<Uniforms>(1)
        .map_err(|e| format!("Failed to allocate root data: {}", e))?;
    println!("Root data allocated at GPU address: 0x{:x}", gpu_ptr);

    // Initialize with identity matrix
    unsafe {
        (*cpu_ptr).mvp = Mat4::identity();
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

        // Create model matrix with rotation
        let model = Mat4::identity()
            .rotate_x(elapsed * 0.5)
            .rotate_y(elapsed * 0.7)
            .rotate_z(elapsed * 0.3);

        // Create view matrix (camera)
        let view = Mat4::look_at(
            [0.0, 0.0, 3.0], // eye
            [0.0, 0.0, 0.0], // center
            [0.0, 1.0, 0.0], // up
        );

        // Create projection matrix
        let projection = Mat4::perspective(
            PI / 3.0,      // 60 degrees FOV
            800.0 / 600.0, // aspect ratio
            0.1,           // near
            100.0,         // far
        );

        // MVP = projection * view * model
        let mvp = projection.mul(&view).mul(&model);

        unsafe {
            (*cpu_ptr).mvp = mvp;
        }
        println!("MVP matrix updated, gpu_ptr=0x{:x}", gpu_ptr);

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
            [0.1, 0.1, 0.1, 1.0], // dark gray clear color
        );

        // Bind graphics pipeline
        cmd.bind_pipeline(&pipeline);

        // Set root pointer via push constants
        cmd.push_constants(&layout, &gpu_ptr.to_ne_bytes());

        // Draw cube (36 vertices = 12 triangles * 3 vertices)
        cmd.draw(36, 1, 0, 0);

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

        frames_rendered += 1;
        if frames_rendered >= 5 {
            println!("Rendered 5 frames, exiting.");
            quit = true;
        }

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

    // Cleanup semaphore
    context.destroy_semaphore(acquire_semaphore);

    println!("Spinning cube example completed successfully.");
    Ok(())
}
