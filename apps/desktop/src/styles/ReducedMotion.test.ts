import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const appCss = readFileSync(`${process.cwd()}/src/styles/app.css`, "utf8");

describe("global reduced motion fallback", () => {
  it("disables animation, transitions, and smooth scrolling when requested", () => {
    expect(appCss).toContain("@media (prefers-reduced-motion: reduce)");
    expect(appCss).toContain("animation-duration: 0.01ms !important");
    expect(appCss).toContain("transition-duration: 0.01ms !important");
    expect(appCss).toContain("scroll-behavior: auto !important");
  });
});