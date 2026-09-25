#version 450
// Lens Mist: névoa difusa de lente (veiling glare) sobre a cena FINAL já
// iluminada. out = mix(cena, blur_grosso(cena), k): como é um mix com o pró-
// prio blur (0 <= k <= 1), NUNCA adiciona energia — não clareia a noite, só
// amacia as bordas de brilho como uma lente com mist. Sem lift de pretos.
//
// * desacoplado do World Light: a cena já chega escura de noite (light buffer);
// * raio do blur em px de tela (3.0) — acompanha o zoom automaticamente;
// * foco em cores frias: força maior onde o azul domina (noite), véu com
//   leve tirada fria;
// * Params.x = forca k (0..~0.25 no slider), Params.y = escala do raio
//   (drive pelo World Light: maior raio conforme mais escuro, 0 acima de 50%).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D SrcTex;
layout(set=0, binding=1) uniform sampler SrcSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 Params;
} uniforms;
layout(location=0) out vec4 FragColor;

#define MIST_RADIUS 3.0
#define TAPS 12

// Direções unitárias de 12 direções (cardinais + diagonais + eixos 2:1)
// para um blur aproximadamente circular.
const vec2 MIST_DIR[TAPS] = vec2[TAPS](
	vec2(1.0000, 0.0000), vec2(-1.0000, 0.0000), vec2(0.0000, 1.0000), vec2(0.0000, -1.0000),
	vec2(0.7071, 0.7071), vec2(-0.7071, 0.7071), vec2(0.7071, -0.7071), vec2(-0.7071, -0.7071),
	vec2(0.8944, 0.4472), vec2(-0.8944, 0.4472), vec2(0.8944, -0.4472), vec2(-0.8944, -0.4472)
);
// Pesos decrescentes por anel (gaussiano aproximado).
const float MIST_W[TAPS] = float[TAPS](
	1.00, 1.00, 1.00, 1.00,
	0.80, 0.80, 0.80, 0.80,
	0.65, 0.65, 0.65, 0.65
);

void main()
{
	vec2 ps = 1.0 / uniforms.TextureSize;
	vec4 base = texture(sampler2D(SrcTex, SrcSampler), vUV);
	float rs = max(uniforms.Params.y, 0.1);

	vec3 acc = base.rgb * 1.0;
	float wsum = 1.0;
	for (int i = 0; i < TAPS; ++i) {
		vec2 o = MIST_DIR[i] * (MIST_RADIUS * rs * ps);
		acc += texture(sampler2D(SrcTex, SrcSampler), vUV + o).rgb * MIST_W[i];
		wsum += MIST_W[i];
	}
	vec3 blurred = acc / wsum;

	float k = clamp(uniforms.Params.x, 0.0, 1.0);

	// Foco em cores frias: a força da névoa cresce onde o azul domina (noite)
	// e diminui nas cores quentes; o véu tem uma leve tirada fria.
	float cool = clamp(max(base.b - base.r, 0.0) * 4.0, 0.0, 1.0);
	float kEff = k * mix(0.4, 1.0, cool);
	vec3 veil = blurred * vec3(0.92, 0.96, 1.08);

	FragColor = vec4(mix(base.rgb, veil, kEff), base.a);
}