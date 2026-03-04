//! Demonstration of bindless texture system inspired by "No Graphics API" article.
//! Shows texture descriptor heap, root arguments, and texture upload.

use rust_and_vulkan::simple::{
    CommandBuffer, PipelineLayout, RootArguments, TextureDescriptorHeap,
};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};

fn main() -> Result<(), String> {
    println!("Bindless Texture System Demo");
    println!("============================");
    println!("Demonstrating concepts from 'No Graphics API' article");
    println!();

    // Initialize SDL3 and Vulkan
    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Bindless Texture Demo", 800, 600)?;
    let instance = VulkanInstance::create(&sdl, &window)?;

    // Create surface
    let surface = VulkanSurface::create(&window, &instance)?;

    // Create Vulkan device
    let device = VulkanDevice::create(instance, Some(surface))?;

    // Create graphics context for simple API
    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("1. Graphics context created successfully.");
    println!();

    // Test 1: Create texture descriptor heap
    println!("2. Creating texture descriptor heap...");
    let mut texture_heap = TextureDescriptorHeap::new(&context, 1024)
        .map_err(|e| format!("Failed to create texture descriptor heap: {}", e))?;
    println!("   Texture descriptor heap created:");
    println!("   - Capacity: {} descriptors", texture_heap.capacity());
    println!(
        "   - Descriptor size: {} bytes",
        texture_heap.descriptor_size()
    );
    println!("   - GPU address: 0x{:x}", texture_heap.gpu_address());
    println!();

    // Test 2: Allocate texture descriptor indices
    println!("3. Allocating texture descriptor indices...");
    let tex1_index = texture_heap
        .allocate()
        .map_err(|e| format!("Failed to allocate texture index: {}", e))?;
    let tex2_index = texture_heap
        .allocate()
        .map_err(|e| format!("Failed to allocate texture index: {}", e))?;
    println!(
        "   Allocated texture indices: {} and {}",
        tex1_index, tex2_index
    );
    println!("   Used descriptors: {}", texture_heap.used());
    println!();

    // Test 3: Create root arguments
    println!("4. Creating root arguments...");

    // Define root data struct matching shader expectations
    #[repr(C, align(16))]
    struct MaterialData {
        albedo_texture_index: u32,
        normal_texture_index: u32,
        roughness: f32,
        metallic: f32,
        _padding: [u32; 2], // Pad to 32 bytes for alignment
    }

    let root_args = RootArguments::new::<MaterialData>(&context)
        .map_err(|e| format!("Failed to create root arguments: {}", e))?;

    // Initialize root data
    let material_data = MaterialData {
        albedo_texture_index: tex1_index,
        normal_texture_index: tex2_index,
        roughness: 0.5,
        metallic: 0.0,
        _padding: [0, 0],
    };

    root_args
        .write(&material_data)
        .map_err(|e| format!("Failed to write root arguments: {}", e))?;

    println!("   Root arguments created:");
    println!("   - Size: {} bytes", root_args.size());
    println!("   - GPU address: 0x{:x}", root_args.gpu_address());
    println!();

    // Test 4: Create command buffer
    println!("5. Creating command buffer...");
    let cmd = CommandBuffer::allocate(&context)
        .map_err(|e| format!("Failed to allocate command buffer: {}", e))?;
    println!("   Command buffer allocated.");
    println!();

    // Test 5: Create pipeline layout with root argument support
    println!("6. Creating pipeline layout...");
    let pipeline_layout = PipelineLayout::with_root_argument(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_ALL_GRAPHICS,
    )
    .map_err(|e| format!("Failed to create pipeline layout: {}", e))?;
    println!("   Pipeline layout created with root argument support.");
    println!(
        "   Push constant size: {} bytes",
        pipeline_layout.push_constant_size()
    );
    println!();

    // Test 6: Demonstrate texture upload (conceptual)
    println!("7. Texture upload demonstration:");
    println!("   The upload_texture() method would:");
    println!("   - Allocate GPU-only memory for optimal texture storage");
    println!("   - Create staging buffer in CPU-mapped memory");
    println!("   - Perform copy with automatic layout transitions");
    println!("   - Enable DCC compression for optimal memory usage");
    println!("   - Submit commands and wait for completion");
    println!();

    // Test 7: Bind texture heap to command buffer
    println!("8. Binding texture heap to command buffer...");
    cmd.bind_texture_heap(&texture_heap, &pipeline_layout, 0);
    println!("   Texture heap bound (conceptual - actual implementation pending).");
    println!();

    // Test 8: Begin command buffer and set root arguments
    println!("9. Beginning command buffer and setting root arguments...");
    cmd.begin()
        .map_err(|e| format!("Failed to begin command buffer: {}", e))?;
    cmd.set_graphics_root_arguments(&pipeline_layout, &root_args);
    cmd.end()
        .map_err(|e| format!("Failed to end command buffer: {}", e))?;
    println!("   Root arguments set via push constants.");
    println!();

    println!("=== Summary ===");
    println!("The bindless texture system provides:");
    println!("  • TextureDescriptorHeap for managing 256-bit descriptors");
    println!("  • RootArguments for simplified shader data passing");
    println!("  • Single 64-bit pointer shader input model");
    println!("  • Texture upload with GPU-only memory and DCC support");
    println!("  • Bindless texture indexing (32-bit indices)");
    println!();
    println!("This aligns with the 'No Graphics API' philosophy by:");
    println!("  1. Eliminating descriptor set management");
    println!("  2. Using direct GPU pointers instead of complex binding models");
    println!("  3. Simplifying pipeline state creation");
    println!("  4. Enabling efficient material/texture switching");
    println!();

    // Event loop
    let mut quit = false;
    let mut frames = 0;
    while !quit && frames < 120 {
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
        frames += 1;
    }

    println!("Demo completed successfully.");
    println!("All bindless texture system components demonstrated.");
    Ok(())
}
