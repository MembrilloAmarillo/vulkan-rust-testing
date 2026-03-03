#version 450
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_scalar_block_layout : require

layout(location = 0) in vec3 v_color;
layout(location = 0) out vec4 outColor;

void main() {
    outColor = vec4(v_color, 1.0);
}