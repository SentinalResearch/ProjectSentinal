// Sentinal — "keynote" cut. Black stage, one idea per beat, product floating in space.
const E = Easing;
const cl = (v, a = 0, b = 1) => Math.max(a, Math.min(b, v));
const lerp = (a, b, t) => a + (b - a) * t;
const MONO = "'JetBrains Mono', ui-monospace, monospace";
const SANS = "'Archivo', system-ui, sans-serif";
const ST = { RUNNING: '#29D35F', WAITING: '#FFD000', STALLED: '#FF8A00', FAILED: '#FF3B2F', COMPLETED: '#36C5E8', IDLE: '#9C9C98', HEALTHY: '#29D35F' };
const FMT = { '16:9': { w: 1920, h: 1080, ts: 1, ds: 1 }, '1:1': { w: 1080, h: 1080, ts: 0.74, ds: 0.68 }, '9:16': { w: 1080, h: 1920, ts: 0.84, ds: 0.8 } };
function hx(h) { h = h.replace('#', ''); return [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16)); }
function mix(a, b, t) { const A = hx(a), B = hx(b); return 'rgb(' + A.map((v, i) => Math.round(lerp(v, B[i], t))).join(',') + ')'; }

function Glyph({ s, size = 12, color = '#0A0A0A' }) {
  const st = { stroke: color, strokeWidth: 2, fill: 'none' };
  let g;
  if (s === 'RUNNING') g = <circle cx="7" cy="7" r="4.5" fill={color} />;
  else if (s === 'FAILED') g = <path d="M3 3l8 8M11 3l-8 8" {...st} />;
  else if (s === 'WAITING') g = <g><path d="M4.6 4.9a2.4 2.4 0 1 1 3.4 2.2c-.7.4-1 .8-1 1.5v.4" {...st} /><rect x="6" y="10.5" width="2" height="2" fill={color} /></g>;
  else if (s === 'STALLED') g = <path d="M4.8 3v8M9.2 3v8" {...st} />;
  else if (s === 'COMPLETED' || s === 'HEALTHY') g = <path d="M2.6 7.2l3 3 5.8-6.4" {...st} />;
  else g = <path d="M2.5 7h9" {...st} />;
  return <svg width={size} height={size} viewBox="0 0 14 14" style={{ flex: 'none', display: 'block' }}>{g}</svg>;
}
function Badge({ s, h = 24, fs = 11 }) {
  return <span style={{ display: 'inline-flex', alignItems: 'center', gap: fs * 0.5, height: h, padding: `0 ${h * 0.36}px 0 ${h * 0.3}px`, background: ST[s], color: '#0A0A0A', fontFamily: SANS, fontSize: fs, fontWeight: 700, letterSpacing: '0.04em', whiteSpace: 'nowrap' }}><Glyph s={s} size={fs + 1} />{s}</span>;
}
function Mark({ size, ink = '#F2F2F0', dot = '#3D5AFE', draw = 1, pop = 1 }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" style={{ display: 'block', overflow: 'visible' }}>
    <path d="M5 22L13.5 2" pathLength="1" stroke={ink} strokeWidth="3.4" strokeLinecap="square" style={{ strokeDasharray: 1, strokeDashoffset: 1 - draw }} />
    <circle cx="18" cy="18.5" r="3.4" fill={dot} style={{ transformOrigin: '18px 18.5px', transform: `scale(${pop})` }} />
  </svg>;
}
function Caret({ c = '#D4D4D0' }) { return <span style={{ display: 'inline-block', width: '0.55em', height: '1em', background: c, verticalAlign: '-0.15em', marginLeft: 2 }} />; }

// One line of type: fades up out of a soft blur, leaves the same way.
function Say({ T, a, b, x, y, w, size, color = '#F2F2F0', mono, align = 'center', children }) {
  const i = E.easeOutCubic(cl((T - a) / 0.9)), o = E.easeInOutCubic(cl((T - (b - 0.6)) / 0.6)), op = i * (1 - o);
  if (op <= 0.001) return null;
  const left = align === 'center' ? x - w / 2 : x;
  return <div style={{ position: 'absolute', left, top: y, width: w, textAlign: align, opacity: op, transform: `translateY(calc(-50% + ${(1 - i) * 26 - o * 14}px))`, filter: `blur(${(1 - i) * 10 + o * 6}px)`, fontFamily: mono ? MONO : SANS, fontSize: size, lineHeight: mono ? 1.4 : 1.02, fontWeight: mono ? 500 : 800, letterSpacing: mono ? 0 : '-0.045em', color, textWrap: 'balance' }}>{children}</div>;
}

