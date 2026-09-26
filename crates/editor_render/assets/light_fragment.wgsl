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

    // Perfil do falloff (radext 1.75x):
    //   cap  = k(0) do OTClient = clamp(intensity * 0.2, 0, 1) — centro fiel.
    //   quad = (1 - dist/radius)² — decai ANTES: sem platô cheio até o meio
    //          do raio (o OTClient linear empacava em k=1 até dist ≈ radius-5).
    //   tail = pow(edge_smooth, 1.2) sobre o último trim — fim muito sutil.
    // Resultado: mesmo raio grande, mas bem menos luz total e cauda fininha.
    let cap = clamp(intensity * 0.2, 0.0, 1.0);
    let radius = intensity * 1.75;
    let t = clamp(dist / radius, 0.0, 1.0);
    let quad = (1.0 - t) * (1.0 - t);

    // Cauda: a zona final (~30% do raio) desce com smoothstep de tangente
    // zero (some o círculo da borda), com mais potência no fim.
    let feather = max(0.9, radius * 0.3);
    let edge = clamp((radius - dist) / feather, 0.0, 1.0);
    let edge_smooth = edge * edge * (3.0 - 2.0 * edge);

    let k = cap * quad * pow(edge_smooth, 1.2);

    let light_contrib = in.color * k;
    return vec4<f32>(light_contrib, 1.0);
}