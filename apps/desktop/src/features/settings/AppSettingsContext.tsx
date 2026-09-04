import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import { desktop, errorMessage } from "../../api/desktop";
import type { AppSettings, AppSettingsWriteRequest, DateFormat } from "../../api/types";

const fallbackSettings: AppSettings = {
  revision: "",
  dateFormat: "dmy",
  appearance: "system",
  theme: "default",
  playerVolume: 50,
  themes: [],
};

type AppSettingsContextValue = {
  settings: AppSettings;
  loading: boolean;
  error: string | null;
  save: (settings: Omit<AppSettingsWriteRequest, "expectedRevision">) => Promise<AppSettings>;
  reload: () => Promise<void>;
};

const AppSettingsContext = createContext<AppSettingsContextValue | null>(null);

export function AppSettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState(fallbackSettings);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [systemDark, setSystemDark] = useState(false);

  const reload = async () => {
    setLoading(true);
    setError(null);
    try {
      setSettings(await desktop.appSettings());
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  const resolvedAppearance = resolveAppearance(settings.appearance, systemDark);

  useEffect(() => {
    document.documentElement.dataset.appearance = resolvedAppearance;
    document.documentElement.dataset.theme = settings.theme;
    applyTheme(settings.themes.find((theme) => theme.id === settings.theme), resolvedAppearance);
  }, [resolvedAppearance, settings.theme, settings.themes]);

  const save = async (nextSettings: Omit<AppSettingsWriteRequest, "expectedRevision">) => {
    const current = await desktop.appSettings();
    const saved = await desktop.appSettingsWrite({
      ...nextSettings,
      expectedRevision: current.revision,
    });
    setSettings(saved);
    return saved;
  };

  return (
    <AppSettingsContext.Provider value={{ settings, loading, error, save, reload }}>
      {children}
    </AppSettingsContext.Provider>
  );
}

export function applyTheme(
  theme: AppSettings["themes"][number] | undefined,
  appearance: "light" | "dark",
) {
  const root = document.documentElement;
  const properties: Record<string, string> = theme && theme.id !== "default"
    ? accessibleThemeProperties(theme, appearance)
    : {};
  for (const property of themeProperties) root.style.removeProperty(property);
  for (const [property, value] of Object.entries(properties)) root.style.setProperty(property, value);
  root.style.color = properties["--ink"] ?? "";
  root.style.background = properties["--canvas"] ?? "";
}

export function resolveAppearance(appearance: AppSettings["appearance"], systemDark: boolean) {
  return appearance === "system" ? (systemDark ? "dark" : "light") : appearance;
}

const themeProperties = ["--canvas", "--canvas-deep", "--surface", "--surface-raised", "--ink", "--muted", "--faint", "--line", "--line-strong", "--pine", "--pine-dark", "--pine-soft", "--amber", "--amber-soft", "--red", "--red-soft", "--blue", "--on-primary", "--focus"];

export function accessibleThemeProperties(
  theme: AppSettings["themes"][number],
  appearance: "light" | "dark",
) {
  const nativeDark = relativeLuminance(theme.bg) < 0.32;
  const canvas = appearance === "dark"
    ? (nativeDark ? theme.bg : mixHex(theme.bg, "#000000", 0.82))
    : (nativeDark ? mixHex(theme.bg, "#ffffff", 0.88) : theme.bg);
  const surface = mixHex(canvas, "#ffffff", appearance === "dark" ? 0.07 : 0.55);
  const raised = mixHex(canvas, "#ffffff", appearance === "dark" ? 0.12 : 0.86);
  const canvasDeep = mixHex(canvas, appearance === "dark" ? "#ffffff" : "#000000", appearance === "dark" ? 0.04 : 0.06);
  const primaryBackground = ensureContrast(theme.primary, "#ffffff", 4.5);
  const primarySoft = mixHex(raised, theme.primary, 0.16);
  const accentSoft = mixHex(raised, theme.accent, 0.16);
  const errorSoft = mixHex(raised, theme.error, 0.16);
  return {
    "--canvas": canvas,
    "--canvas-deep": canvasDeep,
    "--surface": surface,
    "--surface-raised": raised,
    "--ink": ensureContrast(theme.fg, raised, 4.5),
    "--muted": ensureContrast(theme.muted, raised, 4.5),
    "--faint": ensureContrast(theme.muted, raised, 4.5),
    "--line": ensureContrast(theme.border, raised, 3),
    "--line-strong": ensureContrast(theme.borderFocus, raised, 4.5),
    "--pine": ensureContrast(theme.primary, primarySoft, 4.5),
    "--pine-dark": primaryBackground,
    "--pine-soft": primarySoft,
    "--amber": ensureContrast(theme.accent, accentSoft, 4.5),
    "--amber-soft": accentSoft,
    "--red": ensureContrast(theme.error, errorSoft, 4.5),
    "--red-soft": errorSoft,
    "--blue": ensureContrast(theme.borderFocus, raised, 4.5),
    "--on-primary": "#ffffff",
    "--focus": ensureContrast(theme.borderFocus, canvas, 3),
  };
}

function ensureContrast(color: string, background: string, minimum: number) {
  if (contrastRatio(color, background) >= minimum) return color;
  const target = contrastRatio("#ffffff", background) >= contrastRatio("#000000", background)
    ? "#ffffff"
    : "#000000";
  let low = 0;
  let high = 1;
  for (let step = 0; step < 12; step += 1) {
    const amount = (low + high) / 2;
    if (contrastRatio(mixHex(color, target, amount), background) >= minimum) high = amount;
    else low = amount;
  }
  return mixHex(color, target, high);
}

export function contrastRatio(foreground: string, background: string) {
  const first = relativeLuminance(foreground);
  const second = relativeLuminance(background);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}

function relativeLuminance(color: string) {
  const channels = parseHex(color).map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function mixHex(base: string, overlay: string, amount: number) {
  const from = parseHex(base);
  const to = parseHex(overlay);
  return `#${from.map((channel, index) => Math.round(channel + (to[index] - channel) * amount).toString(16).padStart(2, "0")).join("")}`;
}

function parseHex(color: string) {
  const hex = color.replace("#", "");
  return [0, 2, 4].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16));
}

export function useAppSettings() {
  const context = useContext(AppSettingsContext);
  if (!context) {
    throw new Error("useAppSettings must be used inside AppSettingsProvider");
  }
  return context;
}

export function formatTimestamp(timestamp: number | null, format: DateFormat) {
  if (!timestamp) {
    return "No modified date";
  }
  const date = new Date(timestamp * 1000);
  if (format === "iso") {
    return new Intl.DateTimeFormat("sv-SE", { year: "numeric", month: "2-digit", day: "2-digit" }).format(date);
  }
  const options: Intl.DateTimeFormatOptions = { year: "numeric", month: "long", day: "numeric" };
  const locale = format === "mdy" ? "en-US" : format === "ymd" ? "ja-JP" : "en-GB";
  return new Intl.DateTimeFormat(locale, options).format(date);
}