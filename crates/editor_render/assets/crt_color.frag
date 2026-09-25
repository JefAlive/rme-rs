#version 450
// CRT colour: gama de fosforo SMPTE-C/Rec.601 aplicada ao RGB final
// (toggle "crt color").
//
// Portado do grade.glsl (Dogway / Jose Linares, GPLv2+; libretro/glsl-shaders).
// Replica somente a troca cromatica de gamut: primarias SMPTE170M_ph
// (SMPTE-C, o "P22" dos CRTs de consumo - Conrac & RCA, 1969) -> sRGB, via o
// framework RGB_to_XYZ do autor (RW = branco D65), col = lin * G,
// G = M_SMPTE * inverse(M_sRGB). Sem saturação extra.
//
// Efeito nas primarias puras (em linear):
//  * vermelho (1,0,0) -> (0.94, 0.018, 0):   leve dessaturada e esquenta
//    pro laranja;
//  * azul     (0,0,1) -> (0.010, 0.016, 1.006): leve dessaturada, puxa pro
//    ciano;
//  * verde    (0,1,0) -> (0.050, 0.966, 0):   leve dessaturada, oliva;
//  * branco preservado ((1,1,1) -> (1,1,1)).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D Source;
layout(set=0, binding=1) uniform sampler SourceSampler;
layout(location=0) out vec4 FragColor;

// col = lin * GAMUT (linha-vetor, transposto para o ctor coluna-major)
const mat3 GAMUT = mat3(
	0.9395, 0.0502, 0.0103,
	0.0178, 0.9658, 0.0164,
	-0.0016, -0.0044, 1.006
);

void main()
{
	vec4 base = texture(sampler2D(Source, SourceSampler), vUV);
	vec3 lin = pow(base.rgb, vec3(2.2));
	vec3 tint = clamp(lin * GAMUT, vec3(0.0), vec3(1.0));
	FragColor = vec4(pow(tint, vec3(1.0 / 2.2)), base.a);
}