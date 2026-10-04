// Render a design-tool film (`.dc.html` + animations-v3 engine) to MP4 locally.
//
//   node render-dc.mjs "sentinal-videos/Sentinal Film.dc.html" [--fps 60] [--out out/film.mp4]
//   node render-dc.mjs "sentinal-videos/Sentinal Film.dc.html" --stills 2,10,30
//
// The engine's stage (the element with data-om-exportable-video-with-duration-secs)
// accepts a `data-om-seek-to-time-frame` event; with detail.sync it commits the
// frame synchronously, so every frame is exactly its timestamp.
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { createServer } from "node:net";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(resolve(here, "../../web/package.json"));
const { chromium } = require("@playwright/test");

const page_ = process.argv[2];
if (!page_) throw new Error('usage: node render-dc.mjs "<file>.dc.html" [--fps 60] [--out out/x.mp4] [--stills 1,2]');
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const fps = Number(arg("fps", "60"));
const stills = arg("stills", "");
const uhd = process.argv.includes("--4k");
const pixelRatio = 2; // supersample: capture at 2x, downscale for 1080p, keep for --4k
const file = resolve(here, page_);
const slug = basename(file, ".dc.html").toLowerCase().replace(/[^a-z0-9]+/g, "-");
const out = resolve(here, arg("out", `out/${slug}.mp4`));
mkdirSync(resolve(here, "out"), { recursive: true });

// Serve the folder over HTTP (the runtime's script loads fail from file://).
const port = await new Promise((ok) => {
  const probe = createServer().listen(0, "127.0.0.1", () => {
    const { port } = probe.address();
    probe.close(() => ok(port));
  });
});
const server = spawn("python3", ["-m", "http.server", String(port), "--bind", "127.0.0.1", "--directory", dirname(file)], {
  stdio: "ignore",
});
const stop = () => server.kill();
process.on("exit", stop);

const browser = await chromium.launch();
try {
  const context = await browser.newContext({ viewport: { width: 2400, height: 2400 }, deviceScaleFactor: pixelRatio });
  const page = await context.newPage();
  const url = `http://127.0.0.1:${port}/${encodeURIComponent(basename(file))}`;
  for (let attempt = 0; ; attempt++) {
    try {
      await page.goto(url, { waitUntil: "networkidle" });
      break;
    } catch (error) {
      if (attempt > 20) throw error;
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  const stage = page.locator("[data-om-exportable-video-with-duration-secs]").first();
  await stage.waitFor({ timeout: 30_000 });
  await page.evaluate(() => document.fonts.ready);
  const duration = Number(await stage.getAttribute("data-om-exportable-video-with-duration-secs"));
  const size = await stage.evaluate((el) => ({ w: Number(el.getAttribute("width")), h: Number(el.getAttribute("height")) }));
  // Show the stage at exactly 1:1 so the capture is native resolution.
  await page.setViewportSize({ width: size.w, height: size.h + 44 });
  await page.waitForTimeout(300);

  const seek = (time) =>
    stage.evaluate((el, t) => {
      el.dispatchEvent(new CustomEvent("data-om-seek-to-time-frame", { detail: { time: t, sync: true, playing: false } }));
    }, time);
  const capture = async (options) => {
    const box = await stage.boundingBox();
    return page.screenshot({ ...options, clip: { x: box.x, y: box.y, width: size.w, height: size.h } });
  };
  const box0 = await stage.boundingBox();
  if (Math.round(box0.width) !== size.w) throw new Error(`stage is scaled (${box0.width}px wide, expected ${size.w})`);
  console.log(`${basename(file)}: ${duration}s at ${size.w}x${size.h} → ${uhd ? `${size.w * pixelRatio}x${size.h * pixelRatio}` : `${size.w}x${size.h}`}`);

  if (stills) {
    for (const t of stills.split(",").map(Number)) {
      await seek(t);
      const path = resolve(here, `out/${slug}-${String(t).padStart(5, "0")}.png`);
      await capture({ path });
      console.log(path);
    }
  } else {
    const ffmpeg = spawn("ffmpeg", [
      "-loglevel", "error", "-y",
      "-f", "image2pipe", "-framerate", String(fps), "-i", "-",
      "-vf", `scale=${uhd ? size.w * pixelRatio : size.w}:-2:flags=lanczos:in_range=full:out_range=tv,format=yuv420p`, "-color_range", "tv",
      "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709",
      "-c:v", "libx264", "-preset", "slow", "-crf", "14", "-movflags", "+faststart", out,
    ], { stdio: ["pipe", "inherit", "inherit"] });
    const frames = Math.round(duration * fps);
    const started = Date.now();
    for (let f = 0; f < frames; f++) {
      await seek(f / fps);
      const frame = await capture({ type: "jpeg", quality: 97 });
      if (!ffmpeg.stdin.write(frame)) await new Promise((r) => ffmpeg.stdin.once("drain", r));
      if (f % (fps * 5) === 0) console.log(`frame ${f}/${frames} (${((Date.now() - started) / 1000).toFixed(0)} s)`);
    }
    ffmpeg.stdin.end();
    await new Promise((r) => ffmpeg.on("close", r));
    console.log(`wrote ${out}`);
  }
} finally {
  await browser.close();
  stop();
}
