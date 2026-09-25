#version 450
// MDAPT v2.8 Pass 4 - Merge Dithering and Pseudo Transparency Shader
// by Sp00kyFox, 2014 (GPL, ported from libretro common-shaders).
// Blends pixels based on detected dithering patterns.
// Dialeto adaptado para o frontend GLSL do naga; algoritmo da referencia.
// VL = 0 (vertical lines off), CB = 1 (checkerboard on), DEBUG = 0,
// linear_gamma = 0.
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

#define VL 0.00
#define CB 1.00
#define DEBUG 0.0
#define linear_gamma 0.00
#define TEX(dx,dy)   texture(sampler2D(Source, SourceSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))
#define TEXt0(dx,dy) texture(sampler2D(Original, OriginalSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))

bool eq(vec3 A, vec3 B) { return (A == B); }

float mn2(float a, float b) { return min(a, b); }
float mx2(float a, float b) { return max(a, b); }
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

	vec4 C = TEX(0.0, 0.0);     vec3 c = TEXt0(0.0, 0.0).xyz;
	vec2 L = TEX(-1.0, 0.0).xy; vec3 l = TEXt0(-1.0, 0.0).xyz;
	vec2 R = TEX(1.0, 0.0).xy;  vec3 r = TEXt0(1.0, 0.0).xyz;
	vec2 U = TEX(0.0, -1.0).xy;
	vec2 D = TEX(0.0, 1.0).xy;

	float prVL = 0.0, prCB = 0.0;
	vec3 fVL = vec3(0.0), fCB = vec3(0.0);

	// Backpropagation
	C.xy = mx2(C.xy, mn2(C.zw, mx4(L.xy, R.xy, U.xy, D.xy)));

	if (VL > 0.5) {
		float prSum = L.x + R.x;

		prVL = mx2(L.x, R.x);
		prVL = (prVL == 0.0) ? 1.0 : prSum / prVL;

		fVL = (prVL * c + L.x * l + R.x * r) / (prVL + prSum);
		prVL = C.x;
	}

	if (CB > 0.5) {
		vec3 u = TEXt0(0.0, -1.0).xyz;
		vec3 d = TEXt0(0.0, 1.0).xyz;

		float eqCL = (eq(c, l)) ? 1.0 : 0.0;
		float eqCR = (eq(c, r)) ? 1.0 : 0.0;
		float eqCU = (eq(c, u)) ? 1.0 : 0.0;
		float eqCD = (eq(c, d)) ? 1.0 : 0.0;

		float prU = mx2(U.y, eqCU);
		float prD = mx2(D.y, eqCD);
		float prL = mx2(L.y, eqCL);
		float prR = mx2(R.y, eqCR);

		float prSum = prU + prD + prL + prR;

		prCB = mx2(prL, mx2(prR, mx2(prU, prD)));
		prCB = (prCB == 0.0) ? 1.0 : prSum / prCB;

		//standard formula: C/2 + (L + R + D + U)/8
		fCB = (prCB * c + prU * u + prD * d + prL * l + prR * r) / (prCB + prSum);

		float UL = TEX(-1.0, -1.0).y; vec3 ul = TEXt0(-1.0, -1.0).xyz;
		float UR = TEX(1.0, -1.0).y;  vec3 ur = TEXt0(1.0, -1.0).xyz;
		float DL = TEX(-1.0, 1.0).y;  vec3 dl = TEXt0(-1.0, 1.0).xyz;
		float DR = TEX(1.0, 1.0).y;   vec3 dr = TEXt0(1.0, 1.0).xyz;

		// Checkerboard Smoothing
		prCB = mx9(C.y,
			mn2(L.y, eqCL),
			mn2(R.y, eqCR),
			mn2(U.y, eqCU),
			mn2(D.y, eqCD),
			mn2(UL, (eq(c, ul)) ? 1.0 : 0.0),
			mn2(UR, (eq(c, ur)) ? 1.0 : 0.0),
			mn2(DL, (eq(c, dl)) ? 1.0 : 0.0),
			mn2(DR, (eq(c, dr)) ? 1.0 : 0.0));
	}

	if (DEBUG > 0.5)
		FragColor = vec4(prVL, prCB, 0.0, 0.0);

	vec4 final = (prCB >= prVL) ? vec4(mix(c, fCB, prCB), 1.0) : vec4(mix(c, fVL, prVL), 1.0);
	FragColor = (linear_gamma > 0.5) ? pow(final, vec4(1.0 / 2.2)) : final;
}