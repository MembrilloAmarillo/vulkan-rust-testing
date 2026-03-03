#version 450
#extension GL_EXT_buffer_reference : require
#extension GL_EXT_scalar_block_layout : require

struct Uniforms {
    mat4 mvp;
};

layout(buffer_reference, scalar) buffer UniformsBlock {
    Uniforms uniforms;
};

layout(push_constant) uniform PushConstants {
    UniformsBlock ptr;
} push;

layout(location = 0) out vec3 v_color;

void main() {
    // Cube vertices (positions and colors)
    vec3 positions[8] = vec3[](
        vec3(-0.5, -0.5, -0.5),
        vec3( 0.5, -0.5, -0.5),
        vec3( 0.5,  0.5, -0.5),
        vec3(-0.5,  0.5, -0.5),
        vec3(-0.5, -0.5,  0.5),
        vec3( 0.5, -0.5,  0.5),
        vec3( 0.5,  0.5,  0.5),
        vec3(-0.5,  0.5,  0.5)
    );

    vec3 colors[8] = vec3[](
        vec3(1.0, 0.0, 0.0), // red
        vec3(0.0, 1.0, 0.0), // green
        vec3(0.0, 0.0, 1.0), // blue
        vec3(1.0, 1.0, 0.0), // yellow
        vec3(1.0, 0.0, 1.0), // magenta
        vec3(0.0, 1.0, 1.0), // cyan
        vec3(0.5, 0.5, 0.5), // gray
        vec3(1.0, 1.0, 1.0)  // white
    );

    // Cube indices (12 triangles = 36 vertices)
    int indices[36] = int[](
        // front
        0, 1, 2, 2, 3, 0,
        // right
        1, 5, 6, 6, 2, 1,
        // back
        5, 4, 7, 7, 6, 5,
        // left
        4, 0, 3, 3, 7, 4,
        // bottom
        0, 4, 5, 5, 1, 0,
        // top
        3, 2, 6, 6, 7, 3
    );

    int vertex_index = indices[gl_VertexIndex];
    vec3 position = positions[vertex_index];
    v_color = colors[vertex_index];
    
    gl_Position = push.ptr.uniforms.mvp * vec4(position, 1.0);
}