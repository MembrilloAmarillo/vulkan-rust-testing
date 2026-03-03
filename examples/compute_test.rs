//! Test of compute pipeline with buffer device address.
//! Uses a simple compute shader that copies input to output with offset.

use rust_and_vulkan::simple::{ComputePipeline, GpuBumpAllocator, PipelineLayout, ShaderModule};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};

fn main() -> Result<(), String> {
    println!("Compute Pipeline Test");
    println!("=====================");

    // Initialize SDL3 and Vulkan
    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Compute Test", 800, 600)?;
    let instance = VulkanInstance::create(&sdl, &window)?;

    // Create surface (optional for compute)
    let surface = VulkanSurface::create(&window, &instance)?;

    // Create Vulkan device
    let device = VulkanDevice::create(instance, Some(surface))?;

    // Create graphics context for simple API
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("Graphics context created successfully.");

    // Load SPIR-V shader (embedded)
    println!("Loading compute shader...");
    let spirv_bytes = include_bytes!("../shaders/compute.spv");
    // Ensure length is multiple of 4 (SPIR-V word size)
    if spirv_bytes.len() % 4 != 0 {
        return Err("SPIR-V file size not multiple of 4".to_string());
    }
    // Convert bytes to u32 words (SPIR-V is little-endian)
    let mut spirv_words = Vec::with_capacity(spirv_bytes.len() / 4);
    for chunk in spirv_bytes.chunks_exact(4) {
        // SAFETY: chunk is exactly 4 bytes
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        spirv_words.push(word);
    }

    let shader = ShaderModule::new(&context, &spirv_words)
        .map_err(|e| format!("Failed to create shader module: {}", e))?;
    println!("Shader module created.");

    // Create pipeline layout with push constants for root pointer
    println!("Creating pipeline layout...");
    let layout = PipelineLayout::with_push_constants(&context)
        .map_err(|e| format!("Failed to create pipeline layout: {}", e))?;
    println!("Pipeline layout created.");

    // Create compute pipeline
    println!("Creating compute pipeline...");
    let pipeline = ComputePipeline::new(&context, &shader, &layout, None)
        .map_err(|e| format!("Failed to create compute pipeline: {}", e))?;
    println!("Compute pipeline created.");

    // Allocate buffers using bump allocator
    println!("Allocating buffers...");
    let mut bump_alloc = GpuBumpAllocator::new(&context, 1024 * 1024)
        .map_err(|e| format!("Failed to create bump allocator: {}", e))?;

    // Define our root data struct (matches shader)
    #[repr(C, align(16))]
    struct RootData {
        in_value: u32,
        out_value: u32,
    }

    // Allocate root data
    let (cpu_ptr, gpu_ptr) = bump_alloc
        .allocate::<RootData>(1)
        .map_err(|e| format!("Failed to allocate root data: {}", e))?;
    println!("Root data allocated at GPU address: 0x{:x}", gpu_ptr);

    // Initialize input value
    unsafe {
        (*cpu_ptr).in_value = 42;
        (*cpu_ptr).out_value = 0;
    }

    // Allocate command buffer
    println!("Allocating command buffer...");
    let cmd = rust_and_vulkan::simple::CommandBuffer::allocate(&context)
        .map_err(|e| format!("Failed to allocate command buffer: {}", e))?;

    // Begin recording
    cmd.begin()
        .map_err(|e| format!("Failed to begin command buffer: {}", e))?;

    // Dispatch compute with root pointer
    println!("Dispatching compute...");
    cmd.dispatch(&pipeline, &layout, gpu_ptr, [1, 1, 1]);

    // Add barrier to ensure compute completes before reading back
    cmd.barrier(
        rust_and_vulkan::simple::STAGE_COMPUTE,
        rust_and_vulkan::simple::STAGE_TRANSFER,
        rust_and_vulkan::simple::HazardFlags::empty(),
    )
    .map_err(|e| format!("Failed to insert barrier: {}", e))?;

    // End recording
    cmd.end()
        .map_err(|e| format!("Failed to end command buffer: {}", e))?;

    println!("Command buffer recorded.");

    // Submit command buffer and wait for completion
    println!("Submitting command buffer...");
    let fence = context
        .submit(&cmd)
        .map_err(|e| format!("Failed to submit command buffer: {}", e))?;

    println!("Waiting for fence...");
    fence
        .wait_forever()
        .map_err(|e| format!("Failed to wait for fence: {}", e))?;
    println!("Compute shader execution completed.");

    // Read back output value
    unsafe {
        let out_value = (*cpu_ptr).out_value;
        println!("Input value: {}", (*cpu_ptr).in_value);
        println!("Output value: {}", out_value);
        if out_value == 42 {
            println!("SUCCESS: Compute shader correctly copied input to output!");
        } else {
            println!("ERROR: Expected output value 42, got {}", out_value);
            return Err("Compute shader produced incorrect result".to_string());
        }
    }

    println!("Compute test completed successfully.");
    Ok(())
}
