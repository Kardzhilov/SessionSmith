import { createContext, useContext, useEffect, useEffectEvent, useRef, useState, type ReactNode } from "react";
import {
  AudioLines,
  ChevronDown,
  ChevronUp,
  CircleAlert,
  LoaderCircle,
  Pause,
  Play,
  Square,
  Volume2,
  VolumeX,
} from "lucide-react";
import { adaptAudioPlayerTransition } from "../../api/adapters";
import { desktop, errorMessage } from "../../api/desktop";
import { events } from "../../api/generated/bindings";
import type { AudioPlayerSnapshot } from "../../api/types";
import { useAppSettings } from "../settings/AppSettingsContext";

type AudioBusy = "toggle" | "seek" | "stop" | null;

type GlobalAudioContextValue = {
  snapshot: AudioPlayerSnapshot;
  busy: AudioBusy;
  error: string | null;
  activeSource: { campaignId: string; stem: string } | null;
  playSession: (campaignId: string, stem: string) => Promise<void>;
  playFrom: (campaignId: string, stem: string, positionMs: number) => Promise<void>;
  seekSession: (campaignId: string, stem: string, positionMs: number) => Promise<void>;
};

const unloadedAudioSnapshot: AudioPlayerSnapshot = {
  status: "unloaded",
  label: null,
  sourceId: null,
  revision: 0,
  positionMs: 0,
  durationMs: null,
  volume: 50,
  error: null,
};

const GlobalAudioContext = createContext<GlobalAudioContextValue | null>(null);

