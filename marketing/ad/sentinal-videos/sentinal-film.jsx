// Sentinal — 60s product film. One composition, three formats.
const M = { enter: Easing.easeOutCubic, draw: Easing.easeInOutCubic, pop: Easing.easeOutBack };
const cl = (v, a = 0, b = 1) => Math.max(a, Math.min(b, v));
const lerp = (a, b, t) => a + (b - a) * t;
const MONO = "'JetBrains Mono', ui-monospace, monospace";
const SANS = "'Archivo', system-ui, sans-serif";
function hx(h) { h = h.replace('#', ''); return [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16)); }
function mix(a, b, t) { const A = hx(a), B = hx(b); return 'rgb(' + A.map((v, i) => Math.round(lerp(v, B[i], t))).join(',') + ')'; }

const DARK = { bg0: '#0A0A0A', bg1: '#111111', bg2: '#1C1C1C', t1: '#F2F2F0', t2: '#A6A6A2', t3: '#8A8A86', line: '#262626', lineS: '#6B6B67', acc: '#3D5AFE', accFg: '#8C9EFF' };
const LIGHT = { bg0: '#F7F6F2', bg1: '#EFEEE9', bg2: '#E4E3DD', t1: '#0A0A0A', t2: '#4A4A46', t3: '#5E5E5A', line: '#D9D8D2', lineS: '#8A8984', acc: '#2A44E0', accFg: '#2A44E0' };
const ST = { RUNNING: '#29D35F', WAITING: '#FFD000', STALLED: '#FF8A00', FAILED: '#FF3B2F', COMPLETED: '#36C5E8', IDLE: '#9C9C98', HEALTHY: '#29D35F' };

const FMT = {
  '16:9': { w: 1920, h: 1080, hl: { x: 120, y: 84, fs: 76, mw: 1680 }, mock: { x: 120, y: 236, w: 1680, h: 790 }, zb: 1, term: { x: 120, y: 110, w: 1680, h: 860 }, cols: 2, tfs: 20, big: 124, logo: 150, stack: false },
  '1:1': { w: 1080, h: 1080, hl: { x: 72, y: 72, fs: 54, mw: 936 }, mock: { x: 72, y: 206, w: 936, h: 810 }, zb: 1.1, term: { x: 72, y: 72, w: 936, h: 936 }, cols: 2, tfs: 15, big: 92, logo: 130, stack: true },
  '9:16': { w: 1080, h: 1920, hl: { x: 72, y: 170, fs: 88, mw: 936 }, mock: { x: 72, y: 560, w: 936, h: 1220 }, zb: 2, term: { x: 72, y: 160, w: 936, h: 1600 }, cols: 1, tfs: 20, big: 110, logo: 170, stack: true },
};

function typed(T, s, at, cps) { return s.slice(0, cl(Math.floor((T - at) * cps), 0, s.length)); }
function Caret({ h = '1em', c = '#D4D4D0' }) { return <span style={{ display: 'inline-block', width: '0.55em', height: h, background: c, verticalAlign: '-0.15em', marginLeft: 2 }} />; }

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
  return <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, height: h, padding: `0 ${h * 0.36}px 0 ${h * 0.3}px`, background: ST[s], color: '#0A0A0A', fontFamily: SANS, fontSize: fs, fontWeight: 700, letterSpacing: '0.04em', whiteSpace: 'nowrap' }}><Glyph s={s} size={fs + 1} />{s}</span>;
}
function Mark({ size = 20, ink = '#F2F2F0', dot = '#3D5AFE', draw = 1, pop = 1 }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" style={{ display: 'block', overflow: 'visible' }}>
    <path d="M5 22L13.5 2" pathLength="1" stroke={ink} strokeWidth="3.4" strokeLinecap="square" style={{ strokeDasharray: 1, strokeDashoffset: 1 - draw }} />
    <circle cx="18" cy="18.5" r="3.4" fill={dot} style={{ transformOrigin: '18px 18.5px', transform: `scale(${pop})` }} />
  </svg>;
}

// Fit a 1440x900 mock into a rect, with a camera (zoom + focus in mock coords).
function Viewport({ rect, zoom, fx, fy, mw = 1440, mh = 900, opacity = 1, dy = 0, children }) {
  const s = Math.min(rect.w / mw, rect.h / mh) * zoom, cw = mw * s, ch = mh * s;
  let ox = rect.w / 2 - fx * s, oy = rect.h / 2 - fy * s;
  ox = cw > rect.w ? cl(ox, rect.w - cw, 0) : (rect.w - cw) / 2;
  oy = ch > rect.h ? cl(oy, rect.h - ch, 0) : (rect.h - ch) / 2;
  return <div style={{ position: 'absolute', left: rect.x, top: rect.y + dy, width: rect.w, height: rect.h, overflow: 'hidden', opacity, visibility: opacity > 0.001 ? 'visible' : 'hidden' }}>
    <div style={{ position: 'absolute', left: 0, top: 0, width: mw, height: mh, transformOrigin: '0 0', transform: `translate(${ox}px,${oy}px) scale(${s})` }}>{children}</div>
  </div>;
}
function WinFrame({ th, url, children }) {
  return <div style={{ position: 'absolute', inset: 0, background: th.bg0, color: th.t1, fontFamily: SANS, boxShadow: `inset 0 0 0 1px ${th.lineS}`, overflow: 'hidden' }}>
    <div style={{ position: 'absolute', left: 0, top: 0, right: 0, height: 44, background: th.bg1, borderBottom: `1px solid ${th.line}`, display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: MONO, fontSize: 13, color: th.t3 }}>{url}</div>
    {children}
  </div>;
}
function Sidebar({ th, active }) {
  const items = ['Overview', 'Split view', 'Projects', 'History', 'Recipes'];
  const machines = [['RTX-PC', true], ['Mac Mini', true], ['MacBook', false]];
  return <div style={{ position: 'absolute', left: 0, top: 44, width: 240, height: 856, background: th.bg1, borderRight: `1px solid ${th.line}` }}>
    <div style={{ position: 'absolute', left: 22, top: 24, display: 'flex', alignItems: 'center', gap: 10, fontWeight: 800, fontSize: 18, letterSpacing: '-0.02em' }}><Mark size={20} ink={th.t1} dot={th.acc} />Sentinal</div>
    {items.map((n, i) => <div key={n} style={{ position: 'absolute', left: 12, top: 80 + i * 36, width: 216, height: 36, padding: '0 12px', display: 'flex', alignItems: 'center', fontSize: 14, fontWeight: n === active ? 700 : 500, color: n === active ? th.t1 : th.t2, background: n === active ? th.bg2 : 'transparent' }}>{n}</div>)}
    <div style={{ position: 'absolute', left: 24, top: 290, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3 }}>MACHINES</div>
    {machines.map(([n, on], i) => <div key={n} style={{ position: 'absolute', left: 24, right: 20, top: 316 + i * 34, height: 28, display: 'flex', alignItems: 'center', gap: 10, fontSize: 14, color: on ? th.t1 : th.t3 }}>
      <span style={{ width: 8, height: 8, background: on ? ST.RUNNING : 'transparent', boxShadow: on ? 'none' : `inset 0 0 0 1.5px ${th.t3}` }} />{n}<span style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: 11, color: th.t3 }}>{on ? 'online' : '14m ago'}</span></div>)}
  </div>;
}
function Cursor({ x, y, click = 0, opacity = 1 }) {
  return <div style={{ position: 'absolute', left: x, top: y, opacity, pointerEvents: 'none', zIndex: 50 }}>
    {click > 0 && click < 1 && <div style={{ position: 'absolute', left: -22, top: -22, width: 44, height: 44, borderRadius: 22, border: '3px solid #3D5AFE', transform: `scale(${0.4 + click * 1.2})`, opacity: 1 - click }} />}
    <svg width="30" height="30" viewBox="0 0 24 24" style={{ position: 'absolute', left: -3, top: -2, transform: `scale(${click > 0 && click < 0.4 ? 0.88 : 1})`, transformOrigin: '3px 2px' }}><path d="M3 2l16 10-7 1.4L8.6 21z" fill="#F2F2F0" stroke="#0A0A0A" strokeWidth="1.6" strokeLinejoin="round" /></svg>
  </div>;
}

