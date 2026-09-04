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

const builtInThemes: ThemePalette[] = [
  { ...lowContrastTheme, id: "midnight", name: "Midnight", bg: "#171927", fg: "#d0d0e0", primary: "#7aa2f7", accent: "#bb9af7", error: "#f7768e", muted: "#7b84a8", border: "#3b4261", borderFocus: "#7aa2f7" },
  { ...lowContrastTheme, id: "solar", name: "Solar", bg: "#fdf6e3", fg: "#657b83", primary: "#268bd2", accent: "#d33682", error: "#dc322f", muted: "#839496", border: "#93a1a1", borderFocus: "#268bd2" },
  { ...lowContrastTheme, id: "gruvbox", name: "Gruvbox", bg: "#282828", fg: "#ebdbb2", primary: "#83a598", accent: "#fabd2f", error: "#fb4934", muted: "#a89984", border: "#504945", borderFocus: "#fabd2f" },
  { ...lowContrastTheme, id: "paper", name: "Paper", bg: "#f8f7f2", fg: "#2a2a33", primary: "#1d4ed8", accent: "#7c3aed", error: "#b91c1c", muted: "#6b7280", border: "#9ca3af", borderFocus: "#1d4ed8" },
];

describe("accessible custom themes", () => {
  it("corrects text, state, and border roles to WCAG contrast targets", () => {
    const colors = accessibleThemeProperties(lowContrastTheme, "light");
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

  it.each(builtInThemes)("creates complete readable light and dark variants for $name", (theme) => {
    const light = accessibleThemeProperties(theme, "light");
    const dark = accessibleThemeProperties(theme, "dark");

    expect(light["--canvas"]).not.toBe(dark["--canvas"]);
    expect(contrastRatio(light["--canvas"], "#ffffff")).toBeLessThan(1.6);
    expect(contrastRatio(dark["--canvas"], "#000000")).toBeLessThan(2.5);
    for (const colors of [light, dark]) {
      expect(contrastRatio(colors["--ink"], colors["--surface-raised"])).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(colors["--muted"], colors["--surface-raised"])).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(colors["--line"], colors["--surface-raised"])).toBeGreaterThanOrEqual(3);
      expect(contrastRatio(colors["--focus"], colors["--canvas"])).toBeGreaterThanOrEqual(3);
    }
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