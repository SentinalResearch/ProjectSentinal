// Render the Sentinal ad: node render.mjs [--stills 3,12,22] [--fps 30] [--out out/sentinal-ad.mp4]
// Every frame is renderAt(t) — a pure function of time — so output is exact and repeatable.
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(resolve(here, "../../web/package.json"));
const { chromium } = require("@playwright/test");

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const fps = Number(arg("fps", "30"));
const out = resolve(here, arg("out", "out/sentinal-ad.mp4"));
const stills = arg("stills", "");
mkdirSync(resolve(here, "out"), { recursive: true });

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
await page.goto(pathToFileURL(resolve(here, "index.html")).href);
await page.evaluate(() => document.fonts.ready);
const duration = await page.evaluate(() => window.DURATION);

if (stills) {
  for (const t of stills.split(",").map(Number)) {
    await page.evaluate((time) => window.renderAt(time), t);
    const file = resolve(here, `out/still-${String(t).padStart(5, "0")}.png`);
    await page.screenshot({ path: file });
    console.log(file);
  }
  await browser.close();
  process.exit(0);
}

const frames = Math.round(duration * fps);
const ffmpeg = spawn("ffmpeg", [
  "-loglevel", "error", "-y",
  "-f", "image2pipe", "-framerate", String(fps), "-i", "-",
  // JPEG frames are full-range; convert to standard TV range so every player
  // and upload site shows the same blacks.
  "-vf", "scale=in_range=full:out_range=tv,format=yuv420p", "-color_range", "tv",
  "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
  "-c:v", "libx264", "-preset", "slow", "-crf", "16",
  "-movflags", "+faststart", out,
], { stdio: ["pipe", "inherit", "inherit"] });

const started = Date.now();
for (let f = 0; f < frames; f++) {
  await page.evaluate((time) => window.renderAt(time), f / fps);
  const frame = await page.screenshot({ type: "jpeg", quality: 96 });
  if (!ffmpeg.stdin.write(frame)) await new Promise((r) => ffmpeg.stdin.once("drain", r));
  if (f % (fps * 5) === 0) console.log(`frame ${f}/${frames} (${((Date.now() - started) / 1000).toFixed(0)} s)`);
}
ffmpeg.stdin.end();
await new Promise((r) => ffmpeg.on("close", r));
await browser.close();
console.log(`wrote ${out} (${frames} frames at ${fps} fps)`);
