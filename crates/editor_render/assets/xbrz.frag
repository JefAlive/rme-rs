// xBRZ 4x pixel-art scaler, extracted verbatim (algorithm-wise) from the RME
// reference source `gl_renderer.cpp` (`fragPixelArtSrc`) and adapted only to
// naga/WGSL conventions:
//   - GLSL 330 -> 450
//   - separate `texture2D` + `sampler` (naga does not support `sampler2D`)
//   - scalar uniforms moved into one std140 uniform block
//   - `vColor` removed (no per-vertex tint in this path)
//   - `1.0f` -> `1.0`
// Licensing: RME reference is GPL-3.0; this file remains GPL-derived, as
// agreed with the project maintainer.
#version 450
layout(location=0) in vec2 vUV;
layout(set=0, binding=0) uniform texture2D uTexture;
layout(set=0, binding=1) uniform sampler uTextureSampler;
layout(set=0, binding=2) uniform Uniforms {
	int uSourceCellSize;
	float uOutputCellSize;
	vec2 uTexSize;
} uniforms;
layout(location=0) out vec4 FragColor;

vec4 texS(ivec2 p) {
	p = clamp(p, ivec2(0), ivec2(uniforms.uTexSize) - ivec2(1));
	return texelFetch(uTexture, p, 0);
}

// xBRZ 4x. Zenju's xBRZ, as ported to GLSL by the libretro project
// (xbrz/shaders/4xbrz.glsl). It evaluates the 16 sub-pixels of the 4x output
// block and `f` (the position inside the current sprite pixel, in output space)
// picks the blended result. Input mapping (5x5, centre = index 0):
//   20|21|22|23|24
//   19|06|07|08|09
//   18|05|00|01|10
//   17|04|03|02|11
//   16|15|14|13|12
float xbrzReduce(vec3 color) {
	return dot(color, vec3(65536.0, 256.0, 1.0));
}

float xbrzDist(vec3 a, vec3 b) {
	const vec3 w = vec3(0.2627, 0.6780, 0.0593);
	const float scaleB = 0.5 / (1.0 - w.b);
	const float scaleR = 0.5 / (1.0 - w.r);
	vec3 diff = a - b;
	float Y = dot(diff, w);
	float Cb = scaleB * (diff.b - Y);
	float Cr = scaleR * (diff.r - Y);
	return sqrt(Y * Y + Cb * Cb + Cr * Cr);
}

bool xbrzEq(vec3 a, vec3 b) {
	return xbrzDist(a, b) < (30.0 / 255.0);
}

vec3 xbrzTap(ivec2 base, int cs, int x, int y) {
	return texS(base + ivec2(cs * x, cs * y)).rgb;
}

