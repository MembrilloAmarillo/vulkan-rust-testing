#version 460

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec2 v_uv;
layout(location = 2) flat in uint v_texture_index;
layout(location = 3) flat in uint v_material_mode;
layout(location = 0) out vec4 outColor;

// Texture array binding
layout(set = 0, binding = 0) uniform sampler2D u_textures[3];

float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

float value_noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);

    float a = hash12(i + vec2(0.0, 0.0));
    float b = hash12(i + vec2(1.0, 0.0));
    float c = hash12(i + vec2(0.0, 1.0));
    float d = hash12(i + vec2(1.0, 1.0));

    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

float fbm(vec2 p) {
    float sum = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 5; ++i) {
        sum += amp * value_noise(p);
        p *= 2.03;
        amp *= 0.5;
    }
    return sum;
}

void main() {
    vec3 N = normalize(v_normal);
    vec3 L = normalize(vec3(0.35, 0.7, 0.55));
    float lambert = max(dot(N, L), 0.15);

    // Sample the appropriate texture based on texture_index
    uint tex_idx = min(v_texture_index, 2u);
    vec4 sampled_tex = texture(u_textures[tex_idx], v_uv);

    // Mix sampled texture color with procedural noise for variety
    float n = fbm(v_uv * 8.0 + float(v_texture_index) * 1.9);

    vec3 material_color;
    if (v_material_mode == 0u) {
        material_color = mix(sampled_tex.rgb, sampled_tex.rgb * 1.2, n * 0.3);
    } else if (v_material_mode == 1u) {
        material_color = mix(sampled_tex.rgb * 0.8, sampled_tex.rgb, n);
    } else {
        float stripes = step(0.5, fract((v_uv.x + v_uv.y) * 10.0));
        material_color = mix(sampled_tex.rgb, sampled_tex.rgb * vec3(1.5, 0.9, 0.5), stripes * 0.4);
    }

    outColor = vec4(material_color * lambert, 1.0);
}
