#version 450

// Push constant with root pointer (using two 32-bit integers to form 64-bit)
layout(push_constant) uniform RootPointer {
    uint root_ptr_lo;
    uint root_ptr_hi;
} pc;

layout(location = 0) in vec4 v_color;
layout(location = 0) out vec4 out_color;

void main() {
    // Simple color output
    // (GPU pointer dereferencing requires more advanced extensions)
    out_color = v_color;
}