// ——— Laptop screen (1440×900) ———
const RTX = [['RUNNING', 'Qwen Training', 'python train.py --config moe.yaml', 'epoch 68/100 · loss 0.37'], ['RUNNING', 'vLLM', 'vllm serve Qwen/Qwen3-8B --port 8000', 'server on :8000'], ['FAILED', 'pytest', 'pytest tests/', '3 failed · 182 passed'], ['IDLE', 'shell', '~/Projects/TIM', '']];
const MINI = [['STALLED', 'Compression', 'no output 14m', 'CPU 0 % · possibly stalled'], ['WAITING', 'npm install', 'Continue? [y/N]', 'waiting for input']];
const NEEDS = [['FAILED', 'RTX-PC · pytest', '3 failed · 182 passed · 42s'], ['WAITING', 'Mac Mini · npm install', 'Continue? [y/N]'], ['STALLED', 'Mac Mini · Compression', 'no output 14m · CPU 0 %']];
function Screen({ lift }) {
  const off = lift * 170;
  const row = (r, y, k) => <div key={k} style={{ position: 'absolute', left: 40, top: y + off, width: 1120, height: 56, display: 'flex', alignItems: 'center', gap: 16, borderBottom: '1px solid #262626' }}>
    <div style={{ width: 130, flex: 'none' }}><Badge s={r[0]} /></div>
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 2 }}><span style={{ fontSize: 15, fontWeight: 700 }}>{r[1]}</span><span style={{ fontFamily: MONO, fontSize: 12, color: '#8A8A86' }}>{r[2]}</span></div>
    <div style={{ width: 300, fontSize: 14, color: '#A6A6A2' }}>{r[3]}</div></div>;
  const head = (y, n, spec, on) => <div style={{ position: 'absolute', left: 40, top: y + off, width: 1120, height: 40, display: 'flex', alignItems: 'center', gap: 12, borderBottom: `2px solid ${on ? '#F2F2F0' : '#262626'}` }}>
    <span style={{ fontSize: 20, fontWeight: 800, letterSpacing: '-0.02em', color: on ? '#F2F2F0' : '#8A8A86' }}>{n}</span><span style={{ fontSize: 13, color: '#8A8A86' }}>{spec}</span>
    <span style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: 12, color: on ? ST.RUNNING : '#8A8A86' }}>{on ? 'ONLINE' : 'OFFLINE · last seen 14m ago'}</span></div>;
  return <div style={{ position: 'absolute', inset: 0, background: '#0A0A0A', color: '#F2F2F0', fontFamily: SANS }}>
    <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: 240, background: '#111111', borderRight: '1px solid #262626' }}>
      <div style={{ position: 'absolute', left: 22, top: 26, display: 'flex', alignItems: 'center', gap: 10, fontWeight: 800, fontSize: 18 }}><Mark size={20} />Sentinal</div>
      {['Overview', 'Split view', 'Projects', 'History', 'Recipes'].map((n, i) => <div key={n} style={{ position: 'absolute', left: 12, top: 84 + i * 36, width: 216, height: 36, padding: '0 12px', display: 'flex', alignItems: 'center', fontSize: 14, fontWeight: i ? 500 : 700, color: i ? '#A6A6A2' : '#F2F2F0', background: i ? 'transparent' : '#1C1C1C' }}>{n}</div>)}
      <div style={{ position: 'absolute', left: 24, top: 294, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: '#8A8A86' }}>MACHINES</div>
      {[['RTX-PC', 1], ['Mac Mini', 1], ['MacBook', 0]].map(([n, on], i) => <div key={n} style={{ position: 'absolute', left: 24, top: 320 + i * 34, display: 'flex', alignItems: 'center', gap: 10, fontSize: 14, color: on ? '#F2F2F0' : '#8A8A86' }}><span style={{ width: 8, height: 8, background: on ? ST.RUNNING : 'transparent', boxShadow: on ? 'none' : 'inset 0 0 0 1.5px #8A8A86' }} />{n}</div>)}
    </div>
    <div style={{ position: 'absolute', left: 240, top: 0, width: 1200, height: 900 }}>
      <div style={{ position: 'absolute', left: 40, top: 30, display: 'flex', alignItems: 'baseline', gap: 14 }}><span style={{ fontSize: 32, fontWeight: 800, letterSpacing: '-0.03em' }}>Overview</span><span style={{ fontSize: 14, color: '#8A8A86' }}>3 machines · 6 sessions</span></div>
      <div style={{ position: 'absolute', left: 40, top: 96, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: '#8A8A86', opacity: lift }}>NEEDS YOU · 3</div>
      {NEEDS.map((n, i) => <div key={i} style={{ position: 'absolute', left: 40 + i * 378, top: 124, width: 362, height: 128, background: '#111111', boxShadow: `inset 0 3px 0 ${ST[n[0]]}`, padding: 18, display: 'flex', flexDirection: 'column', gap: 8, opacity: cl(lift * 1.4 - i * 0.2), transform: `translateY(${(1 - lift) * 40}px)` }}><Badge s={n[0]} /><span style={{ fontSize: 17, fontWeight: 700 }}>{n[1]}</span><span style={{ fontFamily: MONO, fontSize: 12, color: '#A6A6A2' }}>{n[2]}</span></div>)}
      {head(110, 'RTX-PC', 'Linux · Ryzen 9 · 64 GB · RTX 4090 24 GB', true)}
      {RTX.map((r, i) => row(r, 150 + i * 56, 'r' + i))}
      {head(400, 'Mac Mini', 'macOS · M2 Pro · 32 GB', true)}
      {MINI.map((r, i) => row(r, 440 + i * 56, 'm' + i))}
      {head(580, 'MacBook', 'macOS · M3 · 16 GB', false)}
    </div>
  </div>;
}
function Laptop({ w, children }) {
  const pad = w * 0.022, s = w / 1440, W = w + pad * 2;
  return <div style={{ width: W }}>
    <div style={{ width: W, height: 900 * s + pad * 2, background: '#141414', borderRadius: pad * 1.3, boxShadow: 'inset 0 0 0 1.5px #2E2E2E, 0 90px 180px -50px rgba(0,0,0,.95)', padding: pad }}>
      <div style={{ width: w, height: 900 * s, overflow: 'hidden', position: 'relative' }}><div style={{ position: 'absolute', left: 0, top: 0, width: 1440, height: 900, transformOrigin: '0 0', transform: `scale(${s})` }}>{children}</div></div>
    </div>
    <div style={{ width: W * 1.16, marginLeft: -W * 0.08, height: w * 0.026, background: 'linear-gradient(#3A3A3A,#141414)', borderRadius: `0 0 ${w * 0.03}px ${w * 0.03}px` }} />
  </div>;
}

