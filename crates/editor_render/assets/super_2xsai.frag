// Super 2xSaI: extracted verbatim (algorithm-wise) from the RME reference
// source `gl_composite_shaders.h` (`compositeSuper2xSaiSrc`) and adapted only
// to naga/WGSL conventions:
//   - GLSL 330 -> 450
//   - separate `texture2D` + `sampler` (naga does not support `sampler2D`)
//   - scalar uniforms moved into one std140 uniform block
//   - vertex interface reduced to `vec2` (the reference `vec4 TEX0` carries
//     the coordinates in `.xy`; the split vertex shader emits only location 0)
//   - alpha taken from the centre texel (reference hardcodes 1.0) so the
//     transparent pixels of the composited scene survive the pass
// Licensing: GPL (Derek Liauw Kie Fa, DOSBox Team); file is GPL-derived, as
// agreed with the project maintainer.
#version 450
layout(set=0, binding=0) uniform texture2D Texture;
layout(set=0, binding=1) uniform sampler uTextureSampler;
layout(set=0, binding=2) uniform Uniforms {
	vec2 TextureSize;
	vec2 OutputSize;
	vec2 InputSize;
} uniforms;
layout(location=0) in vec2 TEX0;
layout(location=0) out vec4 FragColor;

#define Source Texture
#define vTexCoord TEX0

const vec3 dtt = vec3(65536.0, 255.0, 1.0);

int GET_RESULT(float A, float B, float C, float D)
{
	int x = 0;
	int y = 0;
	int r = 0;
	if (A == C) x += 1; else if (B == C) y += 1;
	if (A == D) x += 1; else if (B == D) y += 1;
	if (x <= 1) r += 1;
	if (y <= 1) r -= 1;
	return r;
}

float reduce(vec3 color)
{
	return dot(color, dtt);
}

vec3 samplePoint(vec2 uv)
{
	return texture(sampler2D(Source, uTextureSampler), uv).rgb;
}

void main()
{
	vec2 ps = vec2(0.999 / uniforms.TextureSize.x, 0.999 / uniforms.TextureSize.y);

	vec2 dx = vec2(ps.x, 0.0);
	vec2 dy = vec2(0.0, ps.y);
	vec2 g1 = vec2(ps.x, ps.y);
	vec2 g2 = vec2(-ps.x, ps.y);

	vec2 pixcoord = vTexCoord / ps;
	vec2 fp = fract(pixcoord);
	vec2 pC4 = vTexCoord - fp * ps;
	vec2 pC8 = pC4 + g1;

	vec3 C0 = samplePoint(pC4 - g1);
	vec3 C1 = samplePoint(pC4 - dy);
	vec3 C2 = samplePoint(pC4 - g2);
	vec3 D3 = samplePoint(pC4 - g2 + dx);
	vec3 C3 = samplePoint(pC4 - dx);
	vec3 C4 = samplePoint(pC4);
	vec3 C5 = samplePoint(pC4 + dx);
	vec3 D4 = samplePoint(pC8 - g2);
	vec3 C6 = samplePoint(pC4 + g2);
	vec3 C7 = samplePoint(pC4 + dy);
	vec3 C8 = samplePoint(pC4 + g1);
	vec3 D5 = samplePoint(pC8 + dx);
	vec3 D0 = samplePoint(pC4 + g2 + dy);
	vec3 D1 = samplePoint(pC8 + g2);
	vec3 D2 = samplePoint(pC8 + dy);
	vec3 D6 = samplePoint(pC8 + g1);

	float c0 = reduce(C0); float c1 = reduce(C1);
	float c2 = reduce(C2); float c3 = reduce(C3);
	float c4 = reduce(C4); float c5 = reduce(C5);
	float c6 = reduce(C6); float c7 = reduce(C7);
	float c8 = reduce(C8); float d0 = reduce(D0);
	float d1 = reduce(D1); float d2 = reduce(D2);
	float d3 = reduce(D3); float d4 = reduce(D4);
	float d5 = reduce(D5); float d6 = reduce(D6);

	vec3 p00;
	vec3 p10;
	vec3 p01;
	vec3 p11;

	if (c7 == c5 && c4 != c8) {
		p11 = p01 = C7;
	} else if (c4 == c8 && c7 != c5) {
		p11 = p01 = C4;
	} else if (c4 == c8 && c7 == c5) {
		int r = 0;
		r += GET_RESULT(c5, c4, c6, d1);
		r += GET_RESULT(c5, c4, c3, c1);
		r += GET_RESULT(c5, c4, d2, d5);
		r += GET_RESULT(c5, c4, c2, d4);
		if (r > 0) {
			p11 = p01 = C5;
		} else if (r < 0) {
			p11 = p01 = C4;
		} else {
			p11 = p01 = 0.5 * (C4 + C5);
		}
	} else {
		if (c5 == c8 && c8 == d1 && c7 != d2 && c8 != d0) {
			p11 = 0.25 * (3.0 * C8 + C7);
		} else if (c4 == c7 && c7 == d2 && d1 != c8 && c7 != d6) {
			p11 = 0.25 * (3.0 * C7 + C8);
		} else {
			p11 = 0.5 * (C7 + C8);
		}

		if (c5 == c8 && c5 == c1 && c4 != c2 && c5 != c0) {
			p01 = 0.25 * (3.0 * C5 + C4);
		} else if (c4 == c7 && c4 == c2 && c1 != c5 && c4 != d3) {
			p01 = 0.25 * (3.0 * C4 + C5);
		} else {
			p01 = 0.5 * (C4 + C5);
		}
	}

	if (c4 == c8 && c7 != c5 && c3 == c4 && c4 != d2) {
		p10 = 0.5 * (C7 + C4);
	} else if (c4 == c6 && c5 == c4 && c3 != c7 && c4 != d0) {
		p10 = 0.5 * (C7 + C4);
	} else {
		p10 = C7;
	}

	if (c7 == c5 && c4 != c8 && c6 == c7 && c7 != c2) {
		p00 = 0.5 * (C7 + C4);
	} else if (c3 == c7 && c8 == c7 && c6 != c4 && c7 != c0) {
		p00 = 0.5 * (C7 + C4);
	} else {
		p00 = C4;
	}

	if (fp.x < 0.50) {
		if (fp.y < 0.50) {
			p10 = p00;
		}
	} else {
		if (fp.y < 0.50) {
			p10 = p01;
		} else {
			p10 = p11;
		}
	}

	FragColor = vec4(p10, texture(sampler2D(Source, uTextureSampler), pC4).a);
}