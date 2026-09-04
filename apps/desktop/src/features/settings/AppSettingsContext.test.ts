import { describe, expect, it } from "vitest";
import type { ThemePalette } from "../../api/types";
import { accessibleThemeProperties, contrastRatio, resolveAppearance } from "./AppSettingsContext";

const lowContrastTheme: ThemePalette = {
  id: "washed-out",
  name: "Washed out",
  bg: "#f8f8f8",
  fg: "#eeeeee",
  primary: "#efefef",
  accent: "#ededed",
  success: "#ececec",
  warn: "#ebebeb",
  error: "#eaeaea",
  muted: "#f0f0f0",
  border: "#f1f1f1",
  borderFocus: "#f2f2f2",
  selectionBg: "#f3f3f3",
  selectionFg: "#f4f4f4",
};

describe("accessible custom themes", () => {
  it("corrects text, state, and border roles to WCAG contrast targets", () => {
    const colors = accessibleThemeProperties(lowContrastTheme);
    const surface = colors["--surface-raised"];

    expect(contrastRatio(colors["--ink"], surface)).toBeGreaterThanOrEqual(4.5);
    for (const role of ["--muted", "--faint", "--line-strong", "--pine", "--amber", "--red", "--blue"] as const) {
      expect(contrastRatio(colors[role], surface), role).toBeGreaterThanOrEqual(4.5);
    }
    expect(contrastRatio(colors["--line"], surface)).toBeGreaterThanOrEqual(3);
    expect(contrastRatio(colors["--pine"], colors["--pine-soft"])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(colors["--amber"], colors["--amber-soft"])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(colors["--red"], colors["--red-soft"])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(colors["--on-primary"], colors["--pine-dark"])).toBeGreaterThanOrEqual(4.5);
  });
});

describe("appearance resolution", () => {
  it("uses fixed modes immediately and resolves system mode", () => {
    expect(resolveAppearance("light", true)).toBe("light");
    expect(resolveAppearance("dark", false)).toBe("dark");
    expect(resolveAppearance("system", false)).toBe("light");
    expect(resolveAppearance("system", true)).toBe("dark");
  });
});