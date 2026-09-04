import { useEffect, useRef, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Download, LoaderCircle, RefreshCw } from "lucide-react";
import { errorMessage } from "../../api/desktop";

type UpdatePhase = "checking" | "current" | "available" | "downloading" | "installing" | "error";

export function AppUpdateControl({ currentVersion }: { currentVersion: string }) {
  const updateRef = useRef<Update | null>(null);
  const checkSequence = useRef(0);
  const [phase, setPhase] = useState<UpdatePhase>("checking");
  const [nextVersion, setNextVersion] = useState<string | null>(null);
  const [downloaded, setDownloaded] = useState(0);
  const [downloadSize, setDownloadSize] = useState<number | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const checkForUpdate = async () => {
    const sequence = ++checkSequence.current;
    setPhase("checking");
    setFailure(null);
    setNextVersion(null);
    try {
      await updateRef.current?.close();
      const update = await check({ timeout: 30_000 });
      if (sequence !== checkSequence.current) {
        await update?.close();
        return;
      }
      updateRef.current = update;
      if (update) {
        setNextVersion(update.version);
        setPhase("available");
      } else {
        setPhase("current");
      }
    } catch (nextError) {
      if (sequence !== checkSequence.current) return;
      setFailure(errorMessage(nextError));
      setPhase("error");
    }
  };

  useEffect(() => {
    void checkForUpdate();
    return () => {
      checkSequence.current += 1;
      void updateRef.current?.close();
    };
  }, []);

  const installUpdate = async () => {
    const update = updateRef.current;
    if (!update) return;
    setPhase("downloading");
    setDownloaded(0);
    setDownloadSize(null);
    setFailure(null);
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setDownloadSize(event.data.contentLength ?? null);
        } else if (event.event === "Progress") {
          setDownloaded((bytes) => bytes + event.data.chunkLength);
        } else {
          setPhase("installing");
        }
      });
      await relaunch();
    } catch (nextError) {
      setFailure(errorMessage(nextError));
      setPhase("error");
    }
  };

  const progress = downloadSize ? Math.min(100, Math.round((downloaded / downloadSize) * 100)) : null;
  const message = phase === "checking"
    ? "Checking GitHub for updates..."
    : phase === "current"
      ? `Version ${currentVersion} is up to date.`
      : phase === "available"
        ? `Version ${nextVersion} is available.`
        : phase === "downloading"
          ? progress === null ? "Downloading update..." : `Downloading update: ${progress}%`
          : phase === "installing"
            ? "Installing update..."
            : failure ?? "Update check failed.";

  return (
    <div className="app-update" aria-live="polite">
      <p className={phase === "error" ? "app-update__status app-update__status--error" : "app-update__status"} role={phase === "error" ? "alert" : undefined}>
        {(phase === "checking" || phase === "downloading" || phase === "installing") && <LoaderCircle className="is-spinning" size={15} aria-hidden="true" />}
        {message}
      </p>
      {phase === "downloading" && progress !== null && <progress aria-label="Update download progress" max="100" value={progress}>{progress}%</progress>}
      <div className="app-update__actions">
        {phase === "available" && (
          <button className="button button--primary" type="button" onClick={() => void installUpdate()}>
            <Download size={16} aria-hidden="true" />Download and install
          </button>
        )}
        {(phase === "current" || phase === "error") && (
          <button className="button button--quiet" type="button" onClick={() => void checkForUpdate()}>
            <RefreshCw size={16} aria-hidden="true" />Check again
          </button>
        )}
      </div>
    </div>
  );
}