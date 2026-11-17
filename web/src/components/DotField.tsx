import { useEffect, useRef } from "react";

import styles from "./DotField.module.css";

/** What the screen is doing; each signal changes how the dots move. */
export type DotSignal = "idle" | "busy" | "error" | "ok";

const PITCH = 24;
const DOT = 1.1;
const BASE = 0.1;
const TORCH_RADIUS = 170;
const TORCH = 0.32;
const RIPPLE_SPEED = 0.55; // px per ms
const RIPPLE_WIDTH = 46;
const RIPPLE_LIFE = 1700; // ms
const BUSY_EVERY = 950; // ms

interface Ripple {
  x: number;
  y: number;
  start: number;
  colour: string;
  strength: number;
  speed: number;
}

/** A CSS colour token as "r, g, b" (falls back when the token is missing). */
function rgb(token: string, fallback: string): string {
  const probe = document.createElement("span");
  probe.style.color = `var(${token}, ${fallback})`;
  document.body.appendChild(probe);
  const value = getComputedStyle(probe).color;
  probe.remove();
  const parts = value.match(/\d+(\.\d+)?/g);
  return parts && parts.length >= 3 ? parts.slice(0, 3).join(", ") : "128, 128, 128";
}

/**
 * The sign-in screens' dot grid, alive: a slow wave at rest, a soft torch
 * around the pointer, and ripples from the Sentinal mark that say what is
 * happening — pulses while checking, one red ripple for a wrong password,
 * one wide blue wave when you are in. Still and faint with reduced motion.
 * Ripples start at the element marked `data-dots-origin`.
 */
export function DotField({ signal = "idle" }: { signal?: DotSignal }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const ripples = useRef<Ripple[]>([]);
  const signalRef = useRef<DotSignal>(signal);
  const lastBusy = useRef(0);
  const origin = useRef<{ x: number; y: number } | null>(null);

  // Each change of signal adds its ripple.
  useEffect(() => {
    signalRef.current = signal;
    const node = document.querySelector("[data-dots-origin]");
    const box = node?.getBoundingClientRect();
    const host = canvas.current?.getBoundingClientRect();
    origin.current = box && host ? { x: box.left + box.width / 2 - host.left, y: box.top + box.height / 2 - host.top } : null;
    const at = origin.current ?? { x: (host?.width ?? 0) / 2, y: (host?.height ?? 0) / 2 };
    const now = performance.now();
    if (signal === "error") ripples.current.push({ ...at, start: now, colour: "failed", strength: 0.75, speed: RIPPLE_SPEED });
    if (signal === "ok") ripples.current.push({ ...at, start: now, colour: "accent", strength: 1, speed: RIPPLE_SPEED * 2.2 });
    if (signal === "busy") lastBusy.current = 0;
  }, [signal]);

  useEffect(() => {
    const element = canvas.current;
    const context = element?.getContext("2d");
    if (!element || !context) return;
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    let colours = { ink: rgb("--text-1", "#f2f2f0"), accent: rgb("--accent", "#3d5afe"), failed: rgb("--s-failed-fg", "#ff3b2f") };
    let width = 0;
    let height = 0;
    let pointer: { x: number; y: number } | null = null;
    let frame = 0;

    const resize = () => {
      const ratio = window.devicePixelRatio || 1;
      width = element.clientWidth;
      height = element.clientHeight;
      element.width = Math.round(width * ratio);
      element.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      colours = { ink: rgb("--text-1", "#f2f2f0"), accent: rgb("--accent", "#3d5afe"), failed: rgb("--s-failed-fg", "#ff3b2f") };
    };

    const draw = (now: number) => {
      context.clearRect(0, 0, width, height);
      // Keep the busy heartbeat going while checking.
      if (signalRef.current === "busy" && !reduced && now - lastBusy.current > BUSY_EVERY) {
        lastBusy.current = now;
        const at = origin.current ?? { x: width / 2, y: height / 2 };
        ripples.current.push({ ...at, start: now, colour: "accent", strength: 0.55, speed: RIPPLE_SPEED });
      }
      ripples.current = ripples.current.filter((ripple) => now - ripple.start < RIPPLE_LIFE * (ripple.speed > RIPPLE_SPEED ? 1.4 : 1));

      const cx = width / 2;
      const cy = height * 0.45;
      const reach = Math.hypot(width, height) * 0.62;
      for (let y = PITCH / 2; y < height; y += PITCH) {
        for (let x = PITCH / 2; x < width; x += PITCH) {
          // Fade toward the edges so the field sits behind the content.
          const vignette = Math.max(0, 1 - Math.hypot(x - cx, y - cy) / reach);
          let alpha = BASE * vignette;
          if (!reduced) alpha += 0.035 * vignette * Math.sin(x * 0.012 + y * 0.008 + now * 0.0009);
          let colour = colours.ink;
          if (pointer && !reduced) {
            const distance = Math.hypot(x - pointer.x, y - pointer.y);
            if (distance < TORCH_RADIUS) alpha += TORCH * (1 - distance / TORCH_RADIUS) ** 2;
          }
          let best = 0;
          for (const ripple of ripples.current) {
            const age = now - ripple.start;
            const radius = age * ripple.speed;
            const off = Math.abs(Math.hypot(x - ripple.x, y - ripple.y) - radius);
            if (off > RIPPLE_WIDTH) continue;
            const life = 1 - age / (RIPPLE_LIFE * (ripple.speed > RIPPLE_SPEED ? 1.4 : 1));
            const lift = ripple.strength * life * (1 - off / RIPPLE_WIDTH);
            alpha += lift * 0.6;
            if (lift > best) {
              best = lift;
              colour = ripple.colour === "failed" ? colours.failed : colours.accent;
            }
          }
          if (alpha <= 0.01) continue;
          context.fillStyle = `rgba(${colour}, ${Math.min(0.9, alpha).toFixed(3)})`;
          context.beginPath();
          context.arc(x, y, DOT + (best > 0.2 ? best * 0.9 : 0), 0, Math.PI * 2);
          context.fill();
        }
      }
      if (!reduced) frame = requestAnimationFrame(draw);
    };

    const onMove = (event: PointerEvent) => {
      const box = element.getBoundingClientRect();
      pointer = { x: event.clientX - box.left, y: event.clientY - box.top };
    };
    const onLeave = () => {
      pointer = null;
    };
    const onVisibility = () => {
      cancelAnimationFrame(frame);
      if (!document.hidden) frame = requestAnimationFrame(draw);
    };

    resize();
    frame = requestAnimationFrame(draw);
    window.addEventListener("resize", resize);
    window.addEventListener("pointermove", onMove);
    document.addEventListener("pointerleave", onLeave);
    document.addEventListener("visibilitychange", onVisibility);
    // The theme can change while the screen is open.
    const themeWatch = new MutationObserver(resize);
    themeWatch.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", resize);
      window.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerleave", onLeave);
      document.removeEventListener("visibilitychange", onVisibility);
      themeWatch.disconnect();
    };
  }, []);

  return <canvas ref={canvas} className={styles.field} aria-hidden="true" data-signal={signal} />;
}
