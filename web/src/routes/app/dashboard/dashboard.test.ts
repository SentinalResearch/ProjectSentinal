import { describe, expect, it } from "vitest";

import type { Machine, Session } from "../../../data/types";
import { awayFeed } from "./feed";
import { recordSample, sparkBars, sparkValue, SPARK_SAMPLES } from "./spark";

const machines: Machine[] = [
  { id: "rtx", name: "RTX-PC", os: "Linux", hardware: "", status: "ONLINE" },
  { id: "mbp", name: "MacBook", os: "macOS", hardware: "", status: "OFFLINE", lastSeen: "14m" },
];

describe("While you were away", () => {
  const sessions: Session[] = [
    {
      id: "rtx:1",
      machine: "rtx",
      name: "Qwen Training",
      state: "RUNNING",
      events: [
        ["12:42", "COMMAND_STARTED", "python train.py", 1_000],
        ["12:43", "PROCESS_STARTED", "hf-xet-62 · pid 1", 2_000],
        ["13:58", "RESOURCE_HIGH", "VRAM 96.5 %", 5_000],
        ["14:13", "PROGRESS_UPDATED", "epoch 68/100", 9_000],
      ],
    },
    { id: "rtx:2", machine: "rtx", name: "pytest", state: "FAILED", events: [["13:31", "COMMAND_FINISHED", "exit 1 · 42s", 7_000]] },
  ];

  it("lists notable Events newest first, without process churn", () => {
    const feed = awayFeed(sessions, machines);
    expect(feed.map((item) => item.headline)).toEqual([
      "Qwen Training",
      "pytest failed",
      "VRAM usage high",
      "Qwen Training started",
      "MacBook went offline",
    ]);
    expect(feed[2].detail).toBe("RTX-PC · VRAM 96.5 %");
    expect(feed[4].detail).toBe("last seen 14m ago");
  });

  it("orders fixture events by their clock time when they carry no timestamp", () => {
    const untimed: Session[] = [
      { id: "a", machine: "rtx", name: "a", state: "RUNNING", events: [["09:00", "COMMAND_STARTED", "x"]] },
      { id: "b", machine: "rtx", name: "b", state: "RUNNING", events: [["10:30", "COMMAND_STARTED", "y"]] },
    ];
    expect(awayFeed(untimed, [machines[0]]).map((item) => item.headline)).toEqual(["b started", "a started"]);
  });

  it("keeps room for offline Machines within the limit", () => {
    expect(awayFeed(sessions, machines, 2).map((item) => item.headline)).toEqual(["Qwen Training", "MacBook went offline"]);
  });
});

describe("sparklines", () => {
  it("plot progress, else GPU/CPU use of a live Session, else nothing", () => {
    expect(sparkValue({ id: "a", machine: "m", name: "a", state: "RUNNING", progress: { current: 68, total: 100, unit: "epoch" } })).toBe(68);
    expect(sparkValue({ id: "a", machine: "m", name: "a", state: "RUNNING", metrics: { gpuPct: 40, cpuPct: 5 } })).toBe(40);
    expect(sparkValue({ id: "a", machine: "m", name: "a", state: "IDLE", metrics: { cpuPct: 5 } })).toBeNull();
  });

  it("keep the last 20 samples per Session and forget finished ones", () => {
    let history = {};
    const live: Session = { id: "a", machine: "m", name: "a", state: "RUNNING", metrics: { cpuPct: 10 } };
    for (let i = 0; i < 25; i += 1) history = recordSample(history, [live]);
    expect((history as Record<string, unknown[]>).a).toHaveLength(SPARK_SAMPLES);
    expect(recordSample(history, [])).toEqual({});
  });

  it("draw exactly 20 bars scaled to the window's peak, dashes for gaps", () => {
    const bars = sparkBars([null, 50, 100]);
    expect(bars).toHaveLength(20);
    expect(bars.slice(0, 18)).toEqual(Array(18).fill(null));
    expect(bars.slice(18)).toEqual([0.5, 1]);
  });
});
