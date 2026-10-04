import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { sessionKind } from "../../data/format";
import type { Session } from "../../data/types";
import { SessionTopBlock } from "./SessionTopBlock";

const base: Session = {
  id: "pop:1",
  machine: "pop",
  name: "Qwen Training",
  command: "python train.py",
  state: "RUNNING",
  runningFor: "1h33m",
  progress: { current: 70, total: 100, unit: "epoch" },
};

function renderBlock(session: Session) {
  return render(
    <SessionTopBlock
      session={session}
      kind={sessionKind(session)}
      interrupted={false}
      lastCheckpoint={null}
      canSendInput
      onAnswer={() => undefined}
      onRerun={() => undefined}
      onShowOutput={() => undefined}
    />,
  );
}

describe("SessionTopBlock training", () => {
  it("shows loss read from the run's output with its trend and learning rate", () => {
    renderBlock({
      ...base,
      training: { loss: 0.362, firstLoss: 1.84, bestValLoss: 0.41, learningRate: 0.0002, trend: "IMPROVING" },
    });
    expect(screen.getByText("LOSS")).toBeInTheDocument();
    expect(screen.getByText("0.362")).toBeInTheDocument();
    expect(screen.getByText(/from 1\.84/)).toBeInTheDocument();
    expect(screen.getByText(/improving/)).toBeInTheDocument();
    expect(screen.getByText(/lr 2e-4/)).toBeInTheDocument();
    expect(screen.getByText(/best val 0\.41/)).toBeInTheDocument();
  });

  it("is a training block before any resource sample arrives", () => {
    renderBlock({ ...base, training: { loss: 0.5, trend: "UNKNOWN" } });
    expect(screen.getByText("70")).toBeInTheDocument();
    expect(screen.getByText("0.5")).toBeInTheDocument();
  });
});
