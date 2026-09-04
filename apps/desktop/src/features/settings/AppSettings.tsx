import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { CircleAlert, FolderCog, FolderOpen, LoaderCircle, Palette, Save, SlidersHorizontal } from "lucide-react";
import { applyTheme, resolveAppearance, useAppSettings } from "./AppSettingsContext";
import { desktop, errorMessage } from "../../api/desktop";
import type { Appearance, DateFormat } from "../../api/types";

export function AppSettingsPage({
  onOpenSetup,
  onStorageSaved,
}: {
  onOpenSetup: () => void;
  onStorageSaved: () => Promise<void> | void;
}) {
  const { settings, loading, error, save, reload } = useAppSettings();
  const [dateFormat, setDateFormat] = useState<DateFormat>(settings.dateFormat);
  const [appearance, setAppearance] = useState<Appearance>(settings.appearance);
  const [playerVolume, setPlayerVolume] = useState(settings.playerVolume);
  const [theme, setTheme] = useState(settings.theme);
  const [audioDir, setAudioDir] = useState(settings.audioDir);
  const [campaignsDir, setCampaignsDir] = useState(settings.campaignsDir);
  const [outputDir, setOutputDir] = useState(settings.outputDir);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [version, setVersion] = useState("1.0.0");

  useEffect(() => {
    setDateFormat(settings.dateFormat);
    setAppearance(settings.appearance);
    setPlayerVolume(settings.playerVolume);
    setTheme(settings.theme);
    setAudioDir(settings.audioDir);
    setCampaignsDir(settings.campaignsDir);
    setOutputDir(settings.outputDir);
  }, [settings]);

  useEffect(() => {
    void getVersion().then(setVersion).catch(() => undefined);
  }, []);

  const dirty = dateFormat !== settings.dateFormat
    || appearance !== settings.appearance
    || playerVolume !== settings.playerVolume
    || theme !== settings.theme
    || audioDir !== settings.audioDir
    || campaignsDir !== settings.campaignsDir
    || outputDir !== settings.outputDir;

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const preview = () => {
      const resolvedAppearance = resolveAppearance(appearance, media.matches);
      document.documentElement.dataset.appearance = resolvedAppearance;
      applyTheme(settings.themes.find((palette) => palette.id === theme), resolvedAppearance);
    };
    preview();
    media.addEventListener("change", preview);
    return () => {
      media.removeEventListener("change", preview);
      const resolvedAppearance = resolveAppearance(settings.appearance, media.matches);
      document.documentElement.dataset.appearance = resolvedAppearance;
      applyTheme(settings.themes.find((palette) => palette.id === settings.theme), resolvedAppearance);
    };
  }, [appearance, settings.appearance, settings.theme, settings.themes, theme]);

  const submit = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      const storageChanged = audioDir !== settings.audioDir
        || campaignsDir !== settings.campaignsDir
        || outputDir !== settings.outputDir;
      await save({ dateFormat, appearance, theme, playerVolume, audioDir, campaignsDir, outputDir });
      const audio = await desktop.audioState();
      await desktop.audioSetVolume(audio.sourceId, playerVolume);
      if (storageChanged) await onStorageSaved();
    } catch (nextError) {
      setSaveError(errorMessage(nextError));
    } finally {
      setSaving(false);
    }
  };

  const chooseDirectory = async (current: string, update: (path: string) => void) => {
    setSaveError(null);
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: current });
      if (typeof selected === "string") update(selected);
    } catch (nextError) {
      setSaveError(errorMessage(nextError));
    }
  };

  if (loading) {
    return <div className="page-loading"><LoaderCircle className="is-spinning" size={22} /> Loading app settings</div>;
  }

  return (
    <div className="settings-page app-settings-page">
      <header className="section-heading settings-page__header">
        <div>
          <p className="eyebrow">Application</p>
          <h1>App settings</h1>
          <p>Appearance, formatting, and playback defaults for this device.</p>
        </div>
        <button className="button button--primary" type="button" disabled={!dirty || saving} onClick={() => void submit()}>
          {saving ? <LoaderCircle className="is-spinning" size={16} /> : <Save size={16} />}
          Save
        </button>
      </header>

      {(error || saveError) && (
        <div className="settings-save-error" role="alert">
          <CircleAlert size={15} />
          <span>{saveError ?? error}</span>
          {error && <button type="button" onClick={() => void reload()}>Retry</button>}
        </div>
      )}

      <section className="settings-section app-settings-section">
        <div className="settings-section__heading">
          <div className="settings-section__icon"><FolderCog size={17} /></div>
          <div><h2>Storage directories</h2><p>Choose where SessionSmith reads and writes local campaign data.</p></div>
        </div>
        <div className="app-storage-fields">
          {[
            { id: "audio-directory", label: "Audio files", value: audioDir, update: setAudioDir },
            { id: "campaign-directory", label: "Campaign files", value: campaignsDir, update: setCampaignsDir },
            { id: "output-directory", label: "Generated output", value: outputDir, update: setOutputDir },
          ].map((field) => (
            <label className="app-storage-field" htmlFor={field.id} key={field.id}>
              <span>{field.label}</span>
              <span className="app-storage-field__control">
                <input id={field.id} type="text" value={field.value} onChange={(event) => field.update(event.target.value)} />
                <button className="icon-button" type="button" title={`Choose ${field.label.toLowerCase()} directory`} aria-label={`Choose ${field.label.toLowerCase()} directory`} onClick={() => void chooseDirectory(field.value, field.update)}>
                  <FolderOpen size={17} aria-hidden="true" />
                </button>
              </span>
            </label>
          ))}
        </div>
      </section>

      <section className="settings-section app-settings-section">
        <div className="settings-section__heading">
          <div className="settings-section__icon"><Palette size={17} /></div>
          <div><h2>Appearance</h2><p>Follow your system or choose a fixed mode.</p></div>
        </div>
        <div className="app-settings-controls">
          <label>Color mode
            <select value={appearance} onChange={(event) => setAppearance(event.target.value as Appearance)}>
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
          <label>Theme
            <select value={theme} onChange={(event) => setTheme(event.target.value)}>
              {settings.themes.map((palette) => <option value={palette.id} key={palette.id}>{palette.name}</option>)}
            </select>
          </label>
        </div>
        <div className="theme-preview" aria-label="Theme color preview">
          {settings.themes.find((palette) => palette.id === theme) && Object.entries({
            Primary: settings.themes.find((palette) => palette.id === theme)!.primary,
            Accent: settings.themes.find((palette) => palette.id === theme)!.accent,
            Success: settings.themes.find((palette) => palette.id === theme)!.success,
            Warning: settings.themes.find((palette) => palette.id === theme)!.warn,
            Error: settings.themes.find((palette) => palette.id === theme)!.error,
            Muted: settings.themes.find((palette) => palette.id === theme)!.muted,
          }).map(([label, color]) => (
            <span className="theme-preview__swatch" key={label} title={`${label}: ${color}`} style={{ backgroundColor: color }}><span>{label}</span></span>
          ))}
        </div>
      </section>

      <section className="settings-section app-settings-section">
        <div className="settings-section__heading">
          <div className="settings-section__icon"><SlidersHorizontal size={17} /></div>
          <div><h2>Formats and playback</h2><p>Used throughout the library and session workspace.</p></div>
        </div>
        <div className="app-settings-controls">
          <label>Date format
            <select value={dateFormat} onChange={(event) => setDateFormat(event.target.value as DateFormat)}>
              <option value="dmy">23 August 2026</option>
              <option value="mdy">August 23, 2026</option>
              <option value="ymd">2026 August 23</option>
              <option value="iso">2026-08-23</option>
            </select>
          </label>
          <label>Default player volume: {playerVolume}%
            <input type="range" min="0" max="100" value={playerVolume} onChange={(event) => setPlayerVolume(Number(event.target.value))} />
          </label>
        </div>
      </section>

      <section className="settings-section app-settings-about">
        <p className="eyebrow">About</p>
        <h2>SessionSmith {version}</h2>
        <p>Local-first TTRPG transcription and session notes.</p>
        <button className="button button--quiet" type="button" onClick={onOpenSetup}>Run operational setup</button>
      </section>
    </div>
  );
}