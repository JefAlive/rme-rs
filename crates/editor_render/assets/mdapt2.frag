#version 450
// MDAPT v2.8 Pass 2 - Merge Dithering and Pseudo Transparency Shader
// by Sp00kyFox, 2014 (GPL, ported from libretro common-shaders).
// Eliminating isolated detections.
// Dialeto adaptado para o frontend GLSL do naga; algoritmo da referencia.
// VL_LO 1.25 VL_HI 1.75 CB_LO 5.25 CB_HI 5.75 (padroes do libretro).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D Source;
layout(set=0, binding=1) uniform sampler SourceSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 _unused;
} uniforms;
layout(location=0) out vec4 FragColor;

#define VL_LO 1.25
#define VL_HI 1.75
#define CB_LO 5.25
#define CB_HI 5.75
#define TEX(dx,dy) texture(sampler2D(Source, SourceSampler), vUV + vec2((dx),(dy)) * (1.0 / uniforms.TextureSize))
#define andv(x,y) min(x,y)
#define orv(x,y)  max(x,y)

vec2 sigmoid(vec2 signal)
{
	return smoothstep(vec2(VL_LO, CB_LO), vec2(VL_HI, CB_HI), signal);
}

void main()
{
	/*
		NW  UUL U2 UUR NE
		ULL UL  U1 UR  URR
		L2  L1  C  R1  R2
		DLL DL  D1 DR  DRR
		SW  DDL D2 DDR SE
	*/

	vec2 C = TEX(0.0, 0.0).xy;

	vec2 hits = vec2(0.0);

	//phase 1
	vec2 L1 = TEX(-1.0, 0.0).xy;
	vec2 R1 = TEX(1.0, 0.0).xy;
	vec2 U1 = TEX(0.0, -1.0).xy;
	vec2 D1 = TEX(0.0, 1.0).xy;

	//phase 2
	vec2 L2 = andv(TEX(-2.0, 0.0).xy, L1);
	vec2 R2 = andv(TEX(2.0, 0.0).xy, R1);
	vec2 U2 = andv(TEX(0.0, -2.0).xy, U1);
	vec2 D2 = andv(TEX(0.0, 2.0).xy, D1);
	vec2 UL = andv(TEX(-1.0, -1.0).xy, orv(L1, U1));
	vec2 UR = andv(TEX(1.0, -1.0).xy, orv(R1, U1));
	vec2 DL = andv(TEX(-1.0, 1.0).xy, orv(L1, D1));
	vec2 DR = andv(TEX(1.0, 1.0).xy, orv(R1, D1));

	//phase 3
	vec2 ULL = andv(TEX(-2.0, -1.0).xy, orv(L2, UL));
	vec2 URR = andv(TEX(2.0, -1.0).xy, orv(R2, UR));
	vec2 DRR = andv(TEX(2.0, 1.0).xy, orv(R2, DR));
	vec2 DLL = andv(TEX(-2.0, 1.0).xy, orv(L2, DL));
	vec2 UUL = andv(TEX(-1.0, -2.0).xy, orv(U2, UL));
	vec2 UUR = andv(TEX(1.0, -2.0).xy, orv(U2, UR));
	vec2 DDR = andv(TEX(1.0, 2.0).xy, orv(D2, DR));
	vec2 DDL = andv(TEX(-1.0, 2.0).xy, orv(D2, DL));

	//phase 4
	hits += andv(TEX(-2.0, -2.0).xy, orv(UUL, ULL));
	hits += andv(TEX(2.0, -2.0).xy, orv(UUR, URR));
	hits += andv(TEX(-2.0, 2.0).xy, orv(DDL, DLL));
	hits += andv(TEX(2.0, 2.0).xy, orv(DDR, DRR));

	hits += (ULL + URR + DRR + DLL + L2 + R2) + vec2(0.0, 1.0) * (C + U1 + U2 + D1 + D2 + L1 + R1 + UL + UR + DL + DR + UUL + UUR + DDR + DDL);

	FragColor = vec4(C * sigmoid(hits), C);
}