// ——— Phone (390×844) ———
function Phone({ t, T }) {
  const drop = E.easeOutBack(cl((t - 1.6) / 0.6)), tap = cl((t - 3.2) / 0.5), B = E.easeOutCubic(cl((t - 3.5) / 0.5));
  const tapY = cl((t - 5.4) / 0.5), answered = t >= 5.7, toast = E.easeOutCubic(cl((t - 5.8) / 0.4)) * (1 - cl((t - 8.6) / 0.4));
  const ring = (p, c) => p > 0 && p < 1 ? <div style={{ position: 'absolute', left: '50%', top: '50%', width: 60, height: 60, margin: -30, borderRadius: 30, background: c, transform: `scale(${0.3 + p * 1.6})`, opacity: 1 - p }} /> : null;
  return <div style={{ position: 'relative', width: 390, height: 844, fontFamily: SANS, color: '#F2F2F0' }}>
    <div style={{ position: 'absolute', inset: 0, background: '#1A1A1A', borderRadius: 60, boxShadow: 'inset 0 0 0 1.5px #3A3A3A, 0 80px 160px -40px rgba(0,0,0,.95)' }} />
    <div style={{ position: 'absolute', left: 12, top: 12, width: 366, height: 820, borderRadius: 48, background: '#0A0A0A', overflow: 'hidden' }}>
      <div style={{ position: 'absolute', left: 30, right: 30, top: 16, display: 'flex', justifyContent: 'space-between', fontSize: 15, fontWeight: 700 }}><span>2:04</span><span style={{ width: 26, height: 12, boxShadow: 'inset 0 0 0 1.5px #F2F2F0' }} /></div>
      <div style={{ opacity: 1 - B }}>
        <div style={{ position: 'absolute', left: 0, right: 0, top: 104, textAlign: 'center', fontSize: 96, fontWeight: 800, letterSpacing: '-0.05em' }}>2:04</div>
        <div style={{ position: 'absolute', left: 0, right: 0, top: 214, textAlign: 'center', fontSize: 16, color: '#A6A6A2' }}>Thursday</div>
        <div style={{ position: 'absolute', left: 12, right: 12, top: lerp(-150, 270, drop), padding: 14, background: '#1C1C1C', borderRadius: 20, display: 'flex', gap: 12, boxShadow: '0 0 0 1px #2A2A2A' }}>
          <div style={{ width: 40, height: 40, flex: 'none', borderRadius: 10, background: '#000', display: 'flex', alignItems: 'center', justifyContent: 'center', boxShadow: '0 0 0 1px #333' }}><Mark size={24} /></div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}><div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, fontWeight: 700, letterSpacing: '0.06em', color: '#A6A6A2' }}><span>SENTINAL</span><span style={{ fontWeight: 500, letterSpacing: 0 }}>now</span></div>
            <span style={{ fontSize: 15, lineHeight: '20px', fontWeight: 600 }}>Mac Mini · npm install: waiting for input — <span style={{ fontFamily: MONO, fontSize: 14 }}>Continue? [y/N]</span></span></div>
          {ring(tap, 'rgba(242,242,240,.35)')}
        </div>
      </div>
      <div style={{ position: 'absolute', inset: 0, opacity: B, transform: `translateY(${(1 - B) * 30}px)` }}>
        <div style={{ position: 'absolute', left: 22, top: 108, fontFamily: MONO, fontSize: 13, color: '#8A8A86' }}>MAC MINI</div>
        <div style={{ position: 'absolute', left: 22, top: 130, fontSize: 32, fontWeight: 800, letterSpacing: '-0.03em' }}>npm install</div>
        <div style={{ position: 'absolute', left: 22, top: 180 }}><Badge s={answered ? 'RUNNING' : 'WAITING'} h={28} fs={12} /></div>
        <div style={{ position: 'absolute', left: 22, right: 22, top: 256, height: 110, background: '#0C0C0C', boxShadow: '0 0 0 1px #2A2A2A', display: 'flex', alignItems: 'center', padding: '0 20px', fontFamily: MONO, fontSize: 28 }}>Continue? [y/N]{answered && <span style={{ color: ST.RUNNING }}>&nbsp;y</span>}</div>
        <div style={{ position: 'absolute', left: 22, right: 22, top: 390, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div style={{ position: 'relative', height: 76, background: '#F2F2F0', color: '#0A0A0A', display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: MONO, fontSize: 34, fontWeight: 700, transform: `scale(${tapY > 0 && tapY < 0.5 ? 0.95 : 1})` }}>y{ring(tapY, 'rgba(61,90,254,.45)')}</div>
          <div style={{ height: 76, boxShadow: 'inset 0 0 0 1.5px #6B6B67', display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: MONO, fontSize: 34, fontWeight: 700 }}>n</div>
        </div>
        <div style={{ position: 'absolute', left: 22, right: 22, top: 480, height: 56, background: '#3D5AFE', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 16, fontWeight: 700 }}>Open terminal</div>
        <div style={{ position: 'absolute', left: 22, right: 22, bottom: 40, height: 52, background: '#1C1C1C', display: 'flex', alignItems: 'center', gap: 10, padding: '0 16px', fontSize: 14, opacity: toast }}><Glyph s="COMPLETED" size={14} color={ST.RUNNING} />Sent <span style={{ fontFamily: MONO }}>y</span> to npm install</div>
      </div>
    </div>
  </div>;
}

const PANES = [
  ['RUNNING', 'Qwen Training', 'RTX-PC', ['$ python train.py --config moe.yaml', 'epoch 66/100  loss 0.39', 'epoch 67/100  loss 0.38', 'epoch 68/100  loss 0.37']],
  ['RUNNING', 'vLLM', 'RTX-PC', ['$ vllm serve Qwen/Qwen3-8B --port 8000', 'INFO  Started server process', 'INFO  Uvicorn running on :8000']],
  ['FAILED', 'pytest', 'RTX-PC', ['$ pytest tests/', ['3 failed, 182 passed in 42s', ST.FAILED], 'molecule correspondence tests']],
  ['RUNNING', 'npm install', 'Mac Mini', ['$ npm install', ['Continue? [y/N] y', ST.RUNNING], 'added 1,284 packages']],
];
const LOSS = [0.41, 0.407, 0.401, 0.403, 0.395, 0.39, 0.392, 0.385, 0.381, 0.383, 0.377, 0.374, 0.37];

function Piece({ F }) {
  const { T, CUES } = useComposition();
  const P = (s, e, ease = E.easeOutCubic) => ease(cl((T - s) / (e - s)));
  const { w, h, ts, ds } = F, cx = w / 2, cy = h / 2, wide = w > h;
  const cN = CUES.Night, cM = CUES.Mark, cS = CUES.Machines, cG = CUES.Going, cA = CUES.Reach, cK = CUES.Terminal, cE = CUES.End;
  const blink = Math.floor(T * 2.2) % 2 === 0;

  // 1–2 · dot → mark
  const S = 260 * ts, toMark = P(cM, cM + 1.3, E.easeInOutCubic);
  const dOff = { x: (6 / 24) * S, y: (6.5 / 24) * S };
  const gx = lerp(cx - dOff.x, cx, toMark), gy = lerp(cy - dOff.y, cy - 60 * ts, toMark);
  const dotIn = P(cN + 0.2, cN + 0.8, E.easeOutBack), pulse = 1 + 0.07 * Math.sin(T * 3.2) * (1 - toMark);
  const markOut = P(cM + 4.3, cM + 5, E.easeInOutCubic), markOp = (T < cM + 5 ? 1 : 0) * (1 - markOut);
  const dotC = mix(ST.RUNNING, '#3D5AFE', toMark);
  const lineY = cy + S * 0.72;

  // 3 · laptop
  const lw = 1180 * ds, ls = lw / 1440, lin = P(cS + 2.0, cS + 3.8), lout = P(cS + 10.2, cS + 11, E.easeInOutCubic);
  const push = P(cS + 6.2, cS + 8.0, E.easeInOutCubic), Z = lerp(1, 1.9, push);
  const lrx = lerp(40, 6, lin) - push * 6, lry = lerp(-20, 0, lin) + P(cS + 3.8, cS + 11, x => x) * -3;
  const ltx = -120 * ls * Z * push, lty = (1 - lin) * 320 + 290 * ls * Z * push;
  const lOp = lin * (1 - lout) * (T < cS + 11 ? 1 : 0);
  const lH = (900 * ls + lw * 0.044) + lw * 0.026;

  // 4 · going
  const gIn = P(cG + 0.2, cG + 0.9), gOut = P(cG + 3.6, cG + 4.2, E.easeInOutCubic), cnt = P(cG + 0.4, cG + 2.6, E.easeInOutCubic);
  const lossIn = P(cG + 4.0, cG + 4.6), lossOut = P(cG + 6.4, cG + 7.0, E.easeInOutCubic), lossDraw = P(cG + 4.2, cG + 6.2, E.easeInOutCubic);
  const lwid = w * (wide ? 0.72 : 0.82), lht = 300 * ts;
  const lpts = LOSS.map((v, i) => [(i / (LOSS.length - 1)) * lwid, ((v - 0.37) / 0.04) * lht]);
  const lpath = lpts.map((p, i) => (i ? 'L' : 'M') + p[0].toFixed(1) + ' ' + (lht - p[1]).toFixed(1)).join(' ');

  // 5 · phone
  const ps = Math.min((h * (wide ? 0.8 : 0.56)) / 844, (w * 0.5) / 390), pin = P(cA + 0.2, cA + 1.5), pout = P(cA + 9.2, cA + 10, E.easeInOutCubic);
  const px = wide ? cx + 330 : cx, py = wide ? cy : cy + 150 * ts;
  const pOp = pin * (1 - pout) * (T >= cA ? 1 : 0) * (T < cA + 10 ? 1 : 0);

  // 6 · terminals
  const pw = Math.min(640 * ds / (wide ? 1 : 0.9), (w - 120) / 2 - 12), ph = pw * 0.56, gap = 20;
  const flat = P(cK + 3.4, cK + 5.2, E.easeInOutCubic), kOut = P(cK + 8.3, cK + 9, E.easeInOutCubic);
  const kOn = T >= cK && T < cK + 9;

  // 7 · end
  const eD = P(cE + 0.3, cE + 1.1, E.easeInOutCubic), eP = P(cE + 1.0, cE + 1.4, E.easeOutBack);
  const eW = P(cE + 1.3, cE + 2.1), eT = P(cE + 2.3, cE + 3.0), eS = P(cE + 3.6, cE + 4.2), eG = P(cE + 4.4, cE + 5.0), fade = P(cE + 8.2, cE + 9, E.easeInOutCubic);
  const eOn = T >= cE, eScale = 1 + P(cE, cE + 9, x => x) * 0.05;

  const glow = P(cS + 2, cS + 4) * (1 - P(cE + 7.5, cE + 9)) * 0.5;
  const big = 124 * ts, mid = 76 * ts;

  return <div data-screen-label={`t=${Math.floor(T)}s`} style={{ position: 'absolute', inset: 0, background: '#000', overflow: 'hidden', fontFamily: SANS, color: '#F2F2F0' }}>
    <div style={{ position: 'absolute', left: cx - w, top: cy - h * 0.6, width: w * 2, height: h * 1.4, background: 'radial-gradient(closest-side, rgba(61,90,254,.35), rgba(61,90,254,0))', opacity: glow }} />

    {/* 1–2 */}
    {markOp > 0.001 && <div style={{ position: 'absolute', left: gx - S / 2, top: gy - S / 2, opacity: markOp, transform: `scale(${1 - markOut * 0.1})` }}>
      <Mark size={S} draw={P(cM + 0.7, cM + 1.8, E.easeInOutCubic)} pop={dotIn * pulse} dot={dotC} />
    </div>}
    <Say T={T} a={cN + 1.0} b={cN + 3.2} x={cx} y={lineY} w={w * 0.9} size={mid}>It's 2 a.m.</Say>
    <Say T={T} a={cN + 3.2} b={cN + 6.0} x={cx} y={lineY} w={w * 0.9} size={mid} color="#A6A6A2">Your model is still training.</Say>
    <Say T={T} a={cM + 1.8} b={cM + 4.9} x={cx} y={lineY} w={w * 0.9} size={mid}>Sentinal keeps watch.</Say>

    {/* 3 */}
    <Say T={T} a={cS + 0.2} b={cS + 2.5} x={cx} y={cy - big * 0.55} w={w * 0.9} size={big}>Every session.</Say>
    <Say T={T} a={cS + 0.8} b={cS + 2.5} x={cx} y={cy + big * 0.55} w={w * 0.9} size={big} color="#8C9EFF">Every machine.</Say>
    {lOp > 0.001 && <div style={{ position: 'absolute', left: cx - lw * 0.522, top: cy - lH / 2 - 20 * ts, opacity: lOp, transform: `translate(${ltx}px,${lty}px) perspective(2600px) rotateX(${lrx}deg) rotateY(${lry}deg) scale(${Z})`, transformOrigin: '50% 50%' }}>
      <Laptop w={lw}><Screen lift={P(cS + 4.4, cS + 5.6, E.easeInOutCubic)} /></Laptop>
    </div>}
    <div style={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: h * 0.34, background: 'linear-gradient(transparent, #000 70%)', opacity: push * (1 - lout) }} />
    <Say T={T} a={cS + 7.6} b={cS + 10.8} x={cx} y={h - 120 * ts} w={w * 0.9} size={64 * ts}>What needs you comes first.</Say>

    {/* 4 */}
    {gIn * (1 - gOut) > 0.001 && T < cG + 4.3 && <div style={{ position: 'absolute', inset: 0, opacity: gIn * (1 - gOut), filter: `blur(${gOut * 8}px)` }}>
      <div style={{ position: 'absolute', left: 0, right: 0, top: cy - 230 * ts, textAlign: 'center', fontSize: 340 * ts, lineHeight: 1, fontWeight: 800, letterSpacing: '-0.06em' }}>{Math.round(68 * cnt)}<span style={{ fontSize: 150 * ts, color: '#8A8A86' }}> %</span></div>
      <div style={{ position: 'absolute', left: cx - lwid / 2, top: cy + 150 * ts, width: lwid, height: 4, background: '#1C1C1C' }}><div style={{ width: `${68 * cnt}%`, height: '100%', background: '#3D5AFE' }} /></div>
      <div style={{ position: 'absolute', left: 0, right: 0, top: cy + 190 * ts, textAlign: 'center', fontFamily: MONO, fontSize: 24 * ts, color: '#A6A6A2' }}>Qwen Training · epoch {Math.round(lerp(1, 68, cnt))}/100</div>
    </div>}
    {lossIn * (1 - lossOut) > 0.001 && <div style={{ position: 'absolute', inset: 0, opacity: lossIn * (1 - lossOut), filter: `blur(${lossOut * 8}px)` }}>
      <div style={{ position: 'absolute', left: cx - lwid / 2, top: cy - lht / 2 - 40 * ts }}>
        <svg width={lwid} height={lht} style={{ display: 'block', overflow: 'visible' }}><path d={lpath} pathLength="1" fill="none" stroke="#8C9EFF" strokeWidth={6 * ts} strokeLinejoin="round" style={{ strokeDasharray: 1, strokeDashoffset: 1 - lossDraw }} /></svg>
        <span style={{ position: 'absolute', left: 0, top: -64 * ts, fontFamily: MONO, fontSize: 40 * ts, color: '#8A8A86' }}>0.41</span>
        <span style={{ position: 'absolute', right: 0, top: lht + 16 * ts, fontFamily: MONO, fontSize: 56 * ts, fontWeight: 700, color: '#F2F2F0', opacity: P(cG + 5.9, cG + 6.3) }}>0.37</span>
      </div>
      <div style={{ position: 'absolute', left: 0, right: 0, top: cy + lht / 2 + 120 * ts, textAlign: 'center', fontFamily: MONO, fontSize: 24 * ts, color: '#A6A6A2' }}>loss improving for 5 checks</div>
    </div>}
    {T >= cG + 6.8 && T < cG + 10 && <div style={{ position: 'absolute', left: 0, right: 0, top: cy - 200 * ts, display: 'flex', justifyContent: 'center', opacity: P(cG + 6.9, cG + 7.5) * (1 - P(cG + 9.3, cG + 9.9)) }}><Badge s="HEALTHY" h={48 * ts} fs={22 * ts} /></div>}
    <Say T={T} a={cG + 7.1} b={cG + 9.9} x={cx} y={cy} w={w * 0.9} size={big}>Know how it's going.</Say>
    <Say T={T} a={cG + 7.7} b={cG + 9.9} x={cx} y={cy + 130 * ts} w={w * 0.9} size={24 * ts} mono color="#A6A6A2">GPU 96 % · VRAM 21.4 / 24 GB · last output 8s ago</Say>

    {/* 5 */}
    {pOp > 0.001 && <div style={{ position: 'absolute', left: px - 195, top: py - 422, width: 390, height: 844, opacity: pOp, transform: `translateY(${(1 - pin) * 260 - pout * 80}px) perspective(2200px) rotateY(${lerp(-26, -8, pin) + P(cA + 1.5, cA + 10, x => x) * 6}deg) rotateX(${lerp(24, 4, pin)}deg) scale(${ps})` }}><Phone t={T - cA} T={T} /></div>}
    <Say T={T} a={cA + 1.2} b={cA + 9.6} x={wide ? cx - 800 : cx} y={wide ? cy - 30 : 250 * ts} w={wide ? 820 : w * 0.9} size={wide ? big : 96 * ts} align={wide ? 'left' : 'center'}>Answer from anywhere.</Say>
    <Say T={T} a={cA + 2.0} b={cA + 9.6} x={wide ? cx - 800 : cx} y={wide ? cy + 150 : 250 * ts + 130 * ts} w={wide ? 820 : w * 0.9} size={24 * ts} mono color="#A6A6A2" align={wide ? 'left' : 'center'}>Mac Mini · npm install · Continue? [y/N]</Say>

    {/* 6 */}
    <Say T={T} a={cK + 0.3} b={cK + 3.0} x={cx} y={cy} w={w * 0.9} size={big}>The real terminal.</Say>
    {kOn && <div style={{ position: 'absolute', left: cx - pw - gap / 2, top: cy - ph - gap / 2 - 30 * ts, width: pw * 2 + gap, height: ph * 2 + gap, opacity: 1 - kOut, transform: `perspective(2400px) rotateX(${lerp(34, 0, flat)}deg) rotateZ(${lerp(-10, 0, flat)}deg) scale(${lerp(0.9, 1, flat) + kOut * 0.04})`, display: 'grid', gridTemplateColumns: `${pw}px ${pw}px`, gridTemplateRows: `${ph}px ${ph}px`, gap }}>
      {PANES.map((p, i) => { const a = P(cK + 2.4 + i * 0.25, cK + 3.4 + i * 0.25); const fs = pw / 38;
        return <div key={i} style={{ background: '#0C0C0C', boxShadow: `0 0 0 1px ${i === 0 ? '#3D5AFE' : '#2A2A2A'}, 0 40px 80px -30px rgba(0,0,0,.9)`, opacity: a, transform: `translateY(${(1 - a) * 120}px)`, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
          <div style={{ height: fs * 2.4, flex: 'none', display: 'flex', alignItems: 'center', gap: fs * 0.6, padding: `0 ${fs}px`, background: '#111111', borderBottom: '1px solid #262626' }}><Badge s={p[0]} h={fs * 1.6} fs={fs * 0.7} /><span style={{ fontSize: fs, fontWeight: 700 }}>{p[1]}</span><span style={{ fontFamily: MONO, fontSize: fs * 0.8, color: '#8A8A86' }}>{p[2]}</span></div>
          <div style={{ padding: fs, fontFamily: MONO, fontSize: fs, lineHeight: 1.6, color: '#D4D4D0', whiteSpace: 'pre' }}>
            {p[3].map((l, j) => { const L = typeof l === 'string' ? [l] : l; return <div key={j} style={{ color: L[1] || (j === 0 ? '#F2F2F0' : '#A6A6A2') }}>{L[0]}</div>; })}
            {i === 0 && blink && <Caret />}
          </div>
        </div>; })}
    </div>}
    <Say T={T} a={cK + 5.2} b={cK + 8.8} x={cx} y={h - 90 * ts} w={w * 0.9} size={56 * ts}>One tap away.</Say>

    {/* 7 */}
    {eOn && <div style={{ position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 34 * ts, transform: `scale(${eScale})`, textAlign: 'center', padding: '0 60px' }}>
      <Mark size={170 * ts} draw={eD} pop={eP} />
      <span style={{ fontSize: 150 * ts, lineHeight: 0.9, fontWeight: 800, letterSpacing: '-0.06em', opacity: eW, transform: `translateY(${(1 - eW) * 30}px)`, filter: `blur(${(1 - eW) * 10}px)` }}>Sentinal</span>
      <span style={{ fontSize: 40 * ts, lineHeight: 1.3, color: '#A6A6A2', opacity: eT, maxWidth: 1000 * ts, textWrap: 'balance' }}>Every terminal on every machine, at a glance.</span>
      <span style={{ marginTop: 20 * ts, fontSize: 56 * ts, fontWeight: 800, letterSpacing: '-0.04em', color: '#8C9EFF', opacity: eS, transform: `translateY(${(1 - eS) * 16}px)` }}>Coming soon.</span>
      <span style={{ fontFamily: MONO, fontSize: 24 * ts, color: '#8A8A86', opacity: eG }}>github.com/SentinalResearch</span>
    </div>}
    <div style={{ position: 'absolute', inset: 0, background: '#000', opacity: fade, pointerEvents: 'none' }} />
  </div>;
}

function SentinalKeynote(props) {
  const fmt = props.format || '16:9', F = FMT[fmt] || FMT['16:9'];
  return <CompositionStage key={fmt} width={F.w} height={F.h} scenes={window.OM_SCENES} playback={window.OM_PLAYBACK} bg="#000000"><Piece F={F} /></CompositionStage>;
}
window.SentinalKeynote = SentinalKeynote;
