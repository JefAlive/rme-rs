#version 450
// CRT phosphor bloom (custom, portado de compositeCrtBloomSrc).
// Vintage-CRT light diffusion, not a physical P22 simulation and no scanlines.
// Aplicado DEPOIS do upscaling, na resolucao final: a extensao do bloom
// cresce junto com o zoom. O algoritmo permanece o texto da referencia
// (gl_composite_shaders.h, work com o usuario); so dialeto adaptado para
// o frontend GLSL do naga (330 -> 450, texture2D+sampler separados,
// uniforms em bloco std140). Amostragem linear (Filtering) para o halo.
//
// 1. Glow source: chroma-weighted energy computed on a soft-kernel blur that
//    ignores transparent neighbours so empty background never glows.
// 2. Halation instead of additive wash: each channel is blurred with its own
//    radius (blue widest), then recombined with a warm phosphor matrix.
//    White areas stay white instead of clipping.
// 3. Tone shaping: halo compressed (x/(x+k)) rolls highlights off smoothly,
//    then screened (not added) onto the image and graded through a filmic
//    curve (vintage "warm" feel) without touching alpha.
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D SrcTex;
layout(set=0, binding=1) uniform sampler SrcSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 _unused;
} uniforms;
layout(location=0) out vec4 FragColor;

#define RME_GLOW_THRESHOLD 0.46
#define RME_GLOW_SOFTNESS 0.38
#define RME_STRENGTH 0.52
#define RME_RADIUS_R 4.0
#define RME_RADIUS_G 6.0
#define RME_RADIUS_B 8.5
#define TAPS 10

// Phosphor-mix matrix (column-major).
#define RME_P22_R vec3(1.00, 0.14, 0.04)
#define RME_P22_G vec3(0.06, 1.00, 0.10)
#define RME_P22_B vec3(0.05, 0.16, 0.92)

#define RME_GRADE_WARM 0.22
#define RME_GRADE_COOL 0.10
#define RME_GRADE_LIFT 0.018
#define RME_GRADE_GAIN 0.965

const vec2 RME_DIR[TAPS] = vec2[TAPS](
	vec2(1.00, 0.00), vec2(-1.00, 0.00), vec2(0.00, 1.00), vec2(0.00, -1.00), vec2(0.62, 0.62),
	vec2(-0.62, 0.62), vec2(0.62, -0.62), vec2(-0.62, -0.62), vec2(0.38, 0.00), vec2(0.00, 0.38)
);
const float RME_W[TAPS] = float[TAPS](1.00, 1.00, 1.00, 1.00, 0.80, 0.80, 0.80, 0.80, 0.62, 0.62);
const float RME_RING[3] = float[3](0.45, 0.72, 1.0);
const float RME_RING_W[3] = float[3](0.55, 0.78, 1.0);

float rmeGlow(vec3 c)
{
	float l = max(max(c.r, c.g), c.b);
	float chroma = l - min(min(c.r, c.g), c.b);
	return smoothstep(RME_GLOW_THRESHOLD - RME_GLOW_SOFTNESS,
		RME_GLOW_THRESHOLD + RME_GLOW_SOFTNESS,
		l * (1.0 + 0.45 * chroma));
}

void main()
{
	vec2 ps = 1.0 / uniforms.TextureSize;
	vec4 base = texture(sampler2D(SrcTex, SrcSampler), vUV);
	vec3 c = base.rgb;

	// --- glow source ---------------------------------------------------------
	// Weighted average around the pixel; transparent neighbours (alpha 0) are
	// skipped so background does not contribute to the halo.
	vec3 blurred = c * base.a;
	float weight = base.a;
	for (int i = 0; i < TAPS; ++i) {
		vec2 o = RME_DIR[i] * ps;
		vec4 tap = texture(sampler2D(SrcTex, SrcSampler), vUV + o);
		blurred += tap.rgb * tap.a;
		weight += tap.a;
	}
	blurred /= max(weight, 1e-4);

	float energy = rmeGlow(blurred);

	// Scale the halo by how much glowing energy sits around this pixel, so dark
	// pixels next to bright ones receive the bloom (light travels to them).
	float spread = energy;

	// --- per-channel halation -----------------------------------------------
	// Sample AWAY from the pixel (negative offset): the halo at this pixel is
	// built from the light arriving FROM its neighbours, so a bright sprite
	// spreads light onto the darker pixels around it.
	vec3 halo = vec3(0.0);
	float haloW = 0.0;
	for (int i = 0; i < TAPS; ++i) {
		for (int j = 0; j < 3; ++j) {
			float f = RME_RING[j];
			float w = RME_RING_W[j] * RME_W[i];
			vec2 oR = RME_DIR[i] * (RME_RADIUS_R * f * ps);
			vec2 oG = RME_DIR[i] * (RME_RADIUS_G * f * ps);
			vec2 oB = RME_DIR[i] * (RME_RADIUS_B * f * ps);
			halo.r += texture(sampler2D(SrcTex, SrcSampler), vUV - oR).r * w;
			halo.g += texture(sampler2D(SrcTex, SrcSampler), vUV - oG).g * w;
			halo.b += texture(sampler2D(SrcTex, SrcSampler), vUV - oB).b * w;
			haloW += w;
		}
	}
	halo = halo / max(haloW, 1e-4);

	// --- phosphor recombination ----------------------------------------------
	// The halo of a white pixel is white (all channels present), so this matrix
	// only shifts saturated colors; whites stay white, no bleaching.
	vec3 p22 = mat3(RME_P22_R, RME_P22_G, RME_P22_B) * halo;

	// --- tone shaping ---------------------------------------------------------
	// Fade the halo as the pixel approaches white so bright areas bloom without
	// clipping; whites keep their color and the glow reads as light spread.
	float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));
	float headroom = 1.0 - lum;
	float outlineKeep = mix(0.20, 1.0, smoothstep(0.0, 0.30, lum));
	vec3 shaped = p22 * (0.30 + 0.70 * headroom * headroom * headroom) * RME_STRENGTH * spread * outlineKeep;

	// screen blend: out = 1-(1-a)(1-b), keeps whites from blowing out
	vec3 outc = 1.0 - (1.0 - c) * (1.0 - shaped);

	// --- vintage grade (subtle) -----------------------------------------------
	float v = dot(outc, vec3(0.2126, 0.7152, 0.0722));
	outc.r += RME_GRADE_WARM * (1.0 - v) * outc.r;
	outc.b -= RME_GRADE_COOL * (1.0 - v) * outc.b;
	outc = outc * RME_GRADE_GAIN + RME_GRADE_LIFT;

	FragColor = vec4(outc, base.a);
}