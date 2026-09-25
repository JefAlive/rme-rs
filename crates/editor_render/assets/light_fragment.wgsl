// Light fragment shader (WGSL) — contribuição da luz com falloff OTClient
// (lightview.cpp:updatePixels). O light buffer é limpo com a cor ambiente
// (método Tibia) e cada fonte adiciona o seu máximo por canal via blend Max.

struct VsOut {
    @location(0) light_pos: vec2<f32>,
    @location(1) frag_world_pos: vec2<f32>,
    @location(2) intensity: f32,
    @location(3) color: vec3<f32>,
};

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let intensity = in.intensity;
    if intensity <= 0.0 {
        discard;
    }

    // Distância do fragmento ao centro da luz, em tiles (OTClient mede em
    // pixels e divide por tileSize; aqui o mundo já está em tiles fracionários).
    let dist = distance(in.frag_world_pos, in.light_pos);

    // Falloff OTClient: k = clamp((-dist + intensity) * 0.2, 0, 1).
    // Centro (dist=0): k = intensity * 0.2; cap 1.0 em intensity >= 5
    // (luzes menores nunca atingem branco total — fiel ao LightView).
    let falloff = (intensity - dist) * 0.2;
    let k = clamp(falloff, 0.0, 1.0);

    let light_contrib = in.color * k;
    return vec4<f32>(light_contrib, 1.0);
}