#version 450
// CRT color (CRT COMUM / fosforo de tubo barato): matriz de IMPUREZA no
// lugar do gamut fiel SMPTE-C. Um tubo comum tinha crosstalk entre os
// fosforos (sangramento entre canais) e pureza menor (- cores lavadas), na
// direcao oposta ao Trinitron vivo/saturado.
//
// Matriz (em linear) aplicada aos primarios puros:
//   vermelho (1,0,0) -> (0.90, 0.05, 0.02)   vermelho com crosstalk leve
//                                            (dessaturacao sutil, menos lavado)
//   verde    (0,1,0) -> (0.06, 0.88, 0.05)   verde lavado (crosstalk G->R baixo
//                                            de proposito: 0.06; verde domina)
//   azul     (0,0,1) -> (0.05, 0.04, 0.91)   azul dessaturado, puxado ao ciano
// Somas de linha < 1 em R = perda de pureza (lavagem tipica de tubo barato),
// moderada: o vermelho mantem 0.90 no proprio canal.
//
// Pipeline linear: entrada linear -> GAMUT em linear -> saida linear. A
// conversao final linear->sRGB fica na passagem de apresentacao.
//
// Params.x = forca (0 = identidade, >= 1 = matriz completa). Um leve black
// lift (RME_LIFT) apenas tira o "preto queimado" digital sem exagerar: o tubo
// empoeirado sobe um pouquinho o neutro, nada alem.
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D SrcTex;
layout(set=0, binding=1) uniform sampler SrcSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 Params; // x = forca; y = reservado
} uniforms;
layout(location=0) out vec4 FragColor;

#define RME_LIFT 0.0005

// col = rgb * GAMUT (linha-vetor; ctor coluna-major). Colunas = como R,G,B
// contribuem a cada saida: col0 -> canal vermelho, col1 -> verde, col2 -> azul.
const mat3 GAMUT = mat3(
	0.90, 0.06, 0.05,
	0.05, 0.88, 0.04,
	0.02, 0.05, 0.91
);

void main()
{
	vec4 base = texture(sampler2D(SrcTex, SrcSampler), vUV);
	float k = clamp(uniforms.Params.x, 0.0, 1.2);
	vec3 impure = base.rgb * GAMUT;
	vec3 tint = mix(base.rgb, impure, k) + RME_LIFT * k;
	FragColor = vec4(clamp(tint, vec3(0.0), vec3(1.0)), base.a);
}