// ——— Overview (window coords: main area left 240, top 44) ———
const RTX = [['RUNNING', 'Qwen Training', 'python train.py --config moe.yaml', 'epoch 68/100 · loss 0.37', '1h31m'], ['RUNNING', 'vLLM', 'vllm serve Qwen/Qwen3-8B --port 8000', 'server on :8000', ''], ['FAILED', 'pytest', 'pytest tests/', '3 failed · 182 passed', '42s'], ['IDLE', 'shell', '~/Projects/TIM', '', '']];
const MINI = [['STALLED', 'Compression', 'no output 14m', 'CPU 0 % · possibly stalled', '14m'], ['WAITING', 'npm install', 'Continue? [y/N]', 'waiting for input', '']];
function Row({ th, r, y, opacity }) {
  return <div style={{ position: 'absolute', left: 40, top: y, width: 1120, height: 56, display: 'flex', alignItems: 'center', gap: 16, borderBottom: `1px solid ${th.line}`, opacity }}>
    <div style={{ width: 130, flex: 'none' }}><Badge s={r[0]} /></div>
    <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}><span style={{ fontSize: 15, fontWeight: 700 }}>{r[1]}</span><span style={{ fontFamily: MONO, fontSize: 12, color: th.t3 }}>{r[2]}</span></div>
    <div style={{ width: 280, flex: 'none', fontSize: 14, color: th.t2 }}>{r[3]}</div>
    <div style={{ width: 70, flex: 'none', textAlign: 'right', fontFamily: MONO, fontSize: 13, color: th.t2 }}>{r[4]}</div>
  </div>;
}
function GroupHead({ th, y, name, spec, on, opacity }) {
  return <div style={{ position: 'absolute', left: 40, top: y, width: 1120, height: 40, display: 'flex', alignItems: 'center', gap: 12, borderBottom: `2px solid ${on ? th.t1 : th.line}`, opacity }}>
    <span style={{ fontSize: 20, fontWeight: 800, letterSpacing: '-0.02em', color: on ? th.t1 : th.t3 }}>{name}</span><span style={{ fontSize: 13, color: th.t3 }}>{spec}</span>
    <span style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: 12, color: on ? ST.RUNNING : th.t3 }}>{on ? 'ONLINE' : 'OFFLINE · last seen 14m ago'}</span>
  </div>;
}
function OverviewMain({ c }) {
  const { P, cR } = c, th = DARK;
  const lift = P(cR + 3.0, cR + 3.9, M.draw), off = lerp(-190, 0, lift);
  const ri = i => P(cR + 0.7 + i * 0.1, cR + 1.2 + i * 0.1);
  const cards = [[RTX[2], 330 + 2 * 56, 'RTX-PC · pytest tests/', '3 failed · 182 passed · 42s'], [MINI[1], 620 + 56, 'Mac Mini · npm install', 'Continue? [y/N]'], [MINI[0], 620, 'Mac Mini · Compression', 'no output 14m · CPU 0 %']];
  const cIn = P(cR + 3.5, cR + 4.0);
  return <div style={{ position: 'absolute', left: 240, top: 44, width: 1200, height: 856 }}>
    <div style={{ position: 'absolute', left: 40, top: 30, display: 'flex', alignItems: 'baseline', gap: 14 }}><span style={{ fontSize: 32, fontWeight: 800, letterSpacing: '-0.03em' }}>Overview</span><span style={{ fontSize: 14, color: th.t3 }}>3 machines · 6 sessions</span></div>
    <div style={{ position: 'absolute', left: 40, top: 100, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3, opacity: P(cR + 3.4, cR + 3.9) }}>NEEDS YOU · 3</div>
    <GroupHead th={th} y={290 + off} name="RTX-PC" spec="Linux · Ryzen 9 · 64 GB · RTX 4090 24 GB" on opacity={ri(0)} />
    {RTX.map((r, i) => <Row key={i} th={th} r={r} y={330 + i * 56 + off} opacity={ri(i + 1)} />)}
    <GroupHead th={th} y={580 + off} name="Mac Mini" spec="macOS · M2 Pro · 32 GB" on opacity={ri(5)} />
    {MINI.map((r, i) => <Row key={i} th={th} r={r} y={620 + i * 56 + off} opacity={ri(6 + i)} />)}
    <GroupHead th={th} y={760 + off} name="MacBook" spec="macOS · M3 · 16 GB" on={false} opacity={ri(8) * 0.7} />
    {cards.map(([r, ry, sub, fact], i) => {
      const x = lerp(40, 40 + i * 378, lift), y = lerp(ry - 190, 128, lift), w = lerp(1120, 362, lift), h = lerp(56, 124, lift);
      return <div key={i} style={{ position: 'absolute', left: x, top: y, width: w, height: h, background: DARK.bg1, boxShadow: `inset 0 3px 0 ${ST[r[0]]}`, opacity: P(cR + 2.95, cR + 3.2), padding: '18px 18px 0', display: 'flex', flexDirection: 'column', gap: 8, overflow: 'hidden' }}>
        <div style={{ opacity: cIn, display: 'flex', flexDirection: 'column', gap: 8 }}><Badge s={r[0]} /><span style={{ fontSize: 17, fontWeight: 700 }}>{sub}</span><span style={{ fontFamily: MONO, fontSize: 12, color: th.t2 }}>{fact}</span></div>
      </div>;
    })}
  </div>;
}