vec3 xbrzScale(ivec2 base, int cs, vec2 f) {
	const int BLEND_NONE = 0;
	const int BLEND_NORMAL = 1;
	const int BLEND_DOMINANT = 2;
	const float STEEP = 2.2;
	const float DOMINANT = 3.6;

	vec3 src[25];
	src[ 0] = xbrzTap(base, cs,  0,  0);
	src[ 1] = xbrzTap(base, cs,  1,  0);
	src[ 2] = xbrzTap(base, cs,  1,  1);
	src[ 3] = xbrzTap(base, cs,  0,  1);
	src[ 4] = xbrzTap(base, cs, -1,  1);
	src[ 5] = xbrzTap(base, cs, -1,  0);
	src[ 6] = xbrzTap(base, cs, -1, -1);
	src[ 7] = xbrzTap(base, cs,  0, -1);
	src[ 8] = xbrzTap(base, cs,  1, -1);
	src[ 9] = xbrzTap(base, cs,  2, -1);
	src[10] = xbrzTap(base, cs,  2,  0);
	src[11] = xbrzTap(base, cs,  2,  1);
	src[13] = xbrzTap(base, cs,  1,  2);
	src[14] = xbrzTap(base, cs,  0,  2);
	src[15] = xbrzTap(base, cs, -1,  2);
	src[17] = xbrzTap(base, cs, -2,  1);
	src[18] = xbrzTap(base, cs, -2,  0);
	src[19] = xbrzTap(base, cs, -2, -1);
	src[21] = xbrzTap(base, cs, -1, -2);
	src[22] = xbrzTap(base, cs,  0, -2);
	src[23] = xbrzTap(base, cs,  1, -2);

	float v[9];
	v[0] = xbrzReduce(src[0]);
	v[1] = xbrzReduce(src[1]);
	v[2] = xbrzReduce(src[2]);
	v[3] = xbrzReduce(src[3]);
	v[4] = xbrzReduce(src[4]);
	v[5] = xbrzReduce(src[5]);
	v[6] = xbrzReduce(src[6]);
	v[7] = xbrzReduce(src[7]);
	v[8] = xbrzReduce(src[8]);

	ivec4 blendResult = ivec4(BLEND_NONE);

	// Preprocess the four corners around the centre pixel.
	if (!((v[0] == v[1] && v[3] == v[2]) || (v[0] == v[3] && v[1] == v[2]))) {
		float dist_03_01 = xbrzDist(src[4], src[0]) + xbrzDist(src[0], src[8]) + xbrzDist(src[14], src[2]) + xbrzDist(src[2], src[10]) + (4.0 * xbrzDist(src[3], src[1]));
		float dist_00_02 = xbrzDist(src[5], src[3]) + xbrzDist(src[3], src[13]) + xbrzDist(src[7], src[1]) + xbrzDist(src[1], src[11]) + (4.0 * xbrzDist(src[0], src[2]));
		bool dominantGradient = (DOMINANT * dist_03_01) < dist_00_02;
		blendResult[2] = ((dist_03_01 < dist_00_02) && (v[0] != v[1]) && (v[0] != v[3])) ? (dominantGradient ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
	}

	if (!((v[5] == v[0] && v[4] == v[3]) || (v[5] == v[4] && v[0] == v[3]))) {
		float dist_04_00 = xbrzDist(src[17], src[5]) + xbrzDist(src[5], src[7]) + xbrzDist(src[15], src[3]) + xbrzDist(src[3], src[1]) + (4.0 * xbrzDist(src[4], src[0]));
		float dist_05_03 = xbrzDist(src[18], src[4]) + xbrzDist(src[4], src[14]) + xbrzDist(src[6], src[0]) + xbrzDist(src[0], src[2]) + (4.0 * xbrzDist(src[5], src[3]));
		bool dominantGradient = (DOMINANT * dist_05_03) < dist_04_00;
		blendResult[3] = ((dist_04_00 > dist_05_03) && (v[0] != v[5]) && (v[0] != v[3])) ? (dominantGradient ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
	}

	if (!((v[7] == v[8] && v[0] == v[1]) || (v[7] == v[0] && v[8] == v[1]))) {
		float dist_00_08 = xbrzDist(src[5], src[7]) + xbrzDist(src[7], src[23]) + xbrzDist(src[3], src[1]) + xbrzDist(src[1], src[9]) + (4.0 * xbrzDist(src[0], src[8]));
		float dist_07_01 = xbrzDist(src[6], src[0]) + xbrzDist(src[0], src[2]) + xbrzDist(src[22], src[8]) + xbrzDist(src[8], src[10]) + (4.0 * xbrzDist(src[7], src[1]));
		bool dominantGradient = (DOMINANT * dist_07_01) < dist_00_08;
		blendResult[1] = ((dist_00_08 > dist_07_01) && (v[0] != v[7]) && (v[0] != v[1])) ? (dominantGradient ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
	}

	if (!((v[6] == v[7] && v[5] == v[0]) || (v[6] == v[5] && v[7] == v[0]))) {
		float dist_05_07 = xbrzDist(src[18], src[6]) + xbrzDist(src[6], src[22]) + xbrzDist(src[4], src[0]) + xbrzDist(src[0], src[8]) + (4.0 * xbrzDist(src[5], src[7]));
		float dist_06_00 = xbrzDist(src[19], src[5]) + xbrzDist(src[5], src[3]) + xbrzDist(src[21], src[7]) + xbrzDist(src[7], src[1]) + (4.0 * xbrzDist(src[6], src[0]));
		bool dominantGradient = (DOMINANT * dist_05_07) < dist_06_00;
		blendResult[0] = ((dist_05_07 < dist_06_00) && (v[0] != v[5]) && (v[0] != v[7])) ? (dominantGradient ? BLEND_DOMINANT : BLEND_NORMAL) : BLEND_NONE;
	}

	vec3 dst[16];
	dst[ 0] = src[0]; dst[ 1] = src[0]; dst[ 2] = src[0]; dst[ 3] = src[0];
	dst[ 4] = src[0]; dst[ 5] = src[0]; dst[ 6] = src[0]; dst[ 7] = src[0];
	dst[ 8] = src[0]; dst[ 9] = src[0]; dst[10] = src[0]; dst[11] = src[0];
	dst[12] = src[0]; dst[13] = src[0]; dst[14] = src[0]; dst[15] = src[0];

	if (any(notEqual(blendResult, ivec4(BLEND_NONE)))) {
		float dist_01_04;
		float dist_03_08;
		bool haveShallowLine;
		bool haveSteepLine;
		bool needBlend;
		bool doLineBlend;
		vec3 blendPix;

		// Corner (1, 1)
		dist_01_04 = xbrzDist(src[1], src[4]);
		dist_03_08 = xbrzDist(src[3], src[8]);
		haveShallowLine = (STEEP * dist_01_04 <= dist_03_08) && (v[0] != v[4]) && (v[5] != v[4]);
		haveSteepLine   = (STEEP * dist_03_08 <= dist_01_04) && (v[0] != v[8]) && (v[7] != v[8]);
		needBlend = (blendResult[2] != BLEND_NONE);
		doLineBlend = (blendResult[2] >= BLEND_DOMINANT ||
			!((blendResult[1] != BLEND_NONE && !xbrzEq(src[0], src[4])) ||
			  (blendResult[3] != BLEND_NONE && !xbrzEq(src[0], src[8])) ||
			  (xbrzEq(src[4], src[3]) && xbrzEq(src[3], src[2]) && xbrzEq(src[2], src[1]) && xbrzEq(src[1], src[8]) && !xbrzEq(src[0], src[2]))));

		blendPix = (xbrzDist(src[0], src[1]) <= xbrzDist(src[0], src[3])) ? src[1] : src[3];
		dst[ 2] = mix(dst[ 2], blendPix, (needBlend && doLineBlend) ? (haveShallowLine ? (haveSteepLine ? 1.0 / 3.0 : 0.25) : (haveSteepLine ? 0.25 : 0.00)) : 0.00);
		dst[ 9] = mix(dst[ 9], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.25 : 0.00);
		dst[10] = mix(dst[10], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.75 : 0.00);
		dst[11] = mix(dst[11], blendPix, (needBlend) ? ((doLineBlend) ? ((haveSteepLine) ? 1.00 : ((haveShallowLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[12] = mix(dst[12], blendPix, (needBlend) ? ((doLineBlend) ? 1.00 : 0.6848532563) : 0.00);
		dst[13] = mix(dst[13], blendPix, (needBlend) ? ((doLineBlend) ? ((haveShallowLine) ? 1.00 : ((haveSteepLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[14] = mix(dst[14], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.75 : 0.00);
		dst[15] = mix(dst[15], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.25 : 0.00);

		// Corner (1, 0)
		dist_01_04 = xbrzDist(src[7], src[2]);
		dist_03_08 = xbrzDist(src[1], src[6]);
		haveShallowLine = (STEEP * dist_01_04 <= dist_03_08) && (v[0] != v[2]) && (v[3] != v[2]);
		haveSteepLine   = (STEEP * dist_03_08 <= dist_01_04) && (v[0] != v[6]) && (v[5] != v[6]);
		needBlend = (blendResult[1] != BLEND_NONE);
		doLineBlend = (blendResult[1] >= BLEND_DOMINANT ||
			!((blendResult[0] != BLEND_NONE && !xbrzEq(src[0], src[2])) ||
			  (blendResult[2] != BLEND_NONE && !xbrzEq(src[0], src[6])) ||
			  (xbrzEq(src[2], src[1]) && xbrzEq(src[1], src[8]) && xbrzEq(src[8], src[7]) && xbrzEq(src[7], src[6]) && !xbrzEq(src[0], src[8]))));

		blendPix = (xbrzDist(src[0], src[7]) <= xbrzDist(src[0], src[1])) ? src[7] : src[1];
		dst[ 1] = mix(dst[ 1], blendPix, (needBlend && doLineBlend) ? (haveShallowLine ? (haveSteepLine ? 1.0 / 3.0 : 0.25) : (haveSteepLine ? 0.25 : 0.00)) : 0.00);
		dst[ 6] = mix(dst[ 6], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.25 : 0.00);
		dst[ 7] = mix(dst[ 7], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.75 : 0.00);
		dst[ 8] = mix(dst[ 8], blendPix, (needBlend) ? ((doLineBlend) ? ((haveSteepLine) ? 1.00 : ((haveShallowLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[ 9] = mix(dst[ 9], blendPix, (needBlend) ? ((doLineBlend) ? 1.00 : 0.6848532563) : 0.00);
		dst[10] = mix(dst[10], blendPix, (needBlend) ? ((doLineBlend) ? ((haveShallowLine) ? 1.00 : ((haveSteepLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[11] = mix(dst[11], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.75 : 0.00);
		dst[12] = mix(dst[12], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.25 : 0.00);

		// Corner (0, 0)
		dist_01_04 = xbrzDist(src[5], src[8]);
		dist_03_08 = xbrzDist(src[7], src[4]);
		haveShallowLine = (STEEP * dist_01_04 <= dist_03_08) && (v[0] != v[8]) && (v[1] != v[8]);
		haveSteepLine   = (STEEP * dist_03_08 <= dist_01_04) && (v[0] != v[4]) && (v[3] != v[4]);
		needBlend = (blendResult[0] != BLEND_NONE);
		doLineBlend = (blendResult[0] >= BLEND_DOMINANT ||
			!((blendResult[3] != BLEND_NONE && !xbrzEq(src[0], src[8])) ||
			  (blendResult[1] != BLEND_NONE && !xbrzEq(src[0], src[4])) ||
			  (xbrzEq(src[8], src[7]) && xbrzEq(src[7], src[6]) && xbrzEq(src[6], src[5]) && xbrzEq(src[5], src[4]) && !xbrzEq(src[0], src[6]))));

		blendPix = (xbrzDist(src[0], src[5]) <= xbrzDist(src[0], src[7])) ? src[5] : src[7];
		dst[ 0] = mix(dst[ 0], blendPix, (needBlend && doLineBlend) ? (haveShallowLine ? (haveSteepLine ? 1.0 / 3.0 : 0.25) : (haveSteepLine ? 0.25 : 0.00)) : 0.00);
		dst[15] = mix(dst[15], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.25 : 0.00);
		dst[ 4] = mix(dst[ 4], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.75 : 0.00);
		dst[ 5] = mix(dst[ 5], blendPix, (needBlend) ? ((doLineBlend) ? ((haveSteepLine) ? 1.00 : ((haveShallowLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[ 6] = mix(dst[ 6], blendPix, (needBlend) ? ((doLineBlend) ? 1.00 : 0.6848532563) : 0.00);
		dst[ 7] = mix(dst[ 7], blendPix, (needBlend) ? ((doLineBlend) ? ((haveShallowLine) ? 1.00 : ((haveSteepLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[ 8] = mix(dst[ 8], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.75 : 0.00);
		dst[ 9] = mix(dst[ 9], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.25 : 0.00);

		// Corner (0, 1)
		dist_01_04 = xbrzDist(src[3], src[6]);
		dist_03_08 = xbrzDist(src[5], src[2]);
		haveShallowLine = (STEEP * dist_01_04 <= dist_03_08) && (v[0] != v[6]) && (v[7] != v[6]);
		haveSteepLine   = (STEEP * dist_03_08 <= dist_01_04) && (v[0] != v[2]) && (v[1] != v[2]);
		needBlend = (blendResult[3] != BLEND_NONE);
		doLineBlend = (blendResult[3] >= BLEND_DOMINANT ||
			!((blendResult[2] != BLEND_NONE && !xbrzEq(src[0], src[6])) ||
			  (blendResult[0] != BLEND_NONE && !xbrzEq(src[0], src[2])) ||
			  (xbrzEq(src[6], src[5]) && xbrzEq(src[5], src[4]) && xbrzEq(src[4], src[3]) && xbrzEq(src[3], src[2]) && !xbrzEq(src[0], src[4]))));

		blendPix = (xbrzDist(src[0], src[3]) <= xbrzDist(src[0], src[5])) ? src[3] : src[5];
		dst[ 3] = mix(dst[ 3], blendPix, (needBlend && doLineBlend) ? (haveShallowLine ? (haveSteepLine ? 1.0 / 3.0 : 0.25) : (haveSteepLine ? 0.25 : 0.00)) : 0.00);
		dst[12] = mix(dst[12], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.25 : 0.00);
		dst[13] = mix(dst[13], blendPix, (needBlend && doLineBlend && haveSteepLine) ? 0.75 : 0.00);
		dst[14] = mix(dst[14], blendPix, (needBlend) ? ((doLineBlend) ? ((haveSteepLine) ? 1.00 : ((haveShallowLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[15] = mix(dst[15], blendPix, (needBlend) ? ((doLineBlend) ? 1.00 : 0.6848532563) : 0.00);
		dst[ 4] = mix(dst[ 4], blendPix, (needBlend) ? ((doLineBlend) ? ((haveShallowLine) ? 1.00 : ((haveSteepLine) ? 0.75 : 0.50)) : 0.08677704501) : 0.00);
		dst[ 5] = mix(dst[ 5], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.75 : 0.00);
		dst[ 6] = mix(dst[ 6], blendPix, (needBlend && doLineBlend && haveShallowLine) ? 0.25 : 0.00);
	}

	// 16 sub-pixels (4x4) selected by the intra-pixel position.
	return mix(
		mix(mix(mix(dst[ 6], dst[ 7], step(0.25, f.x)), mix(dst[ 8], dst[ 9], step(0.75, f.x)), step(0.50, f.x)),
			mix(mix(dst[ 5], dst[ 0], step(0.25, f.x)), mix(dst[ 1], dst[10], step(0.75, f.x)), step(0.50, f.x)), step(0.25, f.y)),
		mix(mix(mix(dst[ 4], dst[ 3], step(0.25, f.x)), mix(dst[ 2], dst[11], step(0.75, f.x)), step(0.50, f.x)),
			mix(mix(dst[15], dst[14], step(0.25, f.x)), mix(dst[13], dst[12], step(0.75, f.x)), step(0.50, f.x)), step(0.75, f.y)),
		step(0.50, f.y));
}

void main() {
	int sourceCs = max(1, uniforms.uSourceCellSize);
	float outputCs = max(1.0, uniforms.uOutputCellSize);

	// Below one screen pixel per sprite pixel the scaler would have to
	// minify; leave that to the plain (nearest / smooth) blit.
	if (outputCs <= 1.0) {
		FragColor = texture(sampler2D(uTexture, uTextureSampler), vUV);
		return;
	}

	// `outputCs` maps screen pixels to sprite pixels, while `sourceCs` is the
	// texel density of one sprite pixel in the supersampled FBO. The
	// neighbourhood is therefore fetched at `base + offset * sourceCs`, but
	// the intra-pixel fraction that selects the pattern lives in output space.
	vec2 p = vUV * uniforms.uTexSize / float(sourceCs);
	ivec2 c = ivec2(int(floor(p.x)), int(floor(p.y)));
	vec2 f = p - vec2(c);
	ivec2 base = c * sourceCs;

	vec3 color = xbrzScale(base, sourceCs, f);
	float a = texS(base).a;
	FragColor = vec4(color, a);
}