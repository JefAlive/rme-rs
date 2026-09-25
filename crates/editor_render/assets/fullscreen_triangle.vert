// Triângulo em tela cheia para as passadas pixel-art GLSL. Substitui o vertex
// WGSL nessas pipelines para casar a convenção de interpolação do frontend
// GLSL do naga (sampling = None): vertex e fragment vêm do mesmo frontend, as
// entradas/saídas de location 0 coincidem. O RME de referência também tem o
// próprio vertex para os composites (gl_composite_shaders.h compositeVertexSrc).
#version 450
layout(location=0) out vec2 vUV;

void main() {
    vec2 positions[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    vec2 p = positions[gl_VertexIndex];
    gl_Position = vec4(p, 0.0, 1.0);
    // Framebuffer wgpu com origem no topo: inverte V (mesmo que scaler.wgsl).
    vec2 uv = p * 0.5 + vec2(0.5);
    vUV = vec2(uv.x, 1.0 - uv.y);
}