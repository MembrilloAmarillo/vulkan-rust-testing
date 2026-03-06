#version 460

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;

layout(location = 0) out vec4 out_color;

void main() {
    // For now, just output the vertex color
    // TODO: integrate egui texture atlas
    out_color = v_color;
}
