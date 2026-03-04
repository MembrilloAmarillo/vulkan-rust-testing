#version 450

// Push constant with root pointer (using two 32-bit integers to form 64-bit)
layout(push_constant) uniform RootPointer {
    uint root_ptr_lo;
    uint root_ptr_hi;
} pc;

// Vertex outputs
layout(location = 0) out vec4 v_color;

void main() {
    // Hardcoded triangle positions for now
    // (GPU pointer dereferencing requires more advanced extensions)
    vec2 positions[3] = vec2[](
        vec2(-0.5, -0.5),
        vec2(0.5, -0.5),
        vec2(0.0, 0.5)
    );
    
    gl_Position = vec4(positions[gl_VertexIndex], 0.0, 1.0);
    v_color = vec4(1.0, 0.5, 0.25, 1.0); // Orange color
}