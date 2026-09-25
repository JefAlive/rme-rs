#version 450
// Conversão final linear → sRGB para apresentação no egui.
// O pipeline roda todo em linear (cena, world light, bloom, crt color);
// esta passagem NÃO aplica gamma — escreve LINEAR no alvo sRGB,
// deixando a GPU fazer a codificação linear→sRGB nativa (evita dupla codificação).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D Source;
layout(set=0, binding=1) uniform sampler SourceSampler;
layout(location=0) out vec4 FragColor;

void main()
{
	vec4 base = texture(sampler2D(Source, SourceSampler), vUV);
	// base já está em linear (GPU decodificou sRGB→linear na amostragem).
	// Escrevemos LINEAR no alvo Rgba8UnormSrgb; GPU codifica linear→sRGB nativamente.
	FragColor = vec4(max(base.rgb, vec3(0.0)), base.a);
}