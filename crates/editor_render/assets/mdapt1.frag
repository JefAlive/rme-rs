#version 450
// MDAPT v2.8 Pass 1 - Merge Dithering and Pseudo Transparency Shader
// by Sp00kyFox, 2014 (GPL, ported from libretro common-shaders).
// Preparing checkerboard patterns.
// Dialeto adaptado para o frontend GLSL do naga; algoritmo da referencia.
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D Source;
layout(set=0, binding=1) uniform sampler SourceSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 _unused;
} uniforms;
layout(location=0) out vec4 FragColor;

#define TEX(dx,dy) texture(sampler2D(Source, SourceSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))

float mn2(float a, float b) { return min(a, b); }
float mn3(float a, float b, float c) { return min(a, min(b, c)); }
float mx2(float a, float b) { return max(a, b); }
float mx5(float a, float b, float c, float d, float e) { return max(a, max(b, max(c, max(d, e)))); }

void main()
{
	/*
		UL U UR
		L  C R
		DL D DR
	*/

	vec3 C = TEX(0.0, 0.0).xyz;
	vec3 L = TEX(-1.0, 0.0).xyz;
	vec3 R = TEX(1.0, 0.0).xyz;
	vec3 D = TEX(0.0, 1.0).xyz;
	vec3 U = TEX(0.0, -1.0).xyz;

	float UL = TEX(-1.0, -1.0).z;
	float UR = TEX(1.0, -1.0).z;
	float DL = TEX(-1.0, 1.0).z;
	float DR = TEX(1.0, 1.0).z;

	// Checkerboard Pattern Completion
	float c1 = mx2(mn2(UL, UR), mn2(DL, DR));
	float c2 = mx2(mn2(UL, DL), mn2(UR, DR));
	float prCB = mx5(C.z,
		mn3(L.z, R.z, mx2(U.x, D.x)),
		mn3(U.z, D.z, mx2(L.y, R.y)),
		mn2(C.x, c1),
		mn2(C.y, c2));
	FragColor = vec4(C.x, prCB, 0.0, 0.0);
}