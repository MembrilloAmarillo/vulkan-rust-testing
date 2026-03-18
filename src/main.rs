use rust_and_vulkan::automation::AutomationFileLoader;
use rust_and_vulkan::ecss_automation::{ExecutionEvent, ExecutionStats};
use rust_and_vulkan::{EguiManager, EguiRenderer};
use rust_and_vulkan::{ProgramConfig, ProgramRunner, RuntimeConfig};
use rust_and_vulkan::{SdlContext, SdlWindow, VulkanDevice, VulkanInstance, VulkanSurface};

use rust_and_vulkan::simple::{
    Buffer, BufferUsage, CommandBuffer, DescriptorPool, DescriptorSet, DescriptorSetLayout, Format,
    GraphicsContext, GraphicsPipeline, GraphicsPipelineConfig, MemoryType, PipelineLayout,
    ShaderModule, Swapchain, TextureDescriptorHeap, TextureUsage,
};

use glm as gl;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::net::UdpSocket;

fn send_commander_udp(target: &str, command_line: &str) -> Result<String, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind failed: {}", e))?;
    socket
        .set_read_timeout(Some(std::time::Duration::from_millis(600)))
        .map_err(|e| format!("set_read_timeout failed: {}", e))?;

    socket
        .send_to(command_line.as_bytes(), target)
        .map_err(|e| format!("send_to failed: {}", e))?;

    let mut buf = [0u8; 512];
    match socket.recv_from(&mut buf) {
        Ok((n, _)) => {
            let reply = String::from_utf8_lossy(&buf[..n]).to_string();
            Ok(reply)
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
        {
            Ok("sent (no immediate reply)".to_string())
        }
        Err(e) => Err(format!("recv_from failed: {}", e)),
    }
}

fn parse_program_args(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(|s| s.to_string()).collect()
}

fn parse_program_env(raw: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            if !key.is_empty() {
                env.insert(key.to_string(), v.trim().to_string());
            }
        }
    }
    env
}

fn render_virtualized_code_view(
    ui: &mut egui::Ui,
    content: &str,
    line_starts: &[usize],
    highlight_command_name: Option<&str>,
) {
    let total_lines = line_starts.len().saturating_sub(1);
    if total_lines == 0 {
        ui.label("(empty file)");
        return;
    }

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both().auto_shrink([false; 2]).show_rows(
        ui,
        row_height,
        total_lines,
        |ui, row_range| {
            for row in row_range {
                let start = line_starts[row];
                let end = line_starts[row + 1];
                let mut line = &content[start..end];
                if line.ends_with('\n') {
                    line = &line[..line.len() - 1];
                }

                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::GRAY, format!("{:>7} ", row + 1));
                    let highlight_line = highlight_command_name
                        .map(|name| !name.is_empty() && line.contains(name))
                        .unwrap_or(false);
                    if highlight_line {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .background_color(egui::Color32::from_rgb(255, 245, 170)),
                        );
                    } else {
                        ui.monospace(line);
                    }
                });
            }
        },
    );
}

enum AutomationThreadMessage {
    Progress(ExecutionEvent),
    Finished(Result<ExecutionStats, String>),
}

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
    // Deliver mouse-button-down events even when the click is what gives the
    // window focus (i.e. don't swallow the first click as a focus-gain event).
    unsafe {
        rust_and_vulkan::SDL_SetHint(
            rust_and_vulkan::SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH.as_ptr() as *const i8,
            b"1\0".as_ptr() as *const i8,
        );
    }
    let window = SdlWindow::new("AutomationWare", 800, 600)?;

    // SDL3 requires explicit text-input activation for reliable TEXT_INPUT events.
    // Without this, egui text boxes may not receive typed characters.
    unsafe {
        let ok = rust_and_vulkan::SDL_StartTextInput(window.window);
        if !ok {
            eprintln!("Warning: SDL_StartTextInput failed");
        }
    }

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

    // Create swapchain using actual drawable pixel size (important for HiDPI).
    let surface_khr = device.surface.as_ref().expect("surface expected").surface;
    let mut drawable_width = 0i32;
    let mut drawable_height = 0i32;
    unsafe {
        rust_and_vulkan::SDL_GetWindowSizeInPixels(
            window.window,
            &mut drawable_width,
            &mut drawable_height,
        );
    }
    if drawable_width <= 0 || drawable_height <= 0 {
        drawable_width = 800;
        drawable_height = 600;
    }
    let mut swapchain = Swapchain::new(
        &context,
        surface_khr,
        drawable_width as u32,
        drawable_height as u32,
    )
    .map_err(|e| e.to_string())?;

    let use_bindless_descriptor_buffer = context.descriptor_buffer_supported();
    if !use_bindless_descriptor_buffer {
        eprintln!(
            "Descriptor buffer extension unavailable; falling back to traditional descriptor sets"
        );
    }

    // Bindless heap path (if supported)
    let mut bindless_heap: Option<TextureDescriptorHeap> = None;
    let mut bindless_texture_index: u32 = 0;

    let start = std::time::Instant::now();

    // Initialize egui
    let mut egui_manager = EguiManager::new();
    let mut egui_renderer =
        EguiRenderer::new(&context, swapchain.render_pass()).map_err(|e| e.to_string())?;

    // Simple frame-rate estimator for UI display.
    let mut last_frame_time = std::time::Instant::now();
    let mut refresh_label = String::with_capacity(64); // Pre-allocate to prevent reallocation
    refresh_label.push_str("Current refresh: --.- Hz");
    let mut last_refresh_label_update = std::time::Instant::now();
    let refresh_label_update_interval = std::time::Duration::from_millis(16);

    // Initialize automation file loader
    let mut automation_loader = AutomationFileLoader::default();
    automation_loader.refresh_files();
    let mut filter_last_edit_at: Option<std::time::Instant> = None;
    let filter_debounce = std::time::Duration::from_millis(16);

    // Automation execution state
    let mut automation_executing = false;
    let mut automation_stats: Option<ExecutionStats> = None;
    let mut automation_error: Option<String> = None;
    let mut automation_current_command: Option<String> = None;
    let mut automation_progress_label = String::new();
    let mut automation_log: Vec<String> = Vec::new();
    // Channel through which a background execution thread reports its result
    let mut automation_rx: Option<std::sync::mpsc::Receiver<AutomationThreadMessage>> = None;

    // Commander UDP UI state
    let mut commander_show_window = true;
    let mut commander_target_host = "127.0.0.1".to_string();
    let mut commander_target_port: u16 = 8092;
    let mut ftp_list_path = "/".to_string();
    let mut ftp_download_remote = "/remote/file.bin".to_string();
    let mut ftp_download_local = "./downloaded_file.bin".to_string();
    let mut delete_file_path = "/remote/file.bin".to_string();
    let mut delete_all_prefix = "/remote/folder".to_string();
    let mut commander_status = String::new();
    let mut commander_log: Vec<String> = Vec::new();

    // Program runner UI state
    let mut program_runner_show_window = true;
    let program_runner_config_path = if std::env::var("APPIMAGE").is_ok() {
        // Running as AppImage - use home directory for config
        let config_dir = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(".config/rust-and-vulkan");
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
        config_dir.join("program_runner.toml")
    } else {
        // Running normally - use current directory
        std::env::current_dir()
            .map_err(|e| format!("Failed to read current dir: {}", e))?
            .join("program_runner.toml")
    };
    let mut program_runner = ProgramRunner::load_or_create(&program_runner_config_path)
        .map_err(|e| format!("Failed to initialize program runner config: {}", e))?;
    let mut program_runner_status = String::new();

    // Program form fields
    let mut prg_id = String::new();
    let mut prg_name = String::new();
    let mut prg_command = String::new();
    let mut prg_args = String::new();
    let mut prg_working_dir = String::new();
    let mut prg_env = String::new();
    let mut prg_auto_start = false;
    let mut prg_use_python_venv = false;
    let mut prg_venv_path = String::new();
    let mut prg_use_conda_env = false;
    let mut prg_conda_env_name = String::new();

    // Event + render loop
    let mut quit = false;
    let mut window_resized = false;
    while !quit {
        let now = std::time::Instant::now();
        let frame_dt = now.duration_since(last_frame_time).as_secs_f32();
        last_frame_time = now;

        if now.duration_since(last_refresh_label_update) >= refresh_label_update_interval {
            refresh_label.clear();
            let hz = if frame_dt > 0.0 { 1.0 / frame_dt } else { 0.0 };
            let _ = write!(refresh_label, "Current refresh: {:.4} Hz", hz);
            last_refresh_label_update = now;
        }

        automation_loader.poll_file_load();

        if let Err(e) = program_runner.poll() {
            program_runner_status = format!("Poll error: {}", e);
        }

        // Poll background automation thread for results
        if automation_executing {
            if let Some(rx) = &automation_rx {
                let mut finished_result: Option<Result<ExecutionStats, String>> = None;
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        AutomationThreadMessage::Progress(event) => match event {
                            ExecutionEvent::CommandStarted {
                                index,
                                total,
                                name,
                                description,
                            } => {
                                automation_current_command = Some(name.clone());
                                automation_progress_label = if description.trim().is_empty() {
                                    format!("Running {}/{}: {}", index, total, name)
                                } else {
                                    format!(
                                        "Running {}/{}: {} — {}",
                                        index, total, name, description
                                    )
                                };
                                automation_log
                                    .push(format!("▶ {}/{} START {}", index, total, name));
                            }
                            ExecutionEvent::CommandSucceeded {
                                index,
                                total,
                                name,
                                elapsed_ms,
                            } => {
                                automation_log.push(format!(
                                    "✓ {}/{} OK {} ({} ms)",
                                    index, total, name, elapsed_ms
                                ));
                            }
                            ExecutionEvent::CommandFailed {
                                index,
                                total,
                                name,
                                error,
                            } => {
                                automation_log.push(format!(
                                    "✗ {}/{} FAIL {} — {}",
                                    index, total, name, error
                                ));
                            }
                        },
                        AutomationThreadMessage::Finished(result) => {
                            finished_result = Some(result);
                        }
                    }
                }

                if let Some(result) = finished_result {
                    automation_executing = false;
                    automation_rx = None;
                    automation_current_command = None;
                    match result {
                        Ok(stats) => automation_stats = Some(stats),
                        Err(e) => automation_error = Some(e),
                    }
                }

                if automation_log.len() > 200 {
                    let remove_count = automation_log.len() - 200;
                    automation_log.drain(0..remove_count);
                }
            }
        }

        if let Some(last_edit) = filter_last_edit_at {
            if now.duration_since(last_edit) >= filter_debounce {
                automation_loader.refresh_files();
                filter_last_edit_at = None;
            }
        }

        let mut input_pixels_per_point =
            unsafe { rust_and_vulkan::SDL_GetWindowDisplayScale(window.window) };
        if !input_pixels_per_point.is_finite() || input_pixels_per_point <= 0.0 {
            input_pixels_per_point = 1.0;
        }

        // Poll events
        unsafe {
            let mut event = std::mem::zeroed();
            while rust_and_vulkan::SDL_PollEvent(&mut event) {
                if event.type_ == rust_and_vulkan::SDL_EventType::SDL_EVENT_QUIT as u32 {
                    quit = true;
                } else if event.type_
                    == rust_and_vulkan::SDL_EventType::SDL_EVENT_WINDOW_RESIZED as u32
                    || event.type_
                        == rust_and_vulkan::SDL_EventType::SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED
                            as u32
                {
                    window_resized = true;
                }
                // Feed events to egui
                egui_manager.handle_event(&event, input_pixels_per_point);
            }
        }

        // Handle window resize by recreating swapchain
        if window_resized {
            // Get new window size
            let mut width = 0i32;
            let mut height = 0i32;
            unsafe {
                rust_and_vulkan::SDL_GetWindowSizeInPixels(window.window, &mut width, &mut height);
            }

            if width > 0 && height > 0 {
                match swapchain.recreate(&context, surface_khr, width as u32, height as u32) {
                    Ok(_) => println!("Swapchain recreated: {}x{}", width, height),
                    Err(e) => eprintln!("Failed to recreate swapchain: {e:?}"),
                }
            }
            window_resized = false;
        }

        // Begin frame (acquire image, sync, reset cmd)
        if let Err(e) = swapchain.begin_frame() {
            eprintln!("begin_frame failed: {e:?}");
            // On OUT_OF_DATE error, try to recreate the swapchain
            if matches!(e, rust_and_vulkan::simple::Error::Vulkan(ref msg) if msg.contains("out of date"))
            {
                let mut width = 0i32;
                let mut height = 0i32;
                unsafe {
                    rust_and_vulkan::SDL_GetWindowSizeInPixels(
                        window.window,
                        &mut width,
                        &mut height,
                    );
                }

                if width > 0 && height > 0 {
                    if let Err(e) =
                        swapchain.recreate(&context, surface_khr, width as u32, height as u32)
                    {
                        eprintln!("Failed to recreate swapchain: {e:?}");
                    }
                }
            }
            continue;
        }

        let _frame_index = swapchain.current_frame_index();
        let image_index = swapchain.current_image_index();
        let extent = swapchain.extent();
        let cmd: &CommandBuffer = swapchain.current_command_buffer();

        // Begin egui frame in framebuffer pixel space.
        egui_manager.begin_frame(extent.width as f32, extent.height as f32);

        // Build UI panels
        {
            egui::Window::new("Data Display")
                .vscroll(true)
                .show(&egui_manager.ctx, |ui| {
                    ui.label(egui::RichText::new(refresh_label.as_str()));
                });

            if commander_show_window {
                egui::Window::new("Commander UDP")
                    .open(&mut commander_show_window)
                    .default_width(520.0)
                    .default_height(460.0)
                    .show(&egui_manager.ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Host:");
                            ui.text_edit_singleline(&mut commander_target_host);
                            ui.label("Port:");
                            ui.add(
                                egui::DragValue::new(&mut commander_target_port).range(1..=65535),
                            );
                        });

                        let target =
                            format!("{}:{}", commander_target_host.trim(), commander_target_port);

                        ui.separator();
                        ui.heading("Actions");

                        if ui.button("HPC_SEND").clicked() {
                            let cmd = "HPC_SEND".to_string();
                            match send_commander_udp(&target, &cmd) {
                                Ok(reply) => {
                                    commander_status = format!("{} -> {}", cmd, reply);
                                    commander_log.push(commander_status.clone());
                                }
                                Err(e) => {
                                    commander_status = format!("{} -> ERROR {}", cmd, e);
                                    commander_log.push(commander_status.clone());
                                }
                            }
                        }

                        ui.separator();
                        ui.label("FTP_LIST <path>");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut ftp_list_path);
                            if ui.button("Send FTP_LIST").clicked() {
                                let cmd = format!("FTP_LIST {}", ftp_list_path.trim());
                                match send_commander_udp(&target, &cmd) {
                                    Ok(reply) => {
                                        commander_status = format!("{} -> {}", cmd, reply);
                                        commander_log.push(commander_status.clone());
                                    }
                                    Err(e) => {
                                        commander_status = format!("{} -> ERROR {}", cmd, e);
                                        commander_log.push(commander_status.clone());
                                    }
                                }
                            }
                        });

                        ui.separator();
                        ui.label("FTP_DOWNLOAD <remote> <local>");
                        ui.text_edit_singleline(&mut ftp_download_remote);
                        ui.text_edit_singleline(&mut ftp_download_local);
                        if ui.button("Send FTP_DOWNLOAD").clicked() {
                            let cmd = format!(
                                "FTP_DOWNLOAD {} {}",
                                ftp_download_remote.trim(),
                                ftp_download_local.trim()
                            );
                            match send_commander_udp(&target, &cmd) {
                                Ok(reply) => {
                                    commander_status = format!("{} -> {}", cmd, reply);
                                    commander_log.push(commander_status.clone());
                                }
                                Err(e) => {
                                    commander_status = format!("{} -> ERROR {}", cmd, e);
                                    commander_log.push(commander_status.clone());
                                }
                            }
                        }

                        ui.separator();
                        ui.label("DELETE_FILE <path>");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut delete_file_path);
                            if ui.button("Send DELETE_FILE").clicked() {
                                let cmd = format!("DELETE_FILE {}", delete_file_path.trim());
                                match send_commander_udp(&target, &cmd) {
                                    Ok(reply) => {
                                        commander_status = format!("{} -> {}", cmd, reply);
                                        commander_log.push(commander_status.clone());
                                    }
                                    Err(e) => {
                                        commander_status = format!("{} -> ERROR {}", cmd, e);
                                        commander_log.push(commander_status.clone());
                                    }
                                }
                            }
                        });

                        ui.separator();
                        ui.label("DELETE_ALL <prefix>");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut delete_all_prefix);
                            if ui.button("Send DELETE_ALL").clicked() {
                                let cmd = format!("DELETE_ALL {}", delete_all_prefix.trim());
                                match send_commander_udp(&target, &cmd) {
                                    Ok(reply) => {
                                        commander_status = format!("{} -> {}", cmd, reply);
                                        commander_log.push(commander_status.clone());
                                    }
                                    Err(e) => {
                                        commander_status = format!("{} -> ERROR {}", cmd, e);
                                        commander_log.push(commander_status.clone());
                                    }
                                }
                            }
                        });

                        if commander_log.len() > 200 {
                            let remove_count = commander_log.len() - 200;
                            commander_log.drain(0..remove_count);
                        }

                        ui.separator();
                        if !commander_status.is_empty() {
                            ui.label(format!("Status: {}", commander_status));
                        }

                        egui::CollapsingHeader::new("UDP Log")
                            .default_open(true)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(120.0)
                                    .show(ui, |ui| {
                                        for line in commander_log.iter().rev().take(20) {
                                            ui.monospace(line);
                                        }
                                    });
                            });
                    });
            }

            if program_runner_show_window {
                egui::Window::new("📦 Program Runner")
                    .open(&mut program_runner_show_window)
                    .default_width(900.0)
                    .default_height(700.0)
                    .show(&egui_manager.ctx, |ui| {
                        // Status bar at top
                        if !program_runner_status.is_empty() {
                            let status_color = if program_runner_status.contains("Error")
                                || program_runner_status.contains("failed")
                            {
                                egui::Color32::from_rgb(220, 50, 50)
                            } else if program_runner_status.contains("saved") {
                                egui::Color32::from_rgb(50, 180, 50)
                            } else {
                                egui::Color32::from_rgb(100, 150, 200)
                            };
                            ui.colored_label(status_color, format!("ℹ {}", program_runner_status));
                            ui.separator();
                        }

                        // Global action buttons in a prominent bar
                        ui.horizontal(|ui| {
                            ui.heading("🎮 Quick Actions");
                            ui.separator();

                            if ui.button("▶ Start Auto").clicked() {
                                match program_runner.start_auto_programs() {
                                    Ok(handles) => {
                                        program_runner_status =
                                            format!("Started {} auto program(s)", handles.len())
                                    }
                                    Err(e) => {
                                        program_runner_status = format!("Start auto failed: {}", e)
                                    }
                                }
                            }

                            if ui.button("⏹ Stop All").clicked() {
                                match program_runner.stop_all() {
                                    Ok(_) => {
                                        program_runner_status =
                                            "Stopped all running programs".to_string()
                                    }
                                    Err(e) => {
                                        program_runner_status = format!("Stop all failed: {}", e)
                                    }
                                }
                            }

                            if ui.button("💾 Save").clicked() {
                                match program_runner.save() {
                                    Ok(_) => {
                                        program_runner_status =
                                            "Config saved successfully".to_string()
                                    }
                                    Err(e) => {
                                        program_runner_status =
                                            format!("Failed to save config: {}", e)
                                    }
                                }
                            }
                        });

                        ui.separator();

                        // Two-column layout
                        ui.columns(2, |columns| {
                            // LEFT COLUMN: Program list
                            columns[0].vertical(|ui| {
                                ui.group(|ui| {
                                    ui.heading("📋 Configured Programs");
                                    ui.separator();

                                    let programs_snapshot: Vec<ProgramConfig> =
                                        program_runner.list_programs().to_vec();
                                    let running_pairs = program_runner.running_programs();
                                    let mut running_by_program: HashMap<String, u32> =
                                        HashMap::new();
                                    for (handle, program_id) in running_pairs {
                                        running_by_program.insert(program_id.to_string(), handle);
                                    }

                                    let mut start_program_id: Option<String> = None;
                                    let mut stop_handle: Option<u32> = None;
                                    let mut remove_program_id: Option<String> = None;
                                    let mut edit_program_id: Option<String> = None;

                                    if programs_snapshot.is_empty() {
                                        ui.colored_label(
                                            egui::Color32::from_rgb(150, 150, 150),
                                            "No programs configured yet",
                                        );
                                    } else {
                                        egui::ScrollArea::vertical().max_height(450.0).show(
                                            ui,
                                            |ui| {
                                                for p in &programs_snapshot {
                                                    ui.group(|ui| {
                                                        ui.horizontal(|ui| {
                                                            let status_color = if let Some(handle) =
                                                                running_by_program.get(&p.id)
                                                            {
                                                                ui.colored_label(
                                                                    egui::Color32::from_rgb(
                                                                        50, 200, 50,
                                                                    ),
                                                                    "●",
                                                                );
                                                                ui.label(
                                                                    egui::RichText::new(&p.name)
                                                                        .strong(),
                                                                );
                                                                ui.colored_label(
                                                                    egui::Color32::from_rgb(
                                                                        50, 200, 50,
                                                                    ),
                                                                    format!("(#{})", handle),
                                                                );

                                                                if ui.button("⏹").clicked() {
                                                                    stop_handle = Some(*handle);
                                                                }
                                                            } else {
                                                                ui.colored_label(
                                                                    egui::Color32::from_rgb(
                                                                        200, 150, 50,
                                                                    ),
                                                                    "◯",
                                                                );
                                                                ui.label(&p.name);

                                                                if ui.button("▶").clicked() {
                                                                    start_program_id =
                                                                        Some(p.id.clone());
                                                                }
                                                            };

                                                            if ui.button("\u{270F}").clicked() {
                                                                edit_program_id =
                                                                    Some(p.id.clone());
                                                            }

                                                            if ui.button("\u{2716}").clicked() {
                                                                remove_program_id =
                                                                    Some(p.id.clone());
                                                            }
                                                        });

                                                        ui.horizontal(|ui| {
                                                            ui.label("cmd:");
                                                            ui.monospace(format!(
                                                                "{}{}",
                                                                p.command,
                                                                if p.args.is_empty() {
                                                                    String::new()
                                                                } else {
                                                                    format!(" {}", p.args.join(" "))
                                                                }
                                                            ));
                                                        });

                                                        if let Some(dir) = &p.working_dir {
                                                            ui.small(format!(
                                                                "📁 {}",
                                                                dir.display()
                                                            ));
                                                        }

                                                        match &p.runtime {
                                                            RuntimeConfig::Direct => {
                                                                ui.small("🔧 Runtime: Direct")
                                                            }
                                                            RuntimeConfig::PythonVenv {
                                                                venv_path,
                                                            } => ui.small(format!(
                                                                "🐍 Python Venv: {}",
                                                                venv_path
                                                            )),
                                                            RuntimeConfig::CondaEnv {
                                                                env_name,
                                                            } => ui.small(format!(
                                                                "🐍 Conda: {}",
                                                                env_name
                                                            )),
                                                        };

                                                        if p.auto_start {
                                                            ui.small("⚡ Auto-start enabled");
                                                        }
                                                    });
                                                }
                                            },
                                        );
                                    }

                                    // Handle deferred actions
                                    if let Some(id) = start_program_id {
                                        match program_runner.start_program(&id) {
                                            Ok(handle) => {
                                                program_runner_status =
                                                    format!("✓ Started '{}' #{}", id, handle)
                                            }
                                            Err(e) => {
                                                program_runner_status =
                                                    format!("✗ Failed to start '{}': {}", id, e)
                                            }
                                        }
                                    }

                                    if let Some(handle) = stop_handle {
                                        match program_runner.stop_program(handle) {
                                            Ok(_) => {
                                                program_runner_status =
                                                    format!("✓ Stopped program #{}", handle)
                                            }
                                            Err(e) => {
                                                program_runner_status =
                                                    format!("✗ Failed to stop #{}: {}", handle, e)
                                            }
                                        }
                                    }

                                    if let Some(id) = remove_program_id {
                                        match program_runner.remove_program(&id) {
                                            Ok(true) => {
                                                program_runner_status =
                                                    format!("✓ Removed program '{}'", id)
                                            }
                                            Ok(false) => {
                                                program_runner_status =
                                                    format!("Program '{}' not found", id)
                                            }
                                            Err(e) => {
                                                program_runner_status =
                                                    format!("✗ Failed to remove '{}': {}", id, e)
                                            }
                                        }
                                    }

                                    if let Some(id) = edit_program_id {
                                        if let Some(existing) = programs_snapshot
                                            .iter()
                                            .find(|program| program.id == id)
                                        {
                                            prg_id = existing.id.clone();
                                            prg_name = existing.name.clone();
                                            prg_command = existing.command.clone();
                                            prg_args = existing.args.join(" ");
                                            prg_working_dir = existing
                                                .working_dir
                                                .as_ref()
                                                .map(|p| p.display().to_string())
                                                .unwrap_or_default();
                                            prg_auto_start = existing.auto_start;
                                            prg_env = if existing.env.is_empty() {
                                                String::new()
                                            } else {
                                                let mut rows: Vec<String> = existing
                                                    .env
                                                    .iter()
                                                    .map(|(k, v)| format!("{}={}", k, v))
                                                    .collect();
                                                rows.sort();
                                                rows.join("\n")
                                            };

                                            match &existing.runtime {
                                                RuntimeConfig::Direct => {
                                                    prg_use_python_venv = false;
                                                    prg_venv_path.clear();
                                                    prg_use_conda_env = false;
                                                    prg_conda_env_name.clear();
                                                }
                                                RuntimeConfig::PythonVenv { venv_path } => {
                                                    prg_use_python_venv = true;
                                                    prg_venv_path = venv_path.clone();
                                                    prg_use_conda_env = false;
                                                    prg_conda_env_name.clear();
                                                }
                                                RuntimeConfig::CondaEnv { env_name } => {
                                                    prg_use_python_venv = false;
                                                    prg_venv_path.clear();
                                                    prg_use_conda_env = true;
                                                    prg_conda_env_name = env_name.clone();
                                                }
                                            }
                                        }
                                    }
                                });
                            });

                            // RIGHT COLUMN: Program editor
                            columns[1].vertical(|ui| {
                                ui.group(|ui| {
                                    ui.heading("\u{2795} Add / Edit Program");
                                    ui.separator();

                                    let is_editing = !prg_id.is_empty();
                                    if is_editing {
                                        ui.colored_label(
                                            egui::Color32::from_rgb(100, 180, 220),
                                            format!("\u{270F} Editing: {}", prg_id),
                                        );
                                    }

                                    ui.separator();
                                    ui.label("Program Identity");
                                    ui.horizontal(|ui| {
                                        ui.label("ID:");
                                        ui.text_edit_singleline(&mut prg_id);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Name:");
                                        ui.text_edit_singleline(&mut prg_name);
                                    });

                                    ui.separator();
                                    ui.label("Execution");
                                    ui.horizontal(|ui| {
                                        ui.label("Command:");
                                        ui.text_edit_singleline(&mut prg_command);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Args:");
                                        ui.text_edit_singleline(&mut prg_args);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Working dir:");
                                        ui.text_edit_singleline(&mut prg_working_dir);
                                    });

                                    ui.separator();
                                    ui.label("Runtime Environment");
                                    ui.horizontal(|ui| {
                                        if ui
                                            .selectable_value(
                                                &mut prg_use_python_venv,
                                                false,
                                                "Direct",
                                            )
                                            .clicked()
                                        {
                                            prg_use_conda_env = false;
                                        }
                                        if ui
                                            .selectable_value(
                                                &mut prg_use_python_venv,
                                                true,
                                                "Python Venv",
                                            )
                                            .clicked()
                                        {
                                            prg_use_conda_env = false;
                                        }
                                        if ui
                                            .selectable_value(
                                                &mut prg_use_conda_env,
                                                true,
                                                "Conda Env",
                                            )
                                            .clicked()
                                        {
                                            prg_use_python_venv = false;
                                        }
                                    });

                                    if prg_use_python_venv && !prg_use_conda_env {
                                        ui.horizontal(|ui| {
                                            ui.label("Venv path:");
                                            ui.text_edit_singleline(&mut prg_venv_path);
                                        });
                                    }

                                    if prg_use_conda_env && !prg_use_python_venv {
                                        ui.horizontal(|ui| {
                                            ui.label("Conda env:");
                                            ui.text_edit_singleline(&mut prg_conda_env_name);
                                        });
                                    }

                                    ui.separator();
                                    ui.label("Environment Variables");
                                    ui.add(
                                        egui::TextEdit::multiline(&mut prg_env)
                                            .desired_rows(3)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("KEY=VALUE (one per line)"),
                                    );

                                    ui.separator();
                                    ui.checkbox(&mut prg_auto_start, "⚡ Auto-start on load");

                                    ui.separator();
                                    ui.horizontal(|ui| {
                                        if ui.button("💾 Save Program").clicked() {
                                            let runtime = if prg_use_python_venv
                                                && !prg_use_conda_env
                                            {
                                                RuntimeConfig::PythonVenv {
                                                    venv_path: prg_venv_path.trim().to_string(),
                                                }
                                            } else if prg_use_conda_env && !prg_use_python_venv {
                                                RuntimeConfig::CondaEnv {
                                                    env_name: prg_conda_env_name.trim().to_string(),
                                                }
                                            } else {
                                                RuntimeConfig::Direct
                                            };

                                            let program = ProgramConfig {
                                                id: prg_id.trim().to_string(),
                                                name: prg_name.trim().to_string(),
                                                command: prg_command.trim().to_string(),
                                                args: parse_program_args(&prg_args),
                                                working_dir: if prg_working_dir.trim().is_empty() {
                                                    None
                                                } else {
                                                    Some(std::path::PathBuf::from(
                                                        prg_working_dir.trim(),
                                                    ))
                                                },
                                                env: parse_program_env(&prg_env),
                                                auto_start: prg_auto_start,
                                                runtime,
                                                last_run_unix: None,
                                                last_exit_code: None,
                                            };

                                            match program_runner.upsert_program(program) {
                                                Ok(_) => {
                                                    program_runner_status =
                                                        "✓ Program saved".to_string()
                                                }
                                                Err(e) => {
                                                    program_runner_status =
                                                        format!("✗ Save failed: {}", e)
                                                }
                                            }
                                        }

                                        if ui.button("🗑 Clear").clicked() {
                                            prg_id.clear();
                                            prg_name.clear();
                                            prg_command.clear();
                                            prg_args.clear();
                                            prg_working_dir.clear();
                                            prg_env.clear();
                                            prg_auto_start = false;
                                            prg_use_python_venv = false;
                                            prg_venv_path.clear();
                                            prg_use_conda_env = false;
                                            prg_conda_env_name.clear();
                                        }
                                    });
                                });
                            });
                        });

                        ui.separator();
                        ui.small(format!(
                            "📁 Config: {}",
                            program_runner.config_path().display()
                        ));
                    });
            }

            // Handle automation UI interactions first (without borrowing automation_loader in closures)
            let mut should_navigate_up = false;
            let mut should_refresh = false;
            let mut file_to_load: Option<std::path::PathBuf> = None;

            // Automation File Browser Window
            if automation_loader.show_browser {
                egui::Window::new("Automation File Loader")
                    .open(&mut automation_loader.show_browser)
                    .default_width(500.0)
                    .default_height(400.0)
                    .vscroll(true)
                    .show(&egui_manager.ctx, |ui| {
                        ui.heading("File Browser");

                        // Current directory display
                        ui.horizontal(|ui| {
                            ui.label("Location:");
                            let mut dir_str =
                                automation_loader.current_dir.to_string_lossy().to_string();
                            ui.text_edit_singleline(&mut dir_str);
                        });

                        // Navigation buttons
                        ui.horizontal(|ui| {
                            if ui.button("↑ Parent Directory").clicked() {
                                should_navigate_up = true;
                            }
                            if ui.button("🔄 Refresh").clicked() {
                                should_refresh = true;
                            }
                        });

                        // File filter
                        ui.horizontal(|ui| {
                            ui.label("Filter:");
                            let filter_edit =
                                ui.text_edit_singleline(&mut automation_loader.file_filter);
                            if filter_edit.changed() {
                                filter_last_edit_at = Some(now);
                            }
                            if filter_edit.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                should_refresh = true;
                                filter_last_edit_at = None;
                            }
                        });

                        ui.separator();

                        // File list
                        ui.label(format!("Files ({}):", automation_loader.files.len()));

                        for file in &automation_loader.files {
                            let selected =
                                automation_loader.selected_file.as_ref() == Some(&file.path);

                            if ui
                                .selectable_label(selected, file.display_label.as_str())
                                .clicked()
                            {
                                file_to_load = Some(file.path.clone());
                            }
                        }

                        // Error message display
                        if let Some(error) = &automation_loader.error_message {
                            ui.separator();
                            ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                        }
                    });
            }

            // Process deferred actions
            if should_navigate_up {
                automation_loader.navigate_up();
            }
            if should_refresh {
                automation_loader.refresh_files();
            }
            if let Some(path) = file_to_load {
                if path.is_dir() {
                    automation_loader.navigate_to(&path);
                } else {
                    automation_loader.load_file(&path);
                }
            }

            // Code Display Window
            if automation_loader.show_code_display {
                // Pre-capture values to avoid borrow conflicts
                let file_name = automation_loader.current_file_name();
                let file_content = automation_loader.file_content.clone();
                let can_execute = file_name
                    .as_ref()
                    .map(|n| n.ends_with(".toml") || n.ends_with(".json"))
                    .unwrap_or(false);

                let mut window_open = true;
                egui::Window::new("Code Display")
                    .open(&mut window_open)
                    .default_width(700.0)
                    .default_height(600.0)
                    .vscroll(false)
                    .show(&egui_manager.ctx, |ui| {
                        ui.vertical(|ui| {
                            if let Some(name) = &file_name {
                                ui.heading(format!("📄 {}", name));
                            }
                            ui.separator();

                            // Keep code view stable during resize by allocating a fixed portion
                            // of the remaining space and keeping controls below it.
                            let code_height = (ui.available_height() * 0.60).max(120.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), code_height),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    if automation_loader.is_loading_file {
                                        ui.label("Loading file…");
                                    } else if let (Some(content), Some(line_starts)) = (
                                        automation_loader.file_content.as_deref(),
                                        automation_loader.file_line_starts.as_deref(),
                                    ) {
                                        render_virtualized_code_view(
                                            ui,
                                            content,
                                            line_starts,
                                            automation_current_command.as_deref(),
                                        );
                                    } else {
                                        ui.label("No file loaded");
                                    }
                                },
                            );

                            ui.separator();

                            ui.horizontal(|ui| {
                                if can_execute {
                                    let running = automation_executing;
                                    let btn_text = if running {
                                        "⏳ Running…"
                                    } else {
                                        "▶ Run Automation"
                                    };
                                    if ui
                                        .add_enabled(!running, egui::Button::new(btn_text))
                                        .clicked()
                                    {
                                        if let (Some(name), Some(content)) = (&file_name, &file_content)
                                        {
                                            let parse_result = if name.ends_with(".toml") {
                                                rust_and_vulkan::ecss_automation::AutomationEngine::from_toml_str(content)
                                            } else {
                                                rust_and_vulkan::ecss_automation::AutomationEngine::from_json_str(content)
                                            };
                                            match parse_result {
                                                Ok(engine) => {
                                                    automation_executing = true;
                                                    automation_stats = None;
                                                    automation_error = None;
                                                    automation_current_command = None;
                                                    automation_progress_label.clear();
                                                    automation_log.clear();
                                                    let (tx, rx) = std::sync::mpsc::channel();
                                                    automation_rx = Some(rx);
                                                    std::thread::spawn(move || {
                                                        let rt = tokio::runtime::Runtime::new()
                                                            .expect("tokio runtime");
                                                        let result = rt
                                                            .block_on(async {
                                                                engine
                                                                    .execute_with_progress(|event| {
                                                                        let _ = tx.send(
                                                                            AutomationThreadMessage::Progress(
                                                                                event,
                                                                            ),
                                                                        );
                                                                    })
                                                                    .await
                                                            })
                                                            .map_err(|e| e.to_string());
                                                        let _ = tx.send(
                                                            AutomationThreadMessage::Finished(result),
                                                        );
                                                    });
                                                }
                                                Err(e) => {
                                                    automation_error =
                                                        Some(format!("Parse error: {}", e));
                                                }
                                            }
                                        }
                                    }
                                } else if file_name.is_some() {
                                    ui.label("⚠ Only .toml / .json files can be executed");
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Close").clicked() {
                                            automation_loader.show_code_display = false;
                                        }
                                    },
                                );
                            });

                            if let Some(stats) = &automation_stats {
                                let color = if stats.failed == 0 {
                                    egui::Color32::from_rgb(0, 160, 0)
                                } else {
                                    egui::Color32::from_rgb(200, 140, 0)
                                };
                                ui.colored_label(
                                    color,
                                    format!(
                                        "✓ Done: {}/{} commands, {} ms, {:.1}% success",
                                        stats.successful,
                                        stats.successful + stats.failed,
                                        stats.elapsed_ms,
                                        stats.success_rate()
                                    ),
                                );
                            }
                            if automation_executing {
                                if automation_progress_label.is_empty() {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(50, 120, 200),
                                        "Running automation…",
                                    );
                                } else {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(50, 120, 200),
                                        automation_progress_label.as_str(),
                                    );
                                }
                            }
                            if let Some(error) = &automation_error {
                                ui.colored_label(egui::Color32::RED, format!("❌ {}", error));
                            }

                            egui::CollapsingHeader::new("Execution Log")
                                .default_open(automation_executing)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(130.0)
                                        .auto_shrink([false; 2])
                                        .show(ui, |ui| {
                                            if automation_log.is_empty() {
                                                ui.label("No entries yet");
                                            } else {
                                                for entry in &automation_log {
                                                    ui.monospace(entry);
                                                }
                                            }
                                        });
                                });
                        });
                    });

                automation_loader.show_code_display = window_open;
            }
        }

        // End egui frame and get tessellated primitives + texture updates
        let (clipped_primitives, textures_delta) = egui_manager.end_frame();

        // Upload any new/changed textures (e.g. font atlas on first frame)
        egui_renderer
            .update_textures(&context, &textures_delta)
            .map_err(|e| e.to_string())?;

        // Prepare egui renderer with tessellated output
        egui_renderer
            .prepare(
                &context,
                clipped_primitives,
                extent.width as f32,
                extent.height as f32,
            )
            .map_err(|e| e.to_string())?;

        // Record commands
        cmd.begin().map_err(|e| e.to_string())?;

        cmd.begin_render_pass(
            swapchain.render_pass(),
            swapchain.framebuffer(image_index),
            extent.width,
            extent.height,
            [0.02, 0.02, 0.02, 1.0], // Dark gray background to match dark theme
        );

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

    unsafe {
        rust_and_vulkan::SDL_StopTextInput(window.window);
    }

    Ok(())
}
