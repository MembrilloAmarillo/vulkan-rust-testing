//! Capabilities testing example.
//! Demonstrates:
//! - 2D and 3D graphics pipelines
//! - Cube, sphere, and triangle meshes
//! - Vertex/index buffer creation and indexed drawing
//! - Compute pipeline procedural noise generation
//! - Texture upload and bindless descriptor heap usage

use glm::ext::{look_at, perspective, rotate, scale, translate};
use glm::{mat4, vec3, Mat4};
use rust_and_vulkan::simple::{
    Buffer, CommandBuffer, ComputePipeline, Format, GraphicsPipeline, HazardFlags, IndexType,
    MemoryType, PipelineLayout, RootArguments, ShaderModule, Swapchain, Texture,
    TextureDescriptorHeap, TextureUsage, STAGE_COMPUTE, STAGE_TRANSFER,
};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};
use std::f32::consts::PI;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Draw3DRoot {
    vertex_ptr: u64,
    texture_index: u32,
    material_mode: u32,
    mvp: [f32; 16],
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Draw2DRoot {
    vertex_ptr: u64,
    color: [f32; 4],
    mvp: [f32; 16],
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct NoiseRoot {
    out_ptr: u64,
    width: u32,
    height: u32,
    mode: u32,
    time: f32,
    _pad0: u32,
    _pad1: u32,
}

fn load_spirv_words(bytes: &[u8]) -> Result<Vec<u32>, String> {
    if bytes.len() % 4 != 0 {
        return Err("SPIR-V byte length is not a multiple of 4".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(u32::from_le_bytes(
            chunk
                .try_into()
                .map_err(|_| "Failed to convert SPIR-V bytes to words")?,
        ));
    }
    Ok(out)
}

fn mat4_to_array(m: &Mat4) -> [f32; 16] {
    unsafe { *(m as *const Mat4 as *const [f32; 16]) }
}

fn identity() -> Mat4 {
    mat4(
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    )
}

fn make_cube_mesh() -> (Vec<Vertex>, Vec<u32>) {
    let positions = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];

    let normals = [
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];

    let uvs = [
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];

    let vertices: Vec<Vertex> = (0..8)
        .map(|i| Vertex {
            pos: positions[i],
            normal: normals[i],
            uv: uvs[i],
        })
        .collect();

    let indices = vec![
        // front
        0, 1, 2, 2, 3, 0, // right
        1, 5, 6, 6, 2, 1, // back
        5, 4, 7, 7, 6, 5, // left
        4, 0, 3, 3, 7, 4, // bottom
        0, 4, 5, 5, 1, 0, // top
        3, 2, 6, 6, 7, 3,
    ];

    (vertices, indices)
}

fn make_sphere_mesh(stacks: u32, slices: u32) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for stack in 0..=stacks {
        let v = stack as f32 / stacks as f32;
        let phi = v * PI;
        let y = phi.cos();
        let r = phi.sin();

        for slice in 0..=slices {
            let u = slice as f32 / slices as f32;
            let theta = u * PI * 2.0;
            let x = r * theta.cos();
            let z = r * theta.sin();

            vertices.push(Vertex {
                pos: [x * 0.6, y * 0.6, z * 0.6],
                normal: [x, y, z],
                uv: [u, v],
            });
        }
    }

    let row = slices + 1;
    for stack in 0..stacks {
        for slice in 0..slices {
            let i0 = stack * row + slice;
            let i1 = i0 + 1;
            let i2 = i0 + row;
            let i3 = i2 + 1;

            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }

    (vertices, indices)
}

fn make_triangle_2d_mesh() -> (Vec<Vertex>, Vec<u32>) {
    let vertices = vec![
        Vertex {
            pos: [-0.5, -0.35, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: [0.5, -0.35, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            pos: [0.0, 0.45, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.5, 1.0],
        },
    ];
    let indices = vec![0, 1, 2];
    (vertices, indices)
}

fn generate_noise_texture_data(
    context: &rust_and_vulkan::simple::GraphicsContext,
    compute_pipeline: &ComputePipeline,
    compute_layout: &PipelineLayout,
    width: u32,
    height: u32,
    mode: u32,
    time: f32,
) -> Result<Vec<u8>, String> {
    let bytes = (width as usize) * (height as usize) * 4;

    let output = context
        .gpu_malloc(bytes, 16, MemoryType::CpuMapped)
        .map_err(|e| format!("Failed to allocate compute output buffer: {}", e))?;

    let root = RootArguments::new::<NoiseRoot>(context)
        .map_err(|e| format!("Failed to allocate compute root arguments: {}", e))?;

    root.write(&NoiseRoot {
        out_ptr: output.gpu_ptr,
        width,
        height,
        mode,
        time,
        _pad0: 0,
        _pad1: 0,
    })
    .map_err(|e| format!("Failed to write compute root arguments: {}", e))?;

    let cmd = CommandBuffer::allocate(context)
        .map_err(|e| format!("Failed to allocate compute command buffer: {}", e))?;

    cmd.begin()
        .map_err(|e| format!("Failed to begin compute command buffer: {}", e))?;

    cmd.dispatch(
        compute_pipeline,
        compute_layout,
        root.gpu_address(),
        [(width + 7) / 8, (height + 7) / 8, 1],
    );

    cmd.barrier(STAGE_COMPUTE, STAGE_TRANSFER, HazardFlags::empty())
        .map_err(|e| format!("Failed to insert compute barrier: {}", e))?;

    cmd.end()
        .map_err(|e| format!("Failed to end compute command buffer: {}", e))?;

    let fence = context
        .submit(&cmd)
        .map_err(|e| format!("Failed to submit compute command buffer: {}", e))?;
    fence
        .wait_forever()
        .map_err(|e| format!("Failed waiting for compute fence: {}", e))?;

    let data = unsafe { output.as_slice().to_vec() };
    Ok(data)
}

fn upload_texture_from_data(
    context: &rust_and_vulkan::simple::GraphicsContext,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<Texture, String> {
    let upload_cmd = CommandBuffer::allocate(context)
        .map_err(|e| format!("Failed to allocate upload command buffer: {}", e))?;

    context
        .upload_texture(
            &upload_cmd,
            pixels,
            width,
            height,
            Format::Rgba8Unorm,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        )
        .map_err(|e| format!("Failed to upload texture: {}", e))
}

fn main() -> Result<(), String> {
    println!("Capabilities Testing Example");
    println!("============================");
    println!(
        "Showcasing 2D/3D pipelines, compute noise, bindless descriptors, and indexed meshes."
    );

    let sdl = SdlContext::init()?;
    let window = SdlWindow::new("Capabilities Testing", 1280, 720)?;
    let instance = VulkanInstance::create(&sdl, &window)?;
    let surface = VulkanSurface::create(&window, &instance)?;
    let device = VulkanDevice::create(instance, Some(surface))?;

    let context = device
        .graphics_context()
        .map_err(|e| format!("Failed to create graphics context: {}", e))?;

    println!("UMA detected: {}", context.is_unified_memory());

    // Load shader modules
    let mesh_vert_words = load_spirv_words(include_bytes!("../shaders/capabilities_3d.vert.spv"))?;
    let mesh_frag_words = load_spirv_words(include_bytes!("../shaders/capabilities_3d.frag.spv"))?;
    let overlay_vert_words =
        load_spirv_words(include_bytes!("../shaders/capabilities_2d.vert.spv"))?;
    let overlay_frag_words =
        load_spirv_words(include_bytes!("../shaders/capabilities_2d.frag.spv"))?;
    let compute_words = load_spirv_words(include_bytes!("../shaders/capabilities_noise.comp.spv"))?;

    let mesh_vert = ShaderModule::new(&context, &mesh_vert_words)
        .map_err(|e| format!("Failed to create 3D vertex shader: {}", e))?;
    let mesh_frag = ShaderModule::new(&context, &mesh_frag_words)
        .map_err(|e| format!("Failed to create 3D fragment shader: {}", e))?;

    let overlay_vert = ShaderModule::new(&context, &overlay_vert_words)
        .map_err(|e| format!("Failed to create 2D vertex shader: {}", e))?;
    let overlay_frag = ShaderModule::new(&context, &overlay_frag_words)
        .map_err(|e| format!("Failed to create 2D fragment shader: {}", e))?;

    let compute_shader = ShaderModule::new(&context, &compute_words)
        .map_err(|e| format!("Failed to create compute shader: {}", e))?;

    let compute_layout = PipelineLayout::with_push_constants(&context)
        .map_err(|e| format!("Failed to create compute layout: {}", e))?;
    let compute_pipeline = ComputePipeline::new(&context, &compute_shader, &compute_layout, None)
        .map_err(|e| format!("Failed to create compute pipeline: {}", e))?;

    // Bindless texture heap + sampler
    let mut texture_heap = TextureDescriptorHeap::new(&context, 64)
        .map_err(|e| format!("Failed to create texture heap: {}", e))?;
    let sampler = context
        .create_default_sampler()
        .map_err(|e| format!("Failed to create sampler: {}", e))?;

    // Generate textures from compute noise algorithms
    let texture_size = 256u32;
    let mut textures = Vec::new();
    let mut texture_indices = Vec::new();

    for mode in 0..3u32 {
        let noise_pixels = generate_noise_texture_data(
            &context,
            &compute_pipeline,
            &compute_layout,
            texture_size,
            texture_size,
            mode,
            0.0,
        )?;

        let texture =
            upload_texture_from_data(&context, &noise_pixels, texture_size, texture_size)?;

        let descriptor_index = texture_heap
            .allocate()
            .map_err(|e| format!("Failed allocating texture descriptor: {}", e))?;

        texture_heap
            .write_descriptor(&context, descriptor_index, &texture, sampler)
            .map_err(|e| format!("Failed writing texture descriptor: {}", e))?;

        textures.push(texture);
        texture_indices.push(descriptor_index);
    }

    println!(
        "Generated {} compute textures and wrote them to the bindless descriptor heap.",
        texture_indices.len()
    );

    // Build meshes
    let (cube_vertices, cube_indices) = make_cube_mesh();
    let (sphere_vertices, sphere_indices) = make_sphere_mesh(20, 24);
    let (triangle_vertices, triangle_indices) = make_triangle_2d_mesh();

    let cube_vb = Buffer::vertex_buffer(&context, &cube_vertices)
        .map_err(|e| format!("Failed to create cube vertex buffer: {}", e))?;
    let cube_ib = Buffer::index_buffer_u32(&context, &cube_indices)
        .map_err(|e| format!("Failed to create cube index buffer: {}", e))?;

    let sphere_vb = Buffer::vertex_buffer(&context, &sphere_vertices)
        .map_err(|e| format!("Failed to create sphere vertex buffer: {}", e))?;
    let sphere_ib = Buffer::index_buffer_u32(&context, &sphere_indices)
        .map_err(|e| format!("Failed to create sphere index buffer: {}", e))?;

    let tri_vb = Buffer::vertex_buffer(&context, &triangle_vertices)
        .map_err(|e| format!("Failed to create 2D triangle vertex buffer: {}", e))?;
    let tri_ib = Buffer::index_buffer_u32(&context, &triangle_indices)
        .map_err(|e| format!("Failed to create 2D triangle index buffer: {}", e))?;

    let layout_3d = PipelineLayout::with_bindless_textures(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_ALL_GRAPHICS,
    )
    .map_err(|e| format!("Failed to create 3D layout: {}", e))?;

    let layout_2d = PipelineLayout::with_push_constants_size(
        &context,
        rust_and_vulkan::simple::SHADER_STAGE_ALL_GRAPHICS,
        8,
    )
    .map_err(|e| format!("Failed to create 2D layout: {}", e))?;

    let mut swapchain = Swapchain::new(
        &context,
        device.surface.as_ref().unwrap().surface,
        1280,
        720,
    )
    .map_err(|e| format!("Failed to create swapchain: {}", e))?;

    let pipeline_3d = GraphicsPipeline::new(
        &context,
        &mesh_vert,
        &mesh_frag,
        &layout_3d,
        swapchain.render_pass(),
        Format::Bgra8Unorm,
        None,
        None,
    )
    .map_err(|e| format!("Failed to create 3D pipeline: {}", e))?;

    let pipeline_2d = GraphicsPipeline::new(
        &context,
        &overlay_vert,
        &overlay_frag,
        &layout_2d,
        swapchain.render_pass(),
        Format::Bgra8Unorm,
        None,
        None,
    )
    .map_err(|e| format!("Failed to create 2D pipeline: {}", e))?;

    let root3d = RootArguments::new::<Draw3DRoot>(&context)
        .map_err(|e| format!("Failed to allocate 3D root args: {}", e))?;
    let root2d = RootArguments::new::<Draw2DRoot>(&context)
        .map_err(|e| format!("Failed to allocate 2D root args: {}", e))?;

    let mut quit = false;
    let start = Instant::now();

    while !quit {
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                } else if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_KEY_DOWN as u32
                    && event.key.key == rust_and_vulkan::SDLK_ESCAPE
                {
                    quit = true;
                }
            }
        }

        swapchain
            .begin_frame()
            .map_err(|e| format!("Failed to begin frame: {}", e))?;

        let extent = swapchain.extent();
        let cmd = swapchain.current_command_buffer();
        let framebuffer = swapchain.framebuffer(swapchain.current_image_index());
        let render_pass = swapchain.render_pass();

        let t = start.elapsed().as_secs_f32();
        let aspect = extent.width as f32 / extent.height as f32;

        let projection = perspective(aspect, PI / 3.0, 0.1, 100.0);
        let view = look_at(
            vec3(0.0, 0.4, 4.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
        );

        cmd.begin()
            .map_err(|e| format!("Failed to begin command buffer: {}", e))?;

        cmd.begin_render_pass(
            render_pass,
            framebuffer,
            extent.width,
            extent.height,
            [0.05, 0.07, 0.1, 1.0],
        );

        // 3D pipeline pass: cube + sphere
        cmd.bind_pipeline(&pipeline_3d);
        cmd.bind_texture_heap_graphics(&texture_heap, &layout_3d, 0);

        let mut cube_model = identity();
        cube_model = translate(&cube_model, vec3(-1.0, 0.0, 0.0));
        cube_model = rotate(&cube_model, t * 0.9, vec3(0.5, 1.0, 0.1));
        let cube_mvp = projection * view * cube_model;

        root3d
            .write(&Draw3DRoot {
                vertex_ptr: cube_vb.device_address(),
                texture_index: texture_indices[0],
                material_mode: 0,
                mvp: mat4_to_array(&cube_mvp),
            })
            .map_err(|e| format!("Failed writing cube root args: {}", e))?;

        cmd.set_graphics_root_arguments(&layout_3d, &root3d);
        cmd.bind_vertex_buffer(0, &cube_vb, 0);
        cmd.bind_index_buffer(&cube_ib, 0, IndexType::U32);
        cmd.draw_indexed(cube_indices.len() as u32, 1, 0, 0, 0);

        let mut sphere_model = identity();
        sphere_model = translate(&sphere_model, vec3(1.0, 0.0, 0.0));
        sphere_model = rotate(&sphere_model, t * 0.55, vec3(0.2, 1.0, 0.0));
        let sphere_mvp = projection * view * sphere_model;

        root3d
            .write(&Draw3DRoot {
                vertex_ptr: sphere_vb.device_address(),
                texture_index: texture_indices[1],
                material_mode: 1,
                mvp: mat4_to_array(&sphere_mvp),
            })
            .map_err(|e| format!("Failed writing sphere root args: {}", e))?;

        cmd.set_graphics_root_arguments(&layout_3d, &root3d);
        cmd.bind_vertex_buffer(0, &sphere_vb, 0);
        cmd.bind_index_buffer(&sphere_ib, 0, IndexType::U32);
        cmd.draw_indexed(sphere_indices.len() as u32, 1, 0, 0, 0);

        // 2D pipeline pass: indexed triangle overlay
        cmd.bind_pipeline(&pipeline_2d);

        let mut tri_model = identity();
        tri_model = translate(&tri_model, vec3(0.0, -0.65, 0.0));
        tri_model = scale(&tri_model, vec3(0.9, 0.9, 1.0));
        let tri_mvp = tri_model;

        root2d
            .write(&Draw2DRoot {
                vertex_ptr: tri_vb.device_address(),
                color: [0.95, 0.65, 0.2, 1.0],
                mvp: mat4_to_array(&tri_mvp),
            })
            .map_err(|e| format!("Failed writing 2D root args: {}", e))?;

        cmd.set_graphics_root_arguments(&layout_2d, &root2d);
        cmd.bind_vertex_buffer(0, &tri_vb, 0);
        cmd.bind_index_buffer(&tri_ib, 0, IndexType::U32);
        cmd.draw_indexed(triangle_indices.len() as u32, 1, 0, 0, 0);

        cmd.end_render_pass();
        cmd.end()
            .map_err(|e| format!("Failed to end command buffer: {}", e))?;

        swapchain
            .end_frame(&context)
            .map_err(|e| format!("Failed to end frame: {}", e))?;
    }

    context
        .wait_idle()
        .map_err(|e| format!("Failed waiting for device idle: {}", e))?;
    context.destroy_sampler(sampler);

    // Keep textures alive until after rendering and descriptor usage are done.
    drop(textures);

    Ok(())
}
