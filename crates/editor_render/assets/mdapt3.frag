#version 450
// MDAPT v2.8 Pass 3 - Merge Dithering and Pseudo Transparency Shader
// by Sp00kyFox, 2014 (GPL, ported from libretro common-shaders).
// Backpropagation and checkerboard smoothing.
// Dialeto adaptado para o frontend GLSL do naga; algoritmo da referencia.
// Lê o alvo (Source) e a cena original (Original).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D Source;
layout(set=0, binding=1) uniform sampler SourceSampler;
layout(set=0, binding=2) uniform texture2D Original;
layout(set=0, binding=3) uniform sampler OriginalSampler;
layout(set=0, binding=4) uniform Uniforms {
	vec2 TextureSize;
	vec2 _unused;
} uniforms;
layout(location=0) out vec4 FragColor;

#define TEX(dx,dy)   texture(sampler2D(Source, SourceSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))
#define TEXt0(dx,dy) texture(sampler2D(Original, OriginalSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))

bool eq(vec3 A, vec3 B) { return (A == B); }

float mn2(float a, float b) { return min(a, b); }
float mx9(float a, float b, float c, float d, float e, float f, float g, float h, float i)
{
	return max(a, max(b, max(c, max(d, max(e, max(f, max(g, max(h, i))))))));
}
vec2 mn2(vec2 a, vec2 b) { return min(a, b); }
vec2 mx2(vec2 a, vec2 b) { return max(a, b); }
vec2 mx4(vec2 a, vec2 b, vec2 c, vec2 d) { return max(a, max(b, max(c, d))); }

void main()
{
	/*
		UL U UR
		L  C R
		DL D DR
	*/

	vec4 C = TEX(0.0, 0.0);       vec3 c = TEXt0(0.0, 0.0).xyz;
	vec2 L = TEX(-1.0, 0.0).xy;   vec3 l = TEXt0(-1.0, 0.0).xyz;
	vec2 R = TEX(1.0, 0.0).xy;    vec3 r = TEXt0(1.0, 0.0).xyz;
	vec2 U = TEX(0.0, -1.0).xy;   vec3 u = TEXt0(0.0, -1.0).xyz;
	vec2 D = TEX(0.0, 1.0).xy;    vec3 d = TEXt0(0.0, 1.0).xyz;
	float UL = TEX(-1.0, -1.0).y; vec3 ul = TEXt0(-1.0, -1.0).xyz;
	float UR = TEX(1.0, -1.0).y;  vec3 ur = TEXt0(1.0, -1.0).xyz;
	float DL = TEX(-1.0, 1.0).y;  vec3 dl = TEXt0(-1.0, 1.0).xyz;
	float DR = TEX(1.0, 1.0).y;   vec3 dr = TEXt0(1.0, 1.0).xyz;

	// Backpropagation
	C.xy = mx2(C.xy, mn2(C.zw, mx4(L, R, U, D)));

	// Checkerboard Smoothing
	C.y = mx9(C.y,
		mn2(U.y, (eq(c, u)) ? 1.0 : 0.0),
		mn2(D.y, (eq(c, d)) ? 1.0 : 0.0),
		mn2(L.y, (eq(c, l)) ? 1.0 : 0.0),
		mn2(R.y, (eq(c, r)) ? 1.0 : 0.0),
		mn2(UL, (eq(c, ul)) ? 1.0 : 0.0),
		mn2(UR, (eq(c, ur)) ? 1.0 : 0.0),
		mn2(DL, (eq(c, dl)) ? 1.0 : 0.0),
		mn2(DR, (eq(c, dr)) ? 1.0 : 0.0));

	FragColor = vec4(C);
}