use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    const BINDGEN_CLANG_ENV_VARS: &[&str] = &[
        "BINDGEN_EXTRA_CLANG_ARGS",
        "BINDGEN_EXTRA_CLANG_ARGS_x86_64-pc-windows-msvc",
        "BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_msvc",
    ];
    const SHADERS: &[(&str, &str, &str)] = &[
        (
            "shaders/simple_square.vert.glsl",
            "shaders/simple_square.vert.spv",
            "vert",
        ),
        (
            "shaders/simple_square.frag.glsl",
            "shaders/simple_square.frag.spv",
            "frag",
        ),
        ("shaders/ui.vert", "shaders/ui.vert.spv", "vert"),
        ("shaders/ui.frag", "shaders/ui.frag.spv", "frag"),
    ];

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=VULKAN_SDK");
    println!("cargo:rerun-if-env-changed=SDL_LIB");
    println!("cargo:rerun-if-env-changed=SDL_INCLUDE");
    println!("cargo:rerun-if-env-changed=SDL3_DIR");
    println!("cargo:rerun-if-env-changed=GLSLANG_VALIDATOR");
    for env_var in BINDGEN_CLANG_ENV_VARS {
        println!("cargo:rerun-if-env-changed={env_var}");
    }
    for (src, _, _) in SHADERS {
        println!("cargo:rerun-if-changed={src}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| String::from("windows"));

    println!("cargo:rustc-link-lib=SDL3");
    println!("cargo:rustc-link-lib={}", vulkan_link_lib_name(&target_os));

    let mut link_search_paths = BTreeSet::new();
    add_path_from_env(&mut link_search_paths, "SDL_LIB", None);
    add_path_from_env(&mut link_search_paths, "SDL3_DIR", Some("lib"));
    add_path_from_env(
        &mut link_search_paths,
        "VULKAN_SDK",
        Some(default_vulkan_lib_subdir(&target_os)),
    );
    add_fallback_link_paths(&mut link_search_paths, &target_os);
    emit_link_search_paths(&link_search_paths);

    for (src, dst, stage) in SHADERS {
        compile_glsl_to_spirv(src, dst, stage);
    }

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .rustified_enum(".*");

    for name in BINDGEN_CLANG_ENV_VARS {
        if let Ok(value) = env::var(name) {
            for arg in value.split_whitespace() {
                builder = builder.clang_arg(arg);
            }
        }
    }

    if let Ok(vulkan_sdk) = env::var("VULKAN_SDK") {
        builder = builder.clang_arg(format!(
            "-I{}",
            PathBuf::from(vulkan_sdk)
                .join(default_vulkan_include_subdir(&target_os))
                .display()
        ));
    }
    if let Ok(sdl3_dir) = env::var("SDL3_DIR") {
        builder = builder.clang_arg(format!(
            "-I{}",
            PathBuf::from(sdl3_dir)
                .join(default_include_subdir(&target_os))
                .display()
        ));
    }

    let mut include_paths = BTreeSet::new();
    if let Ok(sdl_include) = env::var("SDL_INCLUDE") {
        include_paths.insert(PathBuf::from(sdl_include));
    }
    add_fallback_include_paths(&mut include_paths, &target_os);
    for include_dir in include_paths {
        builder = add_clang_include_if_exists(builder, include_dir);
    }

    let bindings = builder.generate().expect("Unable to generate bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set by Cargo"));
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings");
}

