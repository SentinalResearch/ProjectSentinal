# Sentinal product ad (59 s, 1920×1080, captions only)

This directory contains reproducible film source. Rendered video, audio, and
stills under `out/` are generated artifacts and are intentionally excluded
from Git. The films preserve their original pre-V1 narrative: “coming soon”
remains accurate while the repository is a release candidate and the
production PWA and real two-Machine acceptance gates are open.

The ad is code: `index.html` draws every frame as a pure function of time
(`window.renderAt(t)`), and `render.mjs` steps through it in headless Chromium
and encodes the frames with ffmpeg. The same input always produces the same video.

## Render

```sh
# from the repo root; needs ffmpeg and the web dev dependencies (pnpm -C web install)
node marketing/ad/render.mjs                         # → marketing/ad/out/sentinal-ad.mp4
node marketing/ad/render.mjs --stills 3,14,23,52     # review frames as PNGs
node marketing/ad/render.mjs --fps 60 --out out/sentinal-ad-60fps.mp4
```

Open `index.html` directly in a browser for a live, looping preview.

## Edit

- Captions and their timing: `CAPTIONS` in `index.html`.
- Scene labels (`01 · WATCH` …): `LABELS`.
- Scene timing: each `renderSN(t)` calls `show('sN', t, start, end)`.
- Colors and fonts come from the product's design tokens (`web/src/styles/tokens.css`)
  and its self-hosted Archivo / JetBrains Mono fonts.

## Storyboard

| Time | Scene |
|---|---|
| 0–5 s | Problem: a long training run dies with its SSH terminal |
| 5–9 s | Logo and tagline |
| 9–19 s | `sentinal watch` beside the `CreateSession` it sends; Ctrl+J detach |
| 19–27 s | Reattach from a browser: same process, live progress and GPU/VRAM |
| 27–36 s | `sentinal hook install`; every command with ✓/✗ and duration |
| 36–43 s | `sentinal adopt`: read-only, ends as "exit code unknown" |
| 43–49 s | Private by design: tailnet-only bind, no keystroke recording |
| 49–55 s | Mobile app — marked COMING SOON |
| 55–59 s | End card |

## Honesty notes

CLI output, the protocol snippets, and the `0.0.0.0` error text are the real
ones. The progress bar, loss, and health-style values in the browser scene
are now implemented by deterministic watchers and structured training
summaries. The mobile app remains labelled “coming soon” because production
PWA delivery and real-device V1 acceptance are still release gates.

## Voice-over

`voiceover.mjs` narrates the rendered video with an offline macOS neural voice
and writes `out/sentinal-ad-voiceover.mp4` (captions stay on screen):

```sh
node marketing/ad/render.mjs          # video first
node marketing/ad/voiceover.mjs       # default voice: "Ava (Premium)"
node marketing/ad/voiceover.mjs --voice "Serena (Premium)" --out out/sentinal-ad-serena.mp4
```

Each line in `SCRIPT` starts with its scene and must finish before the next
one; a line that would overrun is spoken slightly faster (175–195 wpm) and the
script prints a warning. The mix is normalized to -16 LUFS.

## Design-tool films (`sentinal-videos/`)

`render-dc.mjs` renders the `.dc.html` films locally, without the editor's
export. It serves the folder over HTTP, seeks the engine's stage frame by frame
through its own `data-om-seek-to-time-frame` export event, and captures each
frame at 2× (3840×2160) before a lanczos downscale, so text and edges stay crisp.

```sh
node marketing/ad/render-dc.mjs "sentinal-videos/Sentinal Film.dc.html"            # 1080p60
node marketing/ad/render-dc.mjs "sentinal-videos/Sentinal Film - Keynote cut.dc.html"
node marketing/ad/render-dc.mjs "sentinal-videos/Sentinal Film.dc.html" --4k       # 2160p60
node marketing/ad/render-dc.mjs "sentinal-videos/Sentinal Film.dc.html" --stills 10,30
node marketing/ad/voiceover.mjs --in out/sentinal-film.mp4 --out out/sentinal-film-voiceover.mp4 \
  --script "sentinal-videos/voice-over script.md" --rate 160
```

The runtime loads React and Babel from unpkg and fonts from Google Fonts, so the
first render needs network access. Each 60 s film takes about five minutes.
