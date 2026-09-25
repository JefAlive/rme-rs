#version 450
// CRT phosphor bloom (custom, portado de compositeCrtBloomSrc).
// "Brilho do feixe no fosforo" estilo neon anos 80.
// Glow aditivo e cromatico em vez de screen-blend que embranquece.
// A base e a cena ja vieram escurecidas pelo World Light (metodo Tibia:
// cena * fator). Quanto menos World Light, mais o bloom esta ativo;
// o glow recupera o brilho original dividindo por esse fator, para que
// objetos brilhantes continuem glowando de noite.
//
// FISICA DO CRT:
//  * intensidade por canal (ganhos): G > R > B  (1.25 / 1.05 / 0.90)
//  * raio / espalhamento: azul espalha mais (450nm) -> R4 / G6 / B8.5;
//  * halos usam valor ANTES do escurecimento (c / WorldLight), ou seja,
//    os objetos brilhantes continuam gerando glow mesmo de noite;
//  * forca do glow cresce quando a luz do mundo cai:
//    boost = mix(0.35, 1.1, 1.0 - WorldLight): dia quase nenhum bloom
//    (sem embranquecer), noite = neon forte;
//  * composicao additiva com min(clamp, 1.0) para proteger brancos;
//  * headroom^2 atenua o glow em pixels claros; linhas pretas preservadas
//    via smoothstep(0, 0.08, lum).
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D SrcTex;
layout(set=0, binding=1) uniform sampler SrcSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 Params; // x = WorldLight (0..1, metodo Tibia); y sem uso
} uniforms;
layout(location=0) out vec4 FragColor;

#define RME_GLOW_THRESHOLD 0.46
#define RME_GLOW_SOFTNESS 0.38
#define RME_STRENGTH 0.52
#define RME_BOOST_DAY 0.35
#define RME_BOOST_NIGHT 1.10
#define RME_RADIUS_R 4.0
#define RME_RADIUS_G 6.0
#define RME_RADIUS_B 8.5
#define TAPS 10
#define RINGS 5

// Ganhos por canal do halo (G > R > B): verde mais brilhante, azul mais fraco mas largo.
#define RME_GAIN vec3(1.05, 1.25, 0.90)

// Recombinacao de fosforo diagonal-dominante: mantem o matiz de cada halo
// (cross-talk pequeno), sem neutralizar para branco como a matriz antiga.
#define RME_P22_R vec3(0.96, 0.07, 0.02)
#define RME_P22_G vec3(0.05, 0.94, 0.04)
#define RME_P22_B vec3(0.03, 0.07, 0.95)

// Anéis sucessivos com peso DECRESCENTE (glow com cauda suave, sem anel rigido).
const float RME_RING[RINGS] = float[RINGS](0.50, 0.85, 1.25, 1.70, 2.20);
const float RME_RING_W[RINGS] = float[RINGS](1.00, 0.70, 0.44, 0.26, 0.14);

const vec2 RME_DIR[TAPS] = vec2[TAPS](
	vec2(1.00, 0.00), vec2(-1.00, 0.00), vec2(0.00, 1.00), vec2(0.00, -1.00), vec2(0.62, 0.62),
	vec2(-0.62, 0.62), vec2(0.62, -0.62), vec2(-0.62, -0.62), vec2(0.38, 0.00), vec2(0.00, 0.38)
);
const float RME_W[TAPS] = float[TAPS](1.00, 1.00, 1.00, 1.00, 0.80, 0.80, 0.80, 0.80, 0.62, 0.62);

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

	// Cena ja veio escurecida pelo World Light (metodo Tibia).
	// Reconstroi luminancia original do fosforo para o glow nao sumir de noite.
	float wl = clamp(uniforms.Params.x, 0.0, 1.0);
	float invWl = 1.0 / max(wl, 0.02);
	vec3 src = c * invWl; // luminancia original usada no glow

	// --- glow source (blur sobre luminancia original, nao sobre cena escura) -----
	vec3 blurred = src * base.a;
	float weight = base.a;
	for (int i = 0; i < TAPS; ++i) {
		vec2 o = RME_DIR[i] * ps;
		vec4 tap = texture(sampler2D(SrcTex, SrcSampler), vUV + o);
		blurred += tap.rgb * invWl * tap.a;
		weight += tap.a;
	}
	blurred /= max(weight, 1e-4);

	float energy = rmeGlow(blurred);
	float spread = energy;

	// --- per-channel halation (original cromatico, sample original dividida) -----
	vec3 halo = vec3(0.0);
	float haloW = 0.0;
	for (int i = 0; i < TAPS; ++i) {
		for (int j = 0; j < RINGS; ++j) {
			float f = RME_RING[j];
			float w = RME_RING_W[j] * RME_W[i];
			vec2 oR = RME_DIR[i] * (RME_RADIUS_R * f * ps);
			vec2 oG = RME_DIR[i] * (RME_RADIUS_G * f * ps);
			vec2 oB = RME_DIR[i] * (RME_RADIUS_B * f * ps);
			halo.r += texture(sampler2D(SrcTex, SrcSampler), vUV - oR).r * invWl * w;
			halo.g += texture(sampler2D(SrcTex, SrcSampler), vUV - oG).g * invWl * w;
			halo.b += texture(sampler2D(SrcTex, SrcSampler), vUV - oB).b * invWl * w;
			haloW += w;
		}
	}
	halo = halo / max(haloW, 1e-4);

	vec3 ph = halo * RME_GAIN;
	vec3 p22 = mat3(RME_P22_R, RME_P22_G, RME_P22_B) * ph;

	// --- forca ligada ao World Light: menos luz = mais bloom ------------------
	float boost = mix(RME_BOOST_DAY, RME_BOOST_NIGHT, 1.0 - wl);

	// Lum da cena original (antes do escurecimento) para headroom + mask
	float lum_orig = dot(src, vec3(0.2126, 0.7152, 0.0722));
	float headroom = 1.0 - lum_orig;

	// Mask: linhas pretas (luma 0) nao recebem glow, preservando definicao.
	// meio-tones e escuros recebem o halo dos vizinhos claros (estilo neon).
	float glowMask = RME_STRENGTH * boost * spread * headroom * headroom;
	glowMask *= smoothstep(0.0, 0.08, lum_orig);

	vec3 glow = p22 * glowMask;

	// Composicao additiva: c + glow, com clamp suave para proteger brancos.
	// O headroom ja atenua o glow em pixels claros; min(clamp) evita estourar 1.
	vec3 outc = c + glow;
	outc = min(outc, vec3(1.0));

	FragColor = vec4(outc, base.a);
}