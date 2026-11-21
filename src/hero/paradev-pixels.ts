/*
Adapted from Paradev DitherClouds (square pixels scaled for our desktop hero).
https://github.com/paradevai/paradev/tree/main/apps/marketing/src/app/[lang]/components/HeroSection/components/AppMockup/components/MainPanel/components/DitherClouds

MIT License

Copyright (c) 2025-2026 Superset Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

export const paradevPatternDefaults = { pixelOpacity: 100, pixelSize: 2, pixelDensity: 100, pixelSpeed: 100, pixelX: 0, pixelY: 0 };
export type ParadevPatternSettings = typeof paradevPatternDefaults;

// Live React state may predate newly added controls during Fast Refresh.
// Sanitize at the renderer boundary as well as when loading saved settings.
export function normalizeParadevSettings(settings: Partial<ParadevPatternSettings> = {}): ParadevPatternSettings {
  const finite = (value: number | undefined, fallback: number, min: number, max: number) =>
    typeof value === 'number' && Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : fallback;
  return {
    pixelOpacity: finite(settings.pixelOpacity, 100, 0, 100),
    pixelSize: Math.round(finite(settings.pixelSize, 2, 1, 5)),
    pixelDensity: finite(settings.pixelDensity, 100, 0, 200),
    pixelSpeed: finite(settings.pixelSpeed, 100, 0, 300),
    pixelX: finite(settings.pixelX, 0, -100, 100),
    pixelY: finite(settings.pixelY, 0, -100, 100),
  };
}

const DRIFT_PER_FRAME = 0.0044;

const BAYER = [
	[0, 32, 8, 40, 2, 34, 10, 42],
	[48, 16, 56, 24, 50, 18, 58, 26],
	[12, 44, 4, 36, 14, 46, 6, 38],
	[60, 28, 52, 20, 62, 30, 54, 22],
	[3, 35, 11, 43, 1, 33, 9, 41],
	[51, 19, 59, 27, 49, 17, 57, 25],
	[15, 47, 7, 39, 13, 45, 5, 37],
	[63, 31, 55, 23, 61, 29, 53, 21],
];

const hash = (i: number, j: number) => {
	const h = Math.sin(i * 127.1 + j * 311.7) * 43758.5453;
	return h - Math.floor(h);
};

const smooth = (t: number) => t * t * (3 - 2 * t);

const noise = (x: number, y: number) => {
	const i = Math.floor(x);
	const j = Math.floor(y);
	const u = smooth(x - i);
	const v = smooth(y - j);
	const a = hash(i, j);
	const b = hash(i + 1, j);
	const c = hash(i, j + 1);
	const d = hash(i + 1, j + 1);
	return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
};

const fbm = (x: number, y: number) => {
	let sum = 0;
	let amp = 0.5;
	for (let o = 0; o < 5; o++) {
		sum += amp * noise(x, y);
		x *= 2.03;
		y *= 2.03;
		amp *= 0.5;
	}
	return sum;
};

const STARS = Array.from({ length: 24 }, (_, i) => ({
	x: hash(i, 1.7),
	y: hash(i, 9.3),
	sparkle: i % 13 === 0,
	phase: hash(i, 4.1) * Math.PI * 2,
}));

export function drawParadevPixels(
	ctx: CanvasRenderingContext2D,
	width: number,
	height: number,
	frame: number,
	input: Partial<ParadevPatternSettings> = paradevPatternDefaults,
) {
	const settings = normalizeParadevSettings(input);
	const DOT = settings.pixelSize;
	const CELL = DOT * 2;
	const cols = Math.ceil(width / CELL);
	const rows = Math.ceil(height / CELL);
	const t = frame * DRIFT_PER_FRAME;
	ctx.clearRect(0, 0, width, height);
	if (settings.pixelDensity === 0) return;

	for (let j = 0; j < rows; j++) {
		const ny = j / rows - settings.pixelY / 100;
		for (let i = 0; i < cols; i++) {
			const nx = i / cols - settings.pixelX / 100;
			const px = nx * 7.5 + t;
			const py = ny * 4.2;
			const warp = fbm(px * 0.6 + 7.3, py * 0.6 - t * 0.5);
			let d = fbm(px + warp * 1.8, py + warp * 0.9);
			const band =
				0.75 * Math.exp(-(((ny - 0.08 - nx * 0.06) / 0.17) ** 2)) +
				0.8 * Math.exp(-(((ny - 0.72 + (1 - nx) * 0.25) / 0.22) ** 2));
			d = (d * (0.55 + 0.6 * band) - 0.53) * 3.2;
			if (d <= 0) continue;
			const level = Math.min(d * settings.pixelDensity / 100, 0.82);
			if (level * 64 <= (BAYER[j % 8]?.[i % 8] ?? 0)) continue;
			ctx.fillStyle = `rgba(232,232,236,${0.14 + 0.38 * level})`;
			ctx.fillRect(i * CELL, j * CELL, DOT, DOT);
		}
	}

	for (const star of STARS) {
		const twinkle = 0.45 + 0.55 * Math.sin(frame * 0.09 + star.phase) ** 2;
		const x = Math.floor((((star.x + settings.pixelX / 100 + 1) % 1) * width) / CELL) * CELL;
		const y = Math.floor((((star.y + settings.pixelY / 100 + 1) % 1) * height) / CELL) * CELL;
		ctx.fillStyle = `rgba(255,255,255,${(star.sparkle ? 0.9 : 0.55) * twinkle})`;
		ctx.fillRect(x, y, DOT, DOT);
		if (star.sparkle) {
			ctx.fillStyle = `rgba(255,255,255,${0.45 * twinkle})`;
			ctx.fillRect(x - CELL, y, DOT, DOT);
			ctx.fillRect(x + CELL, y, DOT, DOT);
			ctx.fillRect(x, y - CELL, DOT, DOT);
			ctx.fillRect(x, y + CELL, DOT, DOT);
		}
	}
}
