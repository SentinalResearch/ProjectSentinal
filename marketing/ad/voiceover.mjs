// Add a macOS neural-voice narration to the rendered ad.
// node voiceover.mjs [--voice "Ava (Premium)"] [--in out/sentinal-ad.mp4] [--out out/sentinal-ad-voiceover.mp4]
//                    [--script "sentinal-videos/voice-over script.md"] [--rate 165]
// --script reads a Markdown table of `| m:ss–m:ss | scene | line |` rows instead of SCRIPT below.
// Each line starts with its scene; a line that would overrun its slot is spoken faster.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const voice = arg("voice", "Ava (Premium)");
const input = resolve(here, arg("in", "out/sentinal-ad.mp4"));
const output = resolve(here, arg("out", "out/sentinal-ad-voiceover.mp4"));
const work = resolve(here, "out/vo");
mkdirSync(work, { recursive: true });

// [start seconds, latest end seconds, text] — aligned with the scenes in index.html.
const SCRIPT = [
  [0.4, 4.9, "One terminal. One closed lid. Hours of work, gone."],
  [5.4, 8.9, "Meet Sentinal. Terminal sessions that survive."],
  [9.5, 18.8, "Wrap any job with one command. Sentinal owns the terminal, so you can detach, close your laptop, and it keeps running."],
  [19.5, 26.8, "Reattach from any browser. Same process, same screen, with live progress and GPU usage."],
  [27.5, 35.8, "Add one shell hook, and every command is recorded, with its exit code and duration."],
  [36.5, 42.8, "Already running? Adopt it, read-only. Sentinal never guesses how it ended."],
  [43.5, 48.9, "Private by design. Your machines. Your tailnet. No keystroke logging."],
  [49.5, 54.7, "And soon, all of it, right in your pocket."],
  [54.8, 58.5, "Sentinal. Structured state first, intelligence second."],
];

const BASE_RATE = Number(arg("rate", "175"));
const MAX_RATE = BASE_RATE + 20;

/** Rows of `| 0:04–0:08 | Scene | Line |` from a Markdown voice-over script. */
function parseScript(path) {
  const seconds = (stamp) => stamp.split(":").reduce((total, part) => total * 60 + Number(part), 0);
  return readFileSync(path, "utf8")
    .split("\n")
    .map((line) => line.match(/^\|\s*(\d+:\d{2})\s*[–-]\s*(\d+:\d{2})\s*\|[^|]*\|\s*(.+?)\s*\|\s*$/))
    .filter(Boolean)
    .map(([, from, to, text]) => [seconds(from) + 0.2, seconds(to) - 0.15, text]);
}
const scriptFile = arg("script", "");
const LINES = scriptFile ? parseScript(resolve(here, scriptFile)) : SCRIPT;
if (LINES.length === 0) throw new Error(`no | m:ss–m:ss | scene | line | rows in ${scriptFile}`);
const duration = (file) =>
  Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", file]).toString());

function speak(text, file, rate) {
  execFileSync("say", ["-v", voice, "-r", String(rate), "-o", file, text]);
  return duration(file);
}

const clips = LINES.map(([start, end, text], i) => {
  const file = resolve(work, `line-${i}.aiff`);
  const slot = end - start;
  let rate = BASE_RATE;
  let length = speak(text, file, rate);
  if (length > slot) {
    rate = Math.min(MAX_RATE, Math.ceil((BASE_RATE * length) / slot) + 2);
    length = speak(text, file, rate);
  }
  const fits = length <= slot + 0.05;
  console.log(`${start.toFixed(1)}s  ${length.toFixed(2)}s/${slot.toFixed(1)}s  rate ${rate}${fits ? "" : "  ⚠ overruns"}  ${text}`);
  return { file, start };
});

// Place each line at its start time, mix, and normalize to a streaming-friendly loudness.
const inputs = clips.flatMap((clip) => ["-i", clip.file]);
const delays = clips
  .map((clip, i) => `[${i + 1}:a]aresample=48000,adelay=${Math.round(clip.start * 1000)}:all=1[a${i}]`)
  .join(";");
const mix = `${clips.map((_, i) => `[a${i}]`).join("")}amix=inputs=${clips.length}:normalize=0,` +
  "highpass=f=80,acompressor=threshold=-18dB:ratio=3:attack=5:release=120,loudnorm=I=-16:TP=-1.5:LRA=7,apad[vo]";
execFileSync("ffmpeg", [
  "-loglevel", "error", "-y", "-i", input, ...inputs,
  "-filter_complex", `${delays};${mix}`,
  "-map", "0:v", "-map", "[vo]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
  "-t", String(duration(input)), "-movflags", "+faststart", output,
]);
console.log(`wrote ${output} (voice: ${voice})`);
