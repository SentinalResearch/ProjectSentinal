import { describe, expect, it } from "vitest";

import { frequentTokens } from "./outputTokens";

describe("chips from recent output", () => {
  it("offers the most frequent words, ignoring numbers and filler", () => {
    const text = "epoch 66/100 loss 0.39\nepoch 67/100 loss 0.38\nCUDA error: out of memory\nthe loss";
    expect(frequentTokens(text, 4)).toEqual(["loss", "epoch", "CUDA", "error"]);
  });
});
