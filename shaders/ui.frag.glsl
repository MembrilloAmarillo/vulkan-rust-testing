#version 460

layout(binding = 0) uniform sampler2D texture_atlas;

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;

layout(location = 0) out vec4 out_color;

void main() {
    vec4 atlas_sample = texture(texture_atlas, v_uv);
    // egui texture atlas is grayscale (in red channel), multiply by vertex color
    out_color = v_color * atlas_sample.r;
}
