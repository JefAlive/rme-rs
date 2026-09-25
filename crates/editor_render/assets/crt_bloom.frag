#version 450
// CRT phosphor halation (rework): halo cromatico de fosforo sobre a cena
// FINAL ja iluminada — desacoplado do World Light. Sem recuperacao de brilho
// (invWl) e sem boost de noite: a forca e constante e ajustavel (Params.x);
// raio por canal com escala (Params.y).
//
// FISICA DO CRT:
//  * raio / espalhamento por canal: azul espalha mais (450nm) -> R4 / G6 / B8.5;
//  * halos amostram a cena final (o lampiao ja vem totalmente aceso pelo
//    light buffer — nao multiplicamos rescate de brilho);
//  * gate por luminancia do HALO: so ha bleed onde ha vizinhos brilhantes;
//  * composicao SCREEN (1-(1-base)(1-glow)): nunca passa de 1.0;
//    pretos recebem bleed fino do vizinho claro; brancos ficam intactos
//    (nao lava a cena clara do dia).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D SrcTex;
layout(set=0, binding=1) uniform sampler SrcSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 Params; // x = strength (0..~0.4 via slider), y = radius scale (1.0)
} uniforms;
layout(location=0) out vec4 FragColor;

#define RME_GLOW_THRESH 0.30
#define RME_GLOW_SOFT 0.28
#define RME_RADIUS_R 4.0
#define RME_RADIUS_G 6.0
#define RME_RADIUS_B 8.5
#define TAPS 10
#define RINGS 3

// Fatores dos anéis: 3 anéis com peso decrescente (cauda suave).
const float RME_RING[RINGS] = float[RINGS](0.50, 0.85, 1.30);
const float RME_RING_W[RINGS] = float[RINGS](1.00, 0.60, 0.30);

const vec2 RME_DIR[TAPS] = vec2[TAPS](
	vec2(1.00, 0.00), vec2(-1.00, 0.00), vec2(0.00, 1.00), vec2(0.00, -1.00), vec2(0.62, 0.62),
	vec2(-0.62, 0.62), vec2(0.62, -0.62), vec2(-0.62, -0.62), vec2(0.38, 0.00), vec2(0.00, 0.38)
);
const float RME_W[TAPS] = float[TAPS](
	1.00, 1.00, 1.00, 1.00, 0.80, 0.80, 0.80, 0.80, 0.62, 0.62
);

void main()
{
	vec2 ps = 1.0 / uniforms.TextureSize;
	vec4 base = texture(sampler2D(SrcTex, SrcSampler), vUV);
	vec3 src = base.rgb;

	float rs = max(uniforms.Params.y, 0.1);

	// --- halation por canal (raios fracionarios, sampler linear) ------------
	vec3 halo = vec3(0.0);
	float haloW = 0.0;
	for (int i = 0; i < TAPS; ++i) {
		for (int j = 0; j < RINGS; ++j) {
			float f = RME_RING[j];
			float w = RME_RING_W[j] * RME_W[i];
			vec2 oR = RME_DIR[i] * (RME_RADIUS_R * f * rs * ps);
			vec2 oG = RME_DIR[i] * (RME_RADIUS_G * f * rs * ps);
			vec2 oB = RME_DIR[i] * (RME_RADIUS_B * f * rs * ps);
			halo.r += texture(sampler2D(SrcTex, SrcSampler), vUV - oR).r * w;
			halo.g += texture(sampler2D(SrcTex, SrcSampler), vUV - oG).g * w;
			halo.b += texture(sampler2D(SrcTex, SrcSampler), vUV - oB).b * w;
			haloW += w;
		}
	}
	halo = halo / max(haloW, 1e-4);

	// Gate por luminancia do halo: somente regioes com vizinhos brilhantes
	// geram bleed (cena de dia muito clara nao halate por tudo).
	float energy = smoothstep(RME_GLOW_THRESH - RME_GLOW_SOFT,
		RME_GLOW_THRESH + RME_GLOW_SOFT,
		max(halo.r, max(halo.g, halo.b)));

	float strength = clamp(uniforms.Params.x, 0.0, 0.5);
	vec3 glow = halo * energy * strength;

	// Screen blend: noite = bleed fino sobre escuro (sem estourar); dia =
	// brancos intactos (nao lava a cena).
	vec3 outc = 1.0 - (vec3(1.0) - src) * (vec3(1.0) - glow);
	FragColor = vec4(outc, base.a);
}