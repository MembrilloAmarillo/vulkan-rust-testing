use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    // Rebuild if shaders change
    println!("cargo:rerun-if-changed=shaders/simple_square.vert.glsl");
    println!("cargo:rerun-if-changed=shaders/simple_square.frag.glsl");

    // Tell cargo to look for shared libraries in the system
    println!("cargo:rustc-link-lib=SDL3");
    println!("cargo:rustc-link-lib=vulkan");

    // Compile the rotating-square shaders to SPIR-V using glslangValidator.
    // This writes the .spv files into the repository's shaders/ directory
    // because `src/main.rs` loads them from there at runtime.
    compile_glsl_to_spirv(
        "shaders/simple_square.vert.glsl",
        "shaders/simple_square.vert.spv",
        "vert",
    );
    compile_glsl_to_spirv(
        "shaders/simple_square.frag.glsl",
        "shaders/simple_square.frag.spv",
        "frag",
    );

    compile_glsl_to_spirv("shaders/ui.vert.glsl", "shaders/ui.vert.spv", "vert");
    compile_glsl_to_spirv("shaders/ui.frag.glsl", "shaders/ui.frag.spv", "frag");

    // Generate Vulkan bindings
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .rustified_enum(".*")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

fn compile_glsl_to_spirv(src: &str, dst: &str, stage: &str) {
    // Ensure parent dir exists (no-op if it already exists).
    if let Some(parent) = Path::new(dst).parent() {
        std::fs::create_dir_all(parent).expect("Failed to create shader output directory");
    }

    let status = Command::new("glslangValidator")
        .args(["-V", "-S", stage, src, "-o", dst])
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to execute glslangValidator. Make sure it is installed and on PATH.\n\
                 Command: glslangValidator -V -S {stage} {src} -o {dst}\n\
                 Error: {e}"
            )
        });

    if !status.success() {
        panic!(
            "glslangValidator failed for shader {src} (stage {stage}).\n\
             Command: glslangValidator -V -S {stage} {src} -o {dst}"
        );
    }
}
