import { describe, expect, it } from "vitest";

import { escapeTemplateLiteral, previewTemplate } from "./recipeTemplate";

describe("recipe templates", () => {
  it("escapes literal braces the way the daemon's Recipe grammar expects", () => {
    expect(escapeTemplateLiteral("'{print $1}'")).toBe("'{{print $1}}'");
  });

  it("previews values and collapses escaped braces back to literals", () => {
    expect(previewTemplate("awk '{{print $1}}' {file}", { file: "data.csv" })).toBe("awk '{print $1}' data.csv");
    expect(previewTemplate("echo {missing}", {})).toBe("echo {missing}");
  });
});