// ——— Session detail ———
const EVENTS = [['12:42', 'COMMAND_STARTED', 'python train.py --config moe.yaml'], ['12:44', 'PROGRESS', 'epoch 1/100'], ['13:41', 'CHECKPOINT_CREATED', 'ckpt/epoch_37.pt'], ['13:58', 'RESOURCE_HIGH', 'VRAM 96.5 %'], ['14:02', 'WARNING', 'VRAM usage high'], ['14:13', 'PROGRESS', 'epoch 68/100']];
const LOSS = [0.41, 0.405, 0.398, 0.4, 0.391, 0.386, 0.388, 0.381, 0.377, 0.379, 0.374, 0.37];
function Meter({ v, color }) { return <div style={{ height: 8, background: DARK.bg2 }}><div style={{ width: `${v * 100}%`, height: '100%', background: color }} /></div>; }
function DetailMain({ c }) {
  const { P, cG, T } = c, th = DARK;
  const pr = P(cG + 0.3, cG + 1.8, M.draw), epoch = Math.round(lerp(66, 68, P(cG + 0.3, cG + 1.8)));
  const spark = P(cG + 0.6, cG + 2.2, M.draw), mt = P(cG + 0.8, cG + 2.2, M.draw);
  const pts = LOSS.map((v, i) => `${(i / (LOSS.length - 1)) * 250},${((0.415 - v) / 0.05) * 44}`).join(' ');
  const stat = (i, label, big, foot, body) => <div key={i} style={{ position: 'absolute', left: 40 + i * 286, top: 168, width: 262, height: 160, borderTop: `2px solid ${th.t1}`, paddingTop: 12, display: 'flex', flexDirection: 'column', gap: 10, opacity: P(cG + 0.1 + i * 0.1, cG + 0.6 + i * 0.1) }}>
    <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3 }}>{label}</span>
    <span style={{ fontSize: 48, lineHeight: '48px', fontWeight: 800, letterSpacing: '-0.04em' }}>{big}</span>{body}
    <span style={{ fontFamily: MONO, fontSize: 12, color: th.t2 }}>{foot}</span></div>;
  const health = [['Progress', '68 %'], ['Activity', 'GOOD'], ['Resources', 'GOOD'], ['Stability', 'GOOD'], ['Errors', 'NONE'], ['Overall', 'HEALTHY']];
  const ev = P(cG + 6.8, cG + 7.3);
  return <div style={{ position: 'absolute', left: 240, top: 44, width: 1200, height: 856 }}>
    <div style={{ position: 'absolute', left: 40, top: 26, fontFamily: MONO, fontSize: 13, color: th.t3 }}>RTX-PC / TIM / Qwen Training</div>
    <div style={{ position: 'absolute', left: 40, top: 52, display: 'flex', alignItems: 'center', gap: 16 }}><span style={{ fontSize: 40, fontWeight: 800, letterSpacing: '-0.04em' }}>Qwen Training</span><Badge s="RUNNING" h={28} fs={12} /><span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, height: 28, padding: '0 10px', boxShadow: `inset 0 0 0 1.5px ${ST.RUNNING}`, color: ST.RUNNING, fontSize: 12, fontWeight: 700, letterSpacing: '0.04em' }}><Glyph s="HEALTHY" size={13} color={ST.RUNNING} />HEALTHY</span></div>
    <div style={{ position: 'absolute', left: 40, top: 112, fontFamily: MONO, fontSize: 14, color: th.t2 }}>python train.py --config moe.yaml · running 1h31m</div>
    <div style={{ position: 'absolute', right: 40, top: 56, height: 44, padding: '0 20px', display: 'flex', alignItems: 'center', background: th.acc, color: '#fff', fontSize: 15, fontWeight: 700 }}>Open terminal</div>
    {stat(0, 'PROGRESS', `${Math.round(pr * 68)} %`, `epoch ${epoch}/100`, <Meter v={pr * 0.68} color={th.acc} />)}
    {stat(1, 'LOSS', '0.37', 'from 0.41 · improving', <svg width="250" height="46" style={{ display: 'block', overflow: 'visible' }}><polyline points={pts} fill="none" stroke={th.accFg} strokeWidth="2.5" pathLength="1" style={{ strokeDasharray: 1, strokeDashoffset: 1 - spark }} /></svg>)}
    {stat(2, 'GPU', `${Math.round(mt * 96)} %`, 'RTX 4090', <Meter v={mt * 0.96} color={ST.RUNNING} />)}
    {stat(3, 'VRAM', '21.4 GB', 'of 24 GB · 89 %', <Meter v={mt * 0.89} color={ST.STALLED} />)}
    <div style={{ position: 'absolute', left: 40, top: 380, width: 560, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3, borderBottom: `2px solid ${th.t1}`, paddingBottom: 8, opacity: P(cG + 3.2, cG + 3.6) }}>EVENTS</div>
    {EVENTS.map((e, i) => { const warn = e[1] === 'WARNING' || e[1] === 'RESOURCE_HIGH'; return <div key={i} style={{ position: 'absolute', left: 40, top: 412 + i * 48, width: 560, height: 48, display: 'flex', alignItems: 'center', gap: 16, borderBottom: `1px solid ${th.line}`, fontFamily: MONO, fontSize: 13, opacity: P(cG + 3.4 + i * 0.25, cG + 3.8 + i * 0.25) }}>
      <span style={{ color: th.t3, width: 44 }}>{e[0]}</span><span style={{ width: 170, fontSize: 11, fontWeight: 700, color: warn ? ST.WAITING : th.t2 }}>{e[1]}</span><span style={{ color: th.t1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', flex: 1 }}>{e[2]}</span></div>; })}
    <div style={{ position: 'absolute', left: 640, top: 380, width: 520, background: th.bg1, padding: 24, display: 'flex', flexDirection: 'column', gap: 0, opacity: P(cG + 5.0, cG + 5.5) }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingBottom: 14 }}><span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3 }}>HEALTH · DETERMINISTIC</span><Badge s="HEALTHY" /></div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', columnGap: 24 }}>
        {health.map(([k, v], i) => <div key={k} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', height: 48, borderTop: `1px solid ${th.line}`, opacity: P(cG + 5.4 + i * 0.18, cG + 5.8 + i * 0.18) }}><span style={{ fontSize: 15, color: th.t2 }}>{k}</span><span style={{ fontFamily: MONO, fontSize: 15, fontWeight: 700, color: v === 'HEALTHY' || v === 'GOOD' ? ST.RUNNING : th.t1 }}>{v}</span></div>)}
      </div>
      <div style={{ marginTop: 16, padding: '14px 16px', background: `rgba(61,90,254,${0.08 + ev * 0.18})`, boxShadow: `inset 0 0 0 1px rgba(140,158,255,${ev * 0.7})`, display: 'flex', flexDirection: 'column', gap: 6, opacity: P(cG + 6.4, cG + 6.9) }}>
        <span style={{ fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.accFg }}>EVIDENCE</span>
        <span style={{ fontFamily: MONO, fontSize: 14, lineHeight: '21px', color: th.t1 }}>loss improving for 5 checks · GPU 96 % · last output 8s ago</span>
      </div>
    </div>
  </div>;
}

// ——— Phone ———
function Phone({ c, rect }) {
  const { P, cA, T } = c;
  const s = Math.min(rect.h / 844, rect.w / 390) * 0.96;
  const inn = P(cA + 0.2, cA + 0.9), out = P(cA + 6.0, cA + 6.6, M.draw);
  const op = inn * (1 - out), dy = (1 - inn) * 80 - out * 60;
  const drift = 1 + P(cA, cA + 6.6, Easing.linear) * 0.035;
  const note = P(cA + 1.2, cA + 1.8, M.pop), tap = cl((T - (cA + 2.6)) / 0.5);
  const B = P(cA + 2.9, cA + 3.3), tapY = cl((T - (cA + 4.3)) / 0.5), answered = T >= cA + 4.6;
  const toast = P(cA + 4.7, cA + 5.1) * (1 - P(cA + 5.9, cA + 6.2));
  return <div style={{ position: 'absolute', left: rect.x + rect.w / 2 - 195 * s, top: rect.y + rect.h / 2 - 422 * s + dy, width: 390, height: 844, transformOrigin: '0 0', transform: `scale(${s * drift})`, opacity: op, visibility: op > 0.001 ? 'visible' : 'hidden', fontFamily: SANS, color: '#F2F2F0' }}>
    <div style={{ position: 'absolute', inset: 0, background: '#1C1C1C', borderRadius: 58, boxShadow: '0 0 0 1px #3A3A3A, 0 60px 120px -30px rgba(0,0,0,.8)' }} />
    <div style={{ position: 'absolute', left: 12, top: 12, width: 366, height: 820, borderRadius: 46, background: '#0A0A0A', overflow: 'hidden' }}>
      <div style={{ position: 'absolute', left: 30, right: 30, top: 16, display: 'flex', justifyContent: 'space-between', fontSize: 15, fontWeight: 700 }}><span>23:41</span><span style={{ width: 26, height: 12, boxShadow: 'inset 0 0 0 1.5px #F2F2F0' }} /></div>
      <div style={{ position: 'absolute', left: 0, right: 0, top: 110, textAlign: 'center', fontSize: 92, fontWeight: 800, letterSpacing: '-0.05em', opacity: 1 - B }}>23:41</div>
      <div style={{ position: 'absolute', left: 12, right: 12, top: lerp(-140, 250, note), padding: 14, background: '#1C1C1C', borderRadius: 20, display: 'flex', gap: 12, opacity: 1 - B, boxShadow: '0 0 0 1px #2A2A2A' }}>
        <div style={{ width: 40, height: 40, flex: 'none', borderRadius: 10, background: '#000', display: 'flex', alignItems: 'center', justifyContent: 'center', boxShadow: '0 0 0 1px #333' }}><Mark size={24} /></div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}><div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, fontWeight: 700, letterSpacing: '0.06em', color: '#A6A6A2' }}><span>SENTINAL</span><span style={{ fontWeight: 500, letterSpacing: 0 }}>now</span></div>
          <span style={{ fontSize: 15, lineHeight: '20px', fontWeight: 600 }}>Mac Mini · npm install: waiting for input — <span style={{ fontFamily: MONO, fontSize: 14 }}>Continue? [y/N]</span></span></div>
        {tap > 0 && tap < 1 && <div style={{ position: 'absolute', left: '50%', top: '50%', width: 60, height: 60, margin: -30, borderRadius: 30, background: 'rgba(242,242,240,.35)', transform: `scale(${0.3 + tap * 1.5})`, opacity: 1 - tap }} />}
      </div>
      <div style={{ position: 'absolute', inset: 0, opacity: B, transform: `translateY(${(1 - B) * 30}px)` }}>
        <div style={{ position: 'absolute', left: 22, top: 60, fontSize: 16, fontWeight: 600, color: '#8C9EFF' }}>← Overview</div>
        <div style={{ position: 'absolute', left: 22, top: 108, fontFamily: MONO, fontSize: 13, color: '#8A8A86' }}>MAC MINI</div>
        <div style={{ position: 'absolute', left: 22, top: 130, fontSize: 32, fontWeight: 800, letterSpacing: '-0.03em' }}>npm install</div>
        <div style={{ position: 'absolute', left: 22, top: 180 }}><Badge s={answered ? 'RUNNING' : 'WAITING'} h={28} fs={12} /></div>
        <div style={{ position: 'absolute', left: 22, top: 244, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: '#8A8A86' }}>PROMPT</div>
        <div style={{ position: 'absolute', left: 22, right: 22, top: 268, height: 110, background: '#0C0C0C', boxShadow: '0 0 0 1px #2A2A2A', display: 'flex', alignItems: 'center', padding: '0 20px', fontFamily: MONO, fontSize: 28, color: '#F2F2F0' }}>Continue? [y/N]{answered && <span style={{ color: '#29D35F' }}>&nbsp;y</span>}</div>
        <div style={{ position: 'absolute', left: 22, right: 22, top: 400, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
          <div style={{ height: 76, background: '#F2F2F0', color: '#0A0A0A', display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: MONO, fontSize: 34, fontWeight: 700, transform: `scale(${tapY > 0 && tapY < 0.5 ? 0.95 : 1})`, position: 'relative' }}>y
            {tapY > 0 && tapY < 1 && <div style={{ position: 'absolute', left: '50%', top: '50%', width: 60, height: 60, margin: -30, borderRadius: 30, background: 'rgba(61,90,254,.45)', transform: `scale(${0.3 + tapY * 1.6})`, opacity: 1 - tapY }} />}</div>
          <div style={{ height: 76, boxShadow: 'inset 0 0 0 1.5px #6B6B67', display: 'flex', alignItems: 'center', justifyContent: 'center', fontFamily: MONO, fontSize: 34, fontWeight: 700 }}>n</div>
        </div>
        <div style={{ position: 'absolute', left: 22, right: 22, top: 490, height: 56, background: '#3D5AFE', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 16, fontWeight: 700 }}>Open terminal</div>
        <div style={{ position: 'absolute', left: 22, right: 22, bottom: 40, height: 52, background: '#1C1C1C', display: 'flex', alignItems: 'center', gap: 10, padding: '0 16px', fontSize: 14, opacity: toast, transform: `translateY(${(1 - toast) * 20}px)` }}><Glyph s="COMPLETED" size={14} color="#29D35F" />Sent <span style={{ fontFamily: MONO }}>y</span> to npm install</div>
      </div>
    </div>
  </div>;
}

// ——— Split view ———
function SplitBody({ c }) {
  const { T, cA } = c;
  const pw = 698, ph = 406;
  const panes = [
    ['RUNNING', 'Qwen Training', 'RTX-PC', 'epoch 68/100', ['epoch 65/100  loss 0.39', 'epoch 66/100  loss 0.39', 'epoch 67/100  loss 0.38', ['epoch 68/100  loss 0.37', cA + 8.6], ['checkpoint ok · GPU 96 %', cA + 9.8]]],
    ['RUNNING', 'vLLM', 'RTX-PC', ':8000', ['$ vllm serve Qwen/Qwen3-8B --port 8000', 'INFO  Started server process', 'INFO  Uvicorn running on http://0.0.0.0:8000']],
    ['FAILED', 'pytest', 'RTX-PC', '42s', ['$ pytest tests/', '', ['3 failed, 182 passed in 42s', 0, '#FF3B2F'], 'failures: molecule correspondence tests']],
    ['RUNNING', 'npm install', 'Mac Mini', 'answered y', ['$ npm install', ['Continue? [y/N] y', 0, '#29D35F']]],
  ];
  const blink = Math.floor(T * 2.2) % 2 === 0;
  return <div style={{ position: 'absolute', left: 16, top: 60, right: 16, bottom: 16, display: 'grid', gridTemplateColumns: `${pw}px ${pw}px`, gridTemplateRows: `${ph}px ${ph}px`, gap: 12 }}>
    {panes.map((p, i) => <div key={i} style={{ background: '#0C0C0C', boxShadow: `0 0 0 1px ${i === 0 ? '#3D5AFE' : '#262626'}`, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{ height: 38, flex: 'none', display: 'flex', alignItems: 'center', gap: 10, padding: '0 12px', background: '#111111', borderBottom: '1px solid #262626' }}><Badge s={p[0]} h={22} fs={10} /><span style={{ fontSize: 14, fontWeight: 700, color: '#F2F2F0' }}>{p[1]}</span><span style={{ fontFamily: MONO, fontSize: 12, color: '#8A8A86' }}>{p[2]}</span><span style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: 12, color: '#A6A6A2' }}>{p[3]}</span></div>
      <div style={{ padding: '14px 16px', fontFamily: MONO, fontSize: 15, lineHeight: '24px', color: '#D4D4D0', whiteSpace: 'pre' }}>
        {p[4].map((l, j) => { const L = typeof l === 'string' ? [l, 0] : l; return T >= L[1] ? <div key={j} style={{ color: L[2] || '#D4D4D0' }}>{L[0] || ' '}</div> : null; })}
        {(i === 0 || i === 3) && <div>{blink ? <Caret /> : ' '}</div>}
      </div>
    </div>)}
  </div>;
}

// ——— History + Recipe (light) ———
const HIST = [['COMPLETED', 'cargo build', '31s'], ['FAILED', 'pytest tests/', '42s'], ['COMPLETED', 'python benchmark.py', '18m'], ['RUNNING', 'python train.py', 'running']];
function HistoryMain({ c }) {
  const { P, T, cH } = c, th = LIGHT;
  const q = typed(T, 'python', cH + 1.2, 8), col = P(cH + 2.3, cH + 2.9, M.draw);
  const matchQ = s => s.indexOf('python') >= 0;
  let y = 226;
  return <div style={{ position: 'absolute', left: 240, top: 44, width: 1200, height: 856 }}>
    <div style={{ position: 'absolute', left: 40, top: 30, fontSize: 32, fontWeight: 800, letterSpacing: '-0.03em' }}>History</div>
    <div style={{ position: 'absolute', left: 40, top: 92, width: 720, height: 54, boxShadow: `inset 0 0 0 ${q ? 2 : 1}px ${q ? th.acc : th.lineS}`, background: '#fff', display: 'flex', alignItems: 'center', gap: 12, padding: '0 18px', fontFamily: MONO, fontSize: 18 }}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none"><circle cx="10.5" cy="10.5" r="6.5" stroke={th.t3} strokeWidth="2.2" /><path d="M15.5 15.5L21 21" stroke={th.t3} strokeWidth="2.2" /></svg>
      {q ? <span>{q}</span> : <span style={{ color: th.t3 }}>Search commands</span>}{T > cH + 1 && T < cH + 3.2 && Math.floor(T * 2.2) % 2 === 0 && <Caret c={th.t1} />}
    </div>
    <div style={{ position: 'absolute', left: 40, top: 160, display: 'flex', gap: 8 }}>{['All machines', 'All projects', 'Success & failed', 'Any date'].map(f => <span key={f} style={{ height: 32, padding: '0 12px', display: 'flex', alignItems: 'center', fontSize: 13, fontWeight: 600, boxShadow: `inset 0 0 0 1px ${th.lineS}`, color: th.t2 }}>{f} ▾</span>)}</div>
    {HIST.map((h, i) => { const hide = matchQ(h[1]) ? 0 : col, ht = 60 * (1 - hide), top = y; y += ht; const idx = h[1].indexOf('python');
      return <div key={i} style={{ position: 'absolute', left: 40, top, width: 1120, height: ht, overflow: 'hidden', opacity: (1 - hide) * P(cH + 0.6 + i * 0.1, cH + 1.0 + i * 0.1), display: 'flex', alignItems: 'center', gap: 18, borderBottom: `1px solid ${th.line}` }}>
        <span style={{ width: 28, height: 28, flex: 'none', background: ST[h[0]], display: 'flex', alignItems: 'center', justifyContent: 'center' }}><Glyph s={h[0]} size={14} /></span>
        <span style={{ flex: 1, fontFamily: MONO, fontSize: 17 }}>{q === 'python' && idx >= 0 ? <><span style={{ background: 'rgba(42,68,224,.14)', color: th.acc, fontWeight: 700 }}>python</span>{h[1].slice(6)}</> : h[1]}</span>
        <span style={{ fontFamily: MONO, fontSize: 14, color: th.t2 }}>RTX-PC</span>
        <span style={{ width: 90, textAlign: 'right', fontFamily: MONO, fontSize: 15, color: th.t2 }}>{h[2]}</span></div>; })}
  </div>;
}
function Tok({ T, at, name, val, th }) {
  const on = T >= at, flash = cl(1 - (T - at) / 0.8);
  return <span style={{ color: on ? '#F2F2F0' : '#8C9EFF', background: on ? `rgba(61,90,254,${0.15 + flash * 0.5})` : 'transparent' }}>{on ? val : `{${name}}`}</span>;
}
function RecipeMain({ c }) {
  const { P, T, cH } = c, th = LIGHT;
  const params = [['model', 'Qwen/Qwen3-8B'], ['port', '8000'], ['context', '32768']];
  const press = cl((T - (cH + 7.5)) / 0.4), started = T >= cH + 7.9;
  return <div style={{ position: 'absolute', left: 240, top: 44, width: 1200, height: 856 }}>
    <div style={{ position: 'absolute', left: 40, top: 30, fontSize: 32, fontWeight: 800, letterSpacing: '-0.03em' }}>Qwen vLLM Server</div>
    <div style={{ position: 'absolute', left: 40, top: 80, fontFamily: MONO, fontSize: 13, color: th.t3 }}>recipe</div>
    {params.map(([k, v], i) => <div key={k} style={{ position: 'absolute', left: 40, top: 124 + i * 62, width: 720, height: 62, display: 'flex', alignItems: 'center', gap: 20, borderBottom: `1px solid ${th.line}` }}>
      <span style={{ width: 110, fontFamily: MONO, fontSize: 15, color: th.t2 }}>{k}</span>
      <span style={{ flex: 1, height: 42, background: '#fff', boxShadow: `inset 0 0 0 1px ${th.lineS}`, display: 'flex', alignItems: 'center', padding: '0 14px', fontFamily: MONO, fontSize: 16 }}>{v}</span>
      <span style={{ width: 60, fontSize: 12, color: th.t3 }}>default</span></div>)}
    <div style={{ position: 'absolute', left: 40, top: 334, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3 }}>TARGET MACHINE</div>
    <div style={{ position: 'absolute', left: 40, top: 358, display: 'flex', gap: 2 }}>{[['RTX-PC', 1], ['Mac Mini', 0], ['MacBook · offline', -1]].map(([n, s]) => <span key={n} style={{ height: 44, padding: '0 18px', display: 'flex', alignItems: 'center', fontSize: 15, fontWeight: 700, background: s === 1 ? th.t1 : th.bg1, color: s === 1 ? th.bg0 : s === 0 ? th.t1 : th.t3 }}>{n}</span>)}</div>
    <div style={{ position: 'absolute', left: 40, top: 430, fontSize: 11, fontWeight: 700, letterSpacing: '0.08em', color: th.t3 }}>COMMAND</div>
    <div style={{ position: 'absolute', left: 40, top: 454, width: 1000, height: 72, background: '#0C0C0C', display: 'flex', alignItems: 'center', padding: '0 22px', fontFamily: MONO, fontSize: 21, color: '#D4D4D0', whiteSpace: 'pre' }}>
      <span style={{ color: '#8A8A86' }}>$ </span>vllm serve <Tok T={T} at={cH + 5.6} name="model" val="Qwen/Qwen3-8B" /> --port <Tok T={T} at={cH + 5.9} name="port" val="8000" /> --max-model-len <Tok T={T} at={cH + 6.2} name="context" val="32768" />
    </div>
    <div style={{ position: 'absolute', left: 40, top: 556, width: 240, height: 54, background: started ? th.t1 : th.acc, color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 10, fontSize: 16, fontWeight: 700, transform: `scale(${press > 0 && press < 0.5 ? 0.96 : 1})` }}>{started ? <><Glyph s="RUNNING" size={12} color={ST.RUNNING} />Running on RTX-PC</> : 'Run on RTX-PC'}</div>
  </div>;
}

// ——— Piece ———
function Headline({ F, text, n, a, b, P, color, acc }) {
  const i = P(a, a + 0.7), o = P(b - 0.4, b, M.draw), op = i * (1 - o);
  return <div style={{ position: 'absolute', left: F.hl.x, top: F.hl.y, width: F.hl.mw, opacity: op, visibility: op > 0.001 ? 'visible' : 'hidden', display: 'flex', flexDirection: 'column', gap: F.hl.fs * 0.2 }}>
    <span style={{ fontFamily: MONO, fontSize: Math.max(16, F.hl.fs * 0.28), color: acc, transform: `translateY(${(1 - i) * 16}px)` }}>{n} / 04</span>
    <span style={{ fontFamily: SANS, fontSize: F.hl.fs, lineHeight: 1.0, fontWeight: 800, letterSpacing: '-0.045em', color, transform: `translateY(${(1 - i) * 36}px)`, textWrap: 'balance' }}>{text}</span>
  </div>;
}
function cam(T, keys) {
  if (T <= keys[0][0]) return keys[0].slice(1);
  for (let i = 0; i < keys.length - 1; i++) if (T < keys[i + 1][0]) { const k = M.draw(cl((T - keys[i][0]) / (keys[i + 1][0] - keys[i][0]))); return [1, 2, 3].map(j => lerp(keys[i][j], keys[i + 1][j], k)); }
  return keys[keys.length - 1].slice(1);
}
function path(T, keys) { return cam(T, keys); }

function Piece({ F }) {
  const { T, CUES } = useComposition();
  const P = (s, e, ease = M.enter) => ease(cl((T - s) / (e - s)));
  const cT = CUES.Terminal, cL = CUES.Login, cR = CUES.Running, cG = CUES.Going, cA = CUES.Reach, cH = CUES.History, cE = CUES.End;
  const c = { T, P, cR, cG, cA, cH };
  const lf = P(cH - 0.5, cH + 0.3, M.draw) * (1 - P(cE - 0.6, cE + 0.1, M.draw));
  const bg = mix(DARK.bg0, LIGHT.bg0, lf), ink = mix(DARK.t1, LIGHT.t1, lf), acc = lf > 0.5 ? LIGHT.acc : DARK.accFg;
  const dotC = lf > 0.5 ? 'rgba(10,10,10,.08)' : 'rgba(242,242,240,.07)';

  // Terminal scene
  const R = F.term, cols = F.cols, rows = 4 / cols, g = 16;
  const qw = (R.w - g * (cols - 1)) / cols, qh = (R.h - g * (rows - 1)) / rows;
  const slot = i => ({ x: R.x + (i % cols) * (qw + g), y: R.y + Math.floor(i / cols) * (qh + g), w: qw, h: qh });
  const split = P(cT + 3.1, cT + 3.9, M.draw);
  const s0 = slot(0), r0 = { x: lerp(R.x, s0.x, split), y: lerp(R.y, s0.y, split), w: lerp(R.w, s0.w, split), h: lerp(R.h, s0.h, split) };
  const dim = 1 - 0.8 * P(cT + 4.4, cT + 5.0) ;
  const termOut = P(cT + 6.7, cT + 7.1, M.draw), termIn = P(cT, cT + 0.4);
  const tOp = termIn * dim * (1 - termOut);
  const ep = [2.4, 2.9, 3.4, 4.0, 4.6, 5.2, 5.8].map((t, i) => ({ t: cT + t, text: `epoch ${i + 1}/100` }));
  const paneDefs = [
    { machine: 'RTX-PC', title: 'Qwen Training', cmd: 'python train.py --config moe.yaml', at: cT + 0.5, out: [{ t: cT + 2.0, text: 'loading config moe.yaml · project TIM' }, ...ep] },
    { machine: 'RTX-PC', title: 'vLLM', cmd: 'vllm serve Qwen/Qwen3-8B --port 8000', at: cT + 3.7, out: [{ t: cT + 5.3, text: 'INFO  Uvicorn running on http://0.0.0.0:8000' }] },
    { machine: 'RTX-PC', title: 'pytest', cmd: 'pytest tests/', at: cT + 3.9, out: [{ t: cT + 5.0, text: '3 failed, 182 passed in 42s', color: '#FF3B2F' }] },
    { machine: 'Mac Mini', title: 'npm install', cmd: 'npm install', at: cT + 4.1, out: [{ t: cT + 5.1, text: 'Continue? [y/N]', color: '#FFD000', caret: true }] },
  ];
  const blink = Math.floor(T * 2.2) % 2 === 0;
  const pane = (d, i, rect, op) => {
    const cmd = typed(T, d.cmd, d.at, 22), done = T >= d.at + d.cmd.length / 22, fs = F.tfs;
    return <div key={i} style={{ position: 'absolute', left: rect.x, top: rect.y, width: rect.w, height: rect.h, background: '#0C0C0C', boxShadow: '0 0 0 1px #262626', opacity: op, overflow: 'hidden' }}>
      <div style={{ height: fs * 2, display: 'flex', alignItems: 'center', gap: 10, padding: `0 ${fs}px`, borderBottom: '1px solid #262626', background: '#111111', fontFamily: MONO, fontSize: fs * 0.72, color: '#8A8A86' }}><span style={{ color: '#F2F2F0', fontWeight: 700 }}>{d.machine}</span><span>{d.title}</span></div>
      <div style={{ padding: fs, fontFamily: MONO, fontSize: fs, lineHeight: 1.55, color: '#D4D4D0', whiteSpace: 'pre' }}>
        <div><span style={{ color: '#8C9EFF' }}>$ </span>{cmd}{!done && T >= d.at - 0.4 && (blink || T >= d.at) && <Caret />}</div>
        {d.out.filter(o => T >= o.t).map((o, j) => <div key={j} style={{ color: o.color || '#A6A6A2' }}>{o.text}{o.caret && blink && <Caret c="#FFD000" />}</div>)}
      </div>
    </div>;
  };
  const say1 = P(cT + 4.6, cT + 5.2), say2 = P(cT + 5.4, cT + 6.0), sayOut = P(cT + 6.6, cT + 7.0, M.draw);
  const prompt = typed(T, 'sentinal', cT + 7.0, 16), pOp = P(cT + 6.9, cT + 7.1) * (1 - P(cL - 0.1, cL + 0.2));

  // Login
  const lgDraw = P(cL + 0.0, cL + 0.7, M.draw), lgPop = P(cL + 0.6, cL + 1.0, M.pop);
  const wIn = P(cL + 0.9, cL + 1.4), fIn = P(cL + 1.0, cL + 1.5);
  const dots = cl(Math.floor((T - (cL + 1.3)) / 0.1), 0, 8), ok = T >= cL + 2.25;
  const lgOut = P(cL + 2.7, cL + 3.4, M.draw), lgOp = (1 - lgOut) * (T >= cL - 0.2 ? 1 : 0);
  const cx = F.w / 2, cy = F.h / 2;

  // Windows
  const w1in = P(cL + 3.0, cL + 3.9), w1out = P(cA - 0.2, cA + 0.4, M.draw);
  const c1 = cam(T, [[cR - 0.8, 1, 720, 450], [cR + 2.8, 1, 720, 450], [cR + 4.2, 1.35, 720, 250], [cR + 6.6, 1.35, 720, 250], [cR + 8.2, 1.5, 560, 400], [cR + 9.6, 1.5, 560, 400], [cG + 0.4, 1.15, 760, 290], [cG + 2.8, 1.15, 760, 290], [cG + 3.8, 1.05, 620, 560], [cG + 5.2, 1.05, 620, 560], [cG + 6.2, 1.6, 1150, 620], [cG + 8.6, 1.6, 1150, 620], [cG + 9.8, 1.1, 720, 450]]);
  const det = P(cR + 9.4, cR + 9.9, M.draw);
  const cur1 = path(T, [[cR + 7.4, 1320, 880], [cR + 8.8, 470, 398]]), click1 = cl((T - (cR + 9.0)) / 0.5), cur1Op = P(cR + 7.4, cR + 7.7) * (1 - P(cR + 9.5, cR + 9.8));
  const w2in = P(cA + 6.3, cA + 7.1), w2out = P(cH - 0.6, cH, M.draw);
  const c2 = cam(T, [[cA + 6.3, 1, 720, 450], [cA + 8.3, 1, 720, 450], [cA + 9.8, 1.55, 370, 270], [cA + 12, 1.55, 370, 270]]);
  const w3in = P(cH + 0.1, cH + 0.9), w3out = P(cE - 0.5, cE + 0.1, M.draw);
  const c3 = cam(T, [[cH, 1, 720, 450], [cH + 1.0, 1.25, 640, 330], [cH + 3.0, 1.25, 640, 330], [cH + 4.2, 1, 720, 450], [cH + 5.2, 1.3, 740, 500], [cH + 6.6, 1.3, 740, 500], [cH + 7.4, 1.35, 620, 560], [cH + 9, 1.35, 620, 560]]);
  const rec = P(cH + 4.5, cH + 4.9, M.draw);
  const cur3 = path(T, [[cH + 3.2, 1000, 700], [cH + 4.2, 100, 286], [cH + 6.4, 100, 286], [cH + 7.3, 400, 628]]), click3a = cl((T - (cH + 4.3)) / 0.5), click3b = cl((T - (cH + 7.5)) / 0.5);
  const cur3Op = P(cH + 3.2, cH + 3.5) * (1 - P(cH + 8.2, cH + 8.5));
  const zb = F.zb;

  // End
  const eDraw = P(cE + 0.3, cE + 1.1, M.draw), ePop = P(cE + 1.0, cE + 1.4, M.pop);
  const eW = P(cE + 1.2, cE + 1.8), eT = P(cE + 1.8, cE + 2.4), eS = P(cE + 2.4, cE + 2.9), eG = P(cE + 2.8, cE + 3.3);
  const fade = P(cE + 6.3, cE + 7.0, M.draw);
  const eScale = 1 + P(cE, cE + 7, Easing.linear) * 0.04;

  return <div data-screen-label={`t=${Math.floor(T)}s`} style={{ position: 'absolute', inset: 0, background: bg, overflow: 'hidden', fontFamily: SANS, color: ink }}>
    <div style={{ position: 'absolute', inset: -40, backgroundImage: `radial-gradient(circle, ${dotC} 1.3px, transparent 1.8px)`, backgroundSize: '32px 32px', transform: `translateY(${-(T * 6) % 32}px)` }} />

    {/* 1 · Terminal */}
    {paneDefs.map((d, i) => i === 0 ? pane(d, i, r0, tOp) : pane(d, i, slot(i), tOp * P(cT + 3.4 + i * 0.15, cT + 3.9 + i * 0.15)))}
    <div style={{ position: 'absolute', left: 0, right: 0, top: cy - F.big * 1.05, textAlign: 'center', fontSize: F.big, lineHeight: 1.0, fontWeight: 800, letterSpacing: '-0.05em', color: '#F2F2F0', opacity: 1 - sayOut }}>
      <div style={{ opacity: say1, transform: `translateY(${(1 - say1) * 40}px)` }}>You start it.</div>
      <div style={{ opacity: say2, transform: `translateY(${(1 - say2) * 40}px)`, color: '#8C9EFF' }}>You walk away.</div>
    </div>
    <div style={{ position: 'absolute', left: 0, right: 0, top: cy - F.big * 0.3, textAlign: 'center', fontFamily: MONO, fontSize: F.big * 0.5, color: '#F2F2F0', opacity: pOp, whiteSpace: 'pre' }}><span style={{ color: '#8C9EFF' }}>$ </span>{prompt}{blink ? <Caret /> : <span style={{ display: 'inline-block', width: '0.55em' }} />}</div>

    {/* 2 · Login */}
    <div style={{ position: 'absolute', left: 0, top: 0, width: F.w, height: F.h, opacity: lgOp, visibility: lgOp > 0.001 ? 'visible' : 'hidden', transform: `scale(${1 - lgOut * 0.08})`, transformOrigin: `${cx}px ${cy}px` }}>
      <div style={{ position: 'absolute', left: cx - F.logo / 2, top: cy - F.logo - (F.stack ? 160 : 60) }}><Mark size={F.logo} draw={lgDraw} pop={lgPop} /></div>
      <div style={{ position: 'absolute', left: 0, right: 0, top: cy + (F.stack ? -70 : 60), display: 'flex', flexDirection: F.stack ? 'column' : 'row', alignItems: 'center', justifyContent: 'center', gap: F.stack ? 36 : 40 }}>
        <span style={{ fontSize: F.stack ? 72 : 76, fontWeight: 800, letterSpacing: '-0.045em', color: '#F2F2F0', opacity: wIn, transform: `translateY(${(1 - wIn) * 24}px)` }}>Welcome back.</span>
        <div style={{ width: F.stack ? 620 : 480, height: 84, boxShadow: `inset 0 0 0 2px ${ok ? '#3D5AFE' : '#6B6B67'}`, background: '#111111', display: 'flex', alignItems: 'center', gap: 14, padding: '0 16px 0 26px', opacity: fIn, transform: `translateY(${(1 - fIn) * 24}px)` }}>
          <div style={{ flex: 1, display: 'flex', gap: 14 }}>{Array.from({ length: dots }).map((_, i) => <span key={i} style={{ width: 16, height: 16, borderRadius: 8, background: '#F2F2F0' }} />)}{dots < 8 && blink && <span style={{ width: 3, height: 32, background: '#F2F2F0', marginTop: -8 }} />}</div>
          <span style={{ width: 54, height: 54, flex: 'none', background: ok ? '#3D5AFE' : '#1C1C1C', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>{ok ? <Glyph s="COMPLETED" size={24} color="#fff" /> : <span style={{ color: '#F2F2F0', fontSize: 24, fontFamily: MONO }}>→</span>}</span>
        </div>
      </div>
    </div>

    {/* Headlines */}
    <Headline F={F} P={P} n="01" text="What am I running?" a={cR + 0.2} b={cR + 9.7} color={ink} acc={acc} />
    <Headline F={F} P={P} n="02" text="How is it going?" a={cG + 0.2} b={cG + 9.8} color={ink} acc={acc} />
    <Headline F={F} P={P} n="03" text="Can I reach it?" a={cA + 0.3} b={cA + 11.6} color={ink} acc={acc} />
    <Headline F={F} P={P} n="04" text="How did I run this last time?" a={cH + 0.3} b={cE - 0.4} color={ink} acc={acc} />

    {/* 3-4 · Overview → detail */}
    <Viewport rect={F.mock} zoom={c1[0] * zb} fx={c1[1]} fy={c1[2]} opacity={w1in * (1 - w1out)} dy={(1 - w1in) * 90 - w1out * 70}>
      <WinFrame th={DARK} url={det > 0.5 ? 'sentinal.local/sessions/qwen-training' : 'sentinal.local/overview'}>
        <Sidebar th={DARK} active={det > 0.5 ? '' : 'Overview'} />
        <div style={{ opacity: 1 - det }}><OverviewMain c={c} /></div>
        <div style={{ opacity: det, transform: `translateY(${(1 - det) * 20}px)` }}><DetailMain c={c} /></div>
        <Cursor x={cur1[0]} y={cur1[1]} click={click1} opacity={cur1Op} />
      </WinFrame>
    </Viewport>

    {/* 5 · Reach */}
    <Phone c={c} rect={F.mock} />
    <Viewport rect={F.mock} zoom={c2[0] * zb} fx={c2[1]} fy={c2[2]} opacity={w2in * (1 - w2out)} dy={(1 - w2in) * 90 - w2out * 70}>
      <WinFrame th={DARK} url="sentinal.local/split">
        <div style={{ position: 'absolute', left: 16, top: 44, height: 16 }} />
        <SplitBody c={c} />
      </WinFrame>
    </Viewport>

    {/* 6 · History → Recipe (light) */}
    <Viewport rect={F.mock} zoom={c3[0] * zb} fx={c3[1]} fy={c3[2]} opacity={w3in * (1 - w3out)} dy={(1 - w3in) * 90 - w3out * 70}>
      <WinFrame th={LIGHT} url={rec > 0.5 ? 'sentinal.local/recipes/qwen-vllm-server' : 'sentinal.local/history'}>
        <Sidebar th={LIGHT} active={rec > 0.5 ? 'Recipes' : 'History'} />
        <div style={{ opacity: 1 - rec }}><HistoryMain c={c} /></div>
        <div style={{ opacity: rec, transform: `translateY(${(1 - rec) * 20}px)` }}><RecipeMain c={c} /></div>
        <Cursor x={cur3[0]} y={cur3[1]} click={Math.max(click3a > 0 && click3a < 1 ? click3a : 0, click3b > 0 && click3b < 1 ? click3b : 0)} opacity={cur3Op} />
      </WinFrame>
    </Viewport>

    {/* 7 · End */}
    <div style={{ position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: F.logo * 0.22, opacity: T >= cE ? 1 : 0, transform: `scale(${eScale})`, textAlign: 'center', padding: '0 72px' }}>
      <Mark size={F.logo * 1.1} draw={eDraw} pop={ePop} />
      <span style={{ fontSize: F.big * 1.05, lineHeight: 0.9, fontWeight: 800, letterSpacing: '-0.06em', color: '#F2F2F0', opacity: eW, transform: `translateY(${(1 - eW) * 30}px)` }}>Sentinal</span>
      <span style={{ fontSize: F.hl.fs * 0.5, lineHeight: 1.3, color: '#A6A6A2', opacity: eT, maxWidth: 900, textWrap: 'balance' }}>Every terminal on every machine, at a glance.</span>
      <div style={{ display: 'flex', flexDirection: F.stack ? 'column' : 'row', alignItems: 'center', gap: 18, marginTop: 12 }}>
        <span style={{ height: 40, padding: '0 16px', display: 'flex', alignItems: 'center', background: '#3D5AFE', color: '#fff', fontSize: 16, fontWeight: 800, letterSpacing: '0.1em', opacity: eS, transform: `scale(${0.9 + eS * 0.1})` }}>COMING SOON</span>
        <span style={{ fontFamily: MONO, fontSize: 22, color: '#F2F2F0', opacity: eG }}>github.com/SentinalResearch</span>
      </div>
    </div>
    <div style={{ position: 'absolute', inset: 0, background: '#0A0A0A', opacity: fade, pointerEvents: 'none' }} />
  </div>;
}

function SentinalFilm(props) {
  const fmt = props.format || '16:9', F = FMT[fmt] || FMT['16:9'];
  return <CompositionStage key={fmt} width={F.w} height={F.h} scenes={window.OM_SCENES} playback={window.OM_PLAYBACK} bg="#0A0A0A"><Piece F={F} /></CompositionStage>;
}
window.SentinalFilm = SentinalFilm;
