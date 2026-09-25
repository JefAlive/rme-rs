#version 450
// MDAPT v2.8 Pass 0 - Merge Dithering and Pseudo Transparency Shader
// by Sp00kyFox, 2014 (GPL, ported from libretro common-shaders).
// Neighbor analysis via color metric and dot product of the difference vectors.
// O texto do algoritmo permanece o da referencia; so o dialeto foi adaptado
// para o frontend GLSL do naga (330 -> 450, texture2D+sampler separados,
// uniforms em bloco std140). Dado derivado de work/gpl:
//   MODE = 0 (analise colorida), PWR = 2.0.
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D Source;
layout(set=0, binding=1) uniform sampler SourceSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 _unused;
} uniforms;
layout(location=0) out vec4 FragColor;

#define PWR 2.0
#define TEX(dx,dy) texture(sampler2D(Source, SourceSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))

#define dotfix(x,y) clamp(dot(x,y), 0.0, 1.0)	// NVIDIA Fix

// Reference: http://www.compuphase.com/cmetric.htm
float eq(vec3 A, vec3 B)
{
	vec3 diff = A - B;
	float ravg = (A.x + B.x) * 0.5;

	diff *= diff * vec3(2.0 + ravg, 4.0, 3.0 - ravg);

	return pow(smoothstep(3.0, 0.0, sqrt(diff.x + diff.y + diff.z)), PWR);
}

float and6(float a, float b, float c, float d, float e, float f)
{
	return min(a, min(b, min(c, min(d, min(e, f)))));
}

void main()
{
	/*
		  U
		L C R
		  D
	*/

	vec3 C = TEX(0.0, 0.0).xyz;
	vec3 L = TEX(-1.0, 0.0).xyz;
	vec3 R = TEX(1.0, 0.0).xyz;
	vec3 U = TEX(0.0, -1.0).xyz;
	vec3 D = TEX(0.0, 1.0).xyz;

	vec3 res = vec3(0.0);

	{
		vec3 dCL = normalize(C - L), dCR = normalize(C - R), dCD = normalize(C - D), dCU = normalize(C - U);

		res.x = dotfix(dCL, dCR) * eq(L, R);
		res.y = dotfix(dCU, dCD) * eq(U, D);
		res.z = and6(res.x, res.y, dotfix(dCL, dCU) * eq(L, U), dotfix(dCL, dCD) * eq(L, D), dotfix(dCR, dCU) * eq(R, U), dotfix(dCR, dCD) * eq(R, D));
	}

	FragColor = vec4(res, 1.0);
}