fn compile_glsl_to_spirv(src: &str, dst: &str, stage: &str) {
    let src_path = Path::new(src);
    if !src_path.exists() {
        panic!("Shader source does not exist: {src}");
    }

    if let Some(parent) = Path::new(dst).parent() {
        std::fs::create_dir_all(parent).expect("Failed to create shader output directory");
    }

    let glslang = env::var("GLSLANG_VALIDATOR").unwrap_or_else(|_| "glslangValidator".to_string());
    let status = Command::new(&glslang)
        .args(["-V", "-S", stage, src, "-o", dst])
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to execute shader compiler '{glslang}'.\n\
                 Make sure glslangValidator is installed, or set GLSLANG_VALIDATOR.\n\
                 Command: {glslang} -V -S {stage} {src} -o {dst}\n\
                 Error: {e}"
            )
        });

    if !status.success() {
        panic!(
            "glslangValidator failed for shader {src} (stage {stage}).\n\
             Command: {glslang} -V -S {stage} {src} -o {dst}"
        );
    }
}

fn vulkan_link_lib_name(target_os: &str) -> &'static str {
    match target_os {
        "windows" => "vulkan-1",
        _ => "vulkan",
    }
}

fn add_path_from_env(paths: &mut BTreeSet<PathBuf>, env_var: &str, suffix: Option<&str>) {
    if let Ok(value) = env::var(env_var) {
        let path = match suffix {
            Some(sfx) => PathBuf::from(value).join(sfx),
            None => PathBuf::from(value),
        };
        paths.insert(path);
    }
}

fn emit_link_search_paths(paths: &BTreeSet<PathBuf>) {
    for path in paths {
        if path.exists() {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    }
}

fn add_clang_include_if_exists(builder: bindgen::Builder, include_dir: PathBuf) -> bindgen::Builder {
    if include_dir.exists() {
        builder.clang_arg(format!("-I{}", include_dir.display()))
    } else {
        builder
    }
}

fn default_include_subdir(target_os: &str) -> &'static str {
    match target_os {
        "windows" => "include",
        _ => "include",
    }
}

fn default_vulkan_lib_subdir(target_os: &str) -> &'static str {
    match target_os {
        "windows" => "Lib",
        _ => "lib",
    }
}

fn default_vulkan_include_subdir(target_os: &str) -> &'static str {
    match target_os {
        "windows" => "Include",
        _ => "include",
    }
}

fn add_fallback_link_paths(paths: &mut BTreeSet<PathBuf>, target_os: &str) {
    match target_os {
        "windows" => {
            if let Some(path) = newest_windows_vulkan_sdk_subdir("Lib") {
                paths.insert(path);
            }
            paths.insert(PathBuf::from(r"C:\devel\base\code\third-party\SDL\lib"));
        }
        "linux" => {
            paths.insert(PathBuf::from("/usr/lib"));
            paths.insert(PathBuf::from("/usr/lib64"));
            paths.insert(PathBuf::from("/usr/local/lib"));
        }
        "macos" => {
            paths.insert(PathBuf::from("/opt/homebrew/lib"));
            paths.insert(PathBuf::from("/usr/local/lib"));
        }
        _ => {}
    }
}

fn add_fallback_include_paths(paths: &mut BTreeSet<PathBuf>, target_os: &str) {
    match target_os {
        "windows" => {
            if let Some(path) = newest_windows_vulkan_sdk_subdir("Include") {
                paths.insert(path);
            }
            paths.insert(PathBuf::from(r"C:\devel\base\code\third-party\SDL\include"));
        }
        "linux" => {
            paths.insert(PathBuf::from("/usr/include"));
            paths.insert(PathBuf::from("/usr/local/include"));
        }
        "macos" => {
            paths.insert(PathBuf::from("/opt/homebrew/include"));
            paths.insert(PathBuf::from("/usr/local/include"));
        }
        _ => {}
    }
}

fn newest_windows_vulkan_sdk_subdir(subdir: &str) -> Option<PathBuf> {
    let root = Path::new(r"C:\VulkanSDK");
    let entries = std::fs::read_dir(root).ok()?;

    let mut best: Option<String> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if best.as_ref().is_none_or(|current| version_key(&name) > version_key(current)) {
            best = Some(name);
        }
    }

    best.map(|version| root.join(version).join(subdir))
}

fn version_key(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}