export function GlobalAudioProvider({ children }: { children: ReactNode }) {
  const { settings, save } = useAppSettings();
  const [snapshot, setSnapshot] = useState<AudioPlayerSnapshot>({
    ...unloadedAudioSnapshot,
    volume: settings.playerVolume,
  });
  const [busy, setBusy] = useState<AudioBusy>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeSource, setActiveSource] = useState<{ campaignId: string; stem: string } | null>(null);
  const sourceIdRef = useRef<number | null>(null);
  const sourceKeyRef = useRef<string | null>(null);
  const revisionRef = useRef(0);
  const operationRef = useRef(0);
  const volumeTimerRef = useRef<number | null>(null);

  const applySnapshot = useEffectEvent((nextSnapshot: AudioPlayerSnapshot, adoptSource = false) => {
    if (nextSnapshot.revision < revisionRef.current) {
      return;
    }
    if (!adoptSource && nextSnapshot.sourceId !== sourceIdRef.current) {
      return;
    }
    if (adoptSource) {
      sourceIdRef.current = nextSnapshot.sourceId;
    }
    revisionRef.current = nextSnapshot.revision;
    setSnapshot(nextSnapshot);
    setError(nextSnapshot.error);
  });

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void desktop.audioState().then((current) => {
      if (active) {
        applySnapshot(current, true);
      }
    }).catch((nextError) => {
      if (active) {
        setError(errorMessage(nextError));
      }
    });
    void events.audioTransition.listen(({ payload }) => {
      if (active) {
        applySnapshot(adaptAudioPlayerTransition(payload).snapshot);
      }
    }).then((stopListening) => {
      if (active) {
        unlisten = stopListening;
      } else {
        stopListening();
      }
    });
    return () => {
      active = false;
      unlisten?.();
      if (volumeTimerRef.current !== null) {
        window.clearTimeout(volumeTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (snapshot.status !== "playing") {
      return;
    }
    const timer = window.setInterval(() => {
      void desktop.audioState().then((current) => applySnapshot(current)).catch((nextError) => {
        setError(errorMessage(nextError));
      });
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [snapshot.status]);

  async function loadSession(campaignId: string, stem: string, operation: number) {
    const sourceKey = JSON.stringify([campaignId, stem]);
    if (sourceKeyRef.current === sourceKey && sourceIdRef.current !== null) {
      return sourceIdRef.current;
    }
    const loaded = await desktop.audioLoad(campaignId, stem);
    if (operation !== operationRef.current) {
      return null;
    }
    if (loaded.sourceId === null) {
      throw new Error("The session audio source could not be loaded.");
    }
    sourceKeyRef.current = sourceKey;
    setActiveSource({ campaignId, stem });
    applySnapshot(loaded, true);
    return loaded.sourceId;
  }

  async function runSessionAction(
    kind: Exclude<AudioBusy, null>,
    campaignId: string,
    stem: string,
    action: (sourceId: number) => Promise<AudioPlayerSnapshot>,
    rethrow = false,
  ) {
    const operation = ++operationRef.current;
    setBusy(kind);
    setError(null);
    try {
      const sourceId = await loadSession(campaignId, stem, operation);
      if (sourceId === null || operation !== operationRef.current) {
        return;
      }
      applySnapshot(await action(sourceId));
    } catch (nextError) {
      if (operation === operationRef.current) {
        setError(errorMessage(nextError));
      }
      if (rethrow) {
        throw nextError;
      }
    } finally {
      if (operation === operationRef.current) {
        setBusy(null);
      }
    }
  }

  async function playSession(campaignId: string, stem: string) {
    if (sourceKeyRef.current === JSON.stringify([campaignId, stem]) && snapshot.status === "playing") {
      await togglePlayback();
      return;
    }
    await runSessionAction("toggle", campaignId, stem, (sourceId) => desktop.audioPlay(sourceId));
  }

  async function seekSession(campaignId: string, stem: string, positionMs: number) {
    const target = Math.max(0, Math.min(Number.MAX_SAFE_INTEGER, Math.round(positionMs)));
    await runSessionAction("seek", campaignId, stem, (sourceId) => desktop.audioSeek(sourceId, target));
  }

  async function playFrom(campaignId: string, stem: string, positionMs: number) {
    const target = Math.max(0, Math.min(Number.MAX_SAFE_INTEGER, Math.round(positionMs)));
    await runSessionAction("seek", campaignId, stem, async (sourceId) => {
      applySnapshot(await desktop.audioSeek(sourceId, target));
      return desktop.audioPlay(sourceId);
    }, true);
  }

  async function togglePlayback() {
    if (busy || sourceIdRef.current === null) {
      return;
    }
    setBusy("toggle");
    setError(null);
    try {
      const sourceId = sourceIdRef.current;
      const nextSnapshot = snapshot.status === "playing"
        ? await desktop.audioPause(sourceId)
        : await desktop.audioPlay(sourceId);
      applySnapshot(nextSnapshot);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(null);
    }
  }

  async function stopPlayback() {
    if (busy || sourceIdRef.current === null) {
      return;
    }
    setBusy("stop");
    setError(null);
    try {
      applySnapshot(await desktop.audioStop(sourceIdRef.current));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(null);
    }
  }

  async function seekActive(positionMs: number) {
    if (busy || sourceIdRef.current === null) {
      return;
    }
    const target = Math.max(0, Math.min(Number.MAX_SAFE_INTEGER, Math.round(positionMs)));
    setBusy("seek");
    setError(null);
    try {
      applySnapshot(await desktop.audioSeek(sourceIdRef.current, target));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(null);
    }
  }

  function setVolume(volume: number) {
    const nextVolume = Math.max(0, Math.min(100, Math.round(volume)));
    setSnapshot((current) => ({ ...current, volume: nextVolume }));
    if (volumeTimerRef.current !== null) {
      window.clearTimeout(volumeTimerRef.current);
    }
    volumeTimerRef.current = window.setTimeout(() => {
      volumeTimerRef.current = null;
      void desktop.audioSetVolume(sourceIdRef.current, nextVolume)
        .then((nextSnapshot) => {
          applySnapshot(nextSnapshot);
          return save({ ...settings, playerVolume: nextVolume });
        })
        .catch((nextError) => setError(errorMessage(nextError)));
    }, 250);
  }

  return (
    <GlobalAudioContext.Provider value={{ snapshot, busy, error, activeSource, playSession, playFrom, seekSession }}>
      {children}
      <GlobalAudioDock
        snapshot={snapshot}
        busy={busy}
        error={error}
        onToggle={() => void togglePlayback()}
        onSeek={(positionMs) => void seekActive(positionMs)}
        onStop={() => void stopPlayback()}
        onVolume={setVolume}
      />
    </GlobalAudioContext.Provider>
  );
}

export function useGlobalAudio() {
  const context = useContext(GlobalAudioContext);
  if (!context) {
    throw new Error("useGlobalAudio must be used inside GlobalAudioProvider");
  }
  return context;
}

function GlobalAudioDock({
  snapshot,
  busy,
  error,
  onToggle,
  onSeek,
  onStop,
  onVolume,
}: {
  snapshot: AudioPlayerSnapshot;
  busy: AudioBusy;
  error: string | null;
  onToggle: () => void;
  onSeek: (positionMs: number) => void;
  onStop: () => void;
  onVolume: (volume: number) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const [scrubbing, setScrubbing] = useState(false);
  const [seekPosition, setSeekPosition] = useState(0);
  const previousVolumeRef = useRef(Math.max(snapshot.volume, 50));
  const loaded = snapshot.status !== "unloaded";
  const playing = snapshot.status === "playing";
  const canSeek = loaded && snapshot.durationMs !== null;
  const maximum = Math.max(snapshot.durationMs ?? 0, 1);
  const position = scrubbing ? seekPosition : snapshot.positionMs;
  const message = error ?? snapshot.error;

  useEffect(() => {
    if (!scrubbing) {
      setSeekPosition(snapshot.positionMs);
    }
  }, [scrubbing, snapshot.positionMs]);

  function finishSeeking(positionMs: number) {
    if (!scrubbing) {
      return;
    }
    const target = Math.max(0, Math.min(maximum, Math.round(positionMs)));
    setScrubbing(false);
    setSeekPosition(target);
    onSeek(target);
  }

  function changeVolume(volume: number) {
    if (volume > 0) {
      previousVolumeRef.current = volume;
    }
    onVolume(volume);
  }

  return (
    <section className={collapsed ? "global-audio global-audio--collapsed" : "global-audio"} aria-label="Global audio player">
      <div className="audio-transport__identity">
        <AudioLines size={17} aria-hidden="true" />
        <span title={snapshot.label ?? "No audio selected"}>{snapshot.label ?? "No audio selected"}</span>
      </div>
      {!collapsed && (
        <>
          <div className="audio-transport__controls">
            <button className="icon-button audio-transport__button audio-transport__button--play" type="button" onClick={onToggle} disabled={!loaded || busy !== null} aria-label={playing ? "Pause audio" : "Play audio"} title={playing ? "Pause audio" : "Play audio"}>
              {busy === "toggle" ? <LoaderCircle className="is-spinning" size={17} aria-hidden="true" /> : playing ? <Pause size={17} aria-hidden="true" /> : <Play size={17} aria-hidden="true" />}
            </button>
            <button className="icon-button audio-transport__button" type="button" onClick={onStop} disabled={!loaded || busy !== null} aria-label="Stop audio" title="Stop audio">
              {busy === "stop" ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Square size={15} aria-hidden="true" />}
            </button>
          </div>
          <div className="audio-transport__timeline">
            <span className="audio-transport__time">{formatPlaybackTime(position)}</span>
            <input
              type="range"
              min="0"
              max={maximum}
              value={Math.min(position, maximum)}
              step="100"
              disabled={!canSeek || busy !== null}
              aria-label="Audio position"
              onPointerDown={(event) => { setScrubbing(true); setSeekPosition(Number(event.currentTarget.value)); }}
              onChange={(event) => { setScrubbing(true); setSeekPosition(Number(event.currentTarget.value)); }}
              onPointerUp={(event) => finishSeeking(Number(event.currentTarget.value))}
              onBlur={(event) => finishSeeking(Number(event.currentTarget.value))}
              onKeyUp={(event) => {
                if (["ArrowLeft", "ArrowRight", "Home", "End", "PageDown", "PageUp"].includes(event.key)) {
                  finishSeeking(Number(event.currentTarget.value));
                }
              }}
            />
            <span className="audio-transport__time">{snapshot.durationMs === null ? "--:--" : formatPlaybackTime(snapshot.durationMs)}</span>
          </div>
          <div className="audio-transport__volume">
            <button className="icon-button audio-transport__button" type="button" onClick={() => changeVolume(snapshot.volume === 0 ? previousVolumeRef.current : 0)} aria-label={snapshot.volume === 0 ? "Restore audio volume" : "Mute audio"} title={snapshot.volume === 0 ? "Restore audio volume" : "Mute audio"}>
              {snapshot.volume === 0 ? <VolumeX size={16} aria-hidden="true" /> : <Volume2 size={16} aria-hidden="true" />}
            </button>
            <input type="range" min="0" max="100" value={snapshot.volume} aria-label={`Audio volume ${snapshot.volume}%`} onChange={(event) => changeVolume(Number(event.currentTarget.value))} />
          </div>
          <span className="audio-transport__status">{formatAudioStatus(snapshot.status)}</span>
          {message && <p className="audio-transport__error" role="alert"><CircleAlert size={15} aria-hidden="true" />{message}</p>}
        </>
      )}
      <button className="icon-button global-audio__collapse" type="button" onClick={() => setCollapsed((current) => !current)} aria-expanded={!collapsed} aria-label={collapsed ? "Expand audio player" : "Collapse audio player"} title={collapsed ? "Expand audio player" : "Collapse audio player"}>
        {collapsed ? <ChevronUp size={17} aria-hidden="true" /> : <ChevronDown size={17} aria-hidden="true" />}
      </button>
    </section>
  );
}

function formatPlaybackTime(milliseconds: number) {
  const totalSeconds = Math.max(0, Math.floor(milliseconds / 1_000));
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatAudioStatus(status: AudioPlayerSnapshot["status"]) {
  return status.charAt(0).toUpperCase() + status.slice(1);
}