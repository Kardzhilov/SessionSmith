import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AudioLines,
  BookOpenText,
  Bot,
  ChevronDown,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Clock3,
  Command,
  FileOutput,
  FolderOpen,
  HeartPulse,
  LibraryBig,
  LoaderCircle,
  Mic,
  PanelLeftClose,
  Plus,
  RefreshCw,
  Radio,
  Search,
  Settings2,
  SlidersHorizontal,
  Sparkles,
  UsersRound,
} from "lucide-react";
import { desktop, errorMessage } from "./api/desktop";
import { ExportDialog, type ExportDialogRequest } from "./features/dialogs/ExportDialog";
import { NotesDialog } from "./features/dialogs/NotesDialog";
import { ProcessDialog, type ProcessDialogRequest } from "./features/dialogs/ProcessDialog";
import { RecordDialog } from "./features/dialogs/RecordDialog";
import { RenameSessionDialog } from "./features/dialogs/RenameSessionDialog";
import { SpeakerReviewDialog } from "./features/dialogs/SpeakerReviewDialog";
import { HealthPage } from "./features/health/Health";
import { JobsFlyout } from "./features/jobs/Jobs";
import { ModelInventoryPage } from "./features/models/Models";
import { NotificationViewport, type AppNotification } from "./features/notifications/Notifications";
import { OnboardingScreen } from "./features/onboarding/Onboarding";
import { SearchDialog } from "./features/search/SearchDialog";
import { AppSettingsPage } from "./features/settings/AppSettings";
import { formatTimestamp, useAppSettings } from "./features/settings/AppSettingsContext";
import { CampaignSettingsPage } from "./features/settings/CampaignSettings";
import { CampaignLogPage, SessionWorkspacePage } from "./features/workspace/Workspace";
import "./styles/tokens.css";
import "./styles/app.css";
import brandIcon from "../src-tauri/icons/icon.png";
import type {
  AppBootstrap,
  ArtifactId,
  CandidateAction,
  CampaignLibrary,
  CampaignSummary,
  DesktopJob,
  HealthReport,
  InboxWatchStatus,
  NotesRequest,
  OnboardingState,
  SearchResult,
  SearchSource,
  SessionSummary,
  SpeakerMapping,
} from "./api/types";

type View = "sessions" | "log" | "settings" | "app-settings" | "models" | "health";
type HealthStatus = "pending" | "ok" | "warn" | "fail";
type CandidateResolutionContext = {
  campaignId: string;
  stem: string;
  artifactId: ArtifactId;
  action: CandidateAction;
};
type SpeakerJobContext = { campaignId: string; stem: string; reset: boolean };
type SessionRenameJobContext = { campaignId: string; oldStem: string; newStem: string };
const searchPreferencesKey = "sessionsmith:search-preferences";

const stoppedInboxWatchStatus: InboxWatchStatus = {
  running: false,
  campaignId: null,
  campaignName: null,
  intervalSecs: null,
  queued: 0,
  processingPath: null,
  overflowCount: 0,
  lastError: null,
};

const audioImportFilter = {
  name: "Audio",
  extensions: ["wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "wma", "webm"],
};

const navItems: Array<{
  id: View;
  label: string;
  icon: typeof LibraryBig;
}> = [
  { id: "sessions", label: "Sessions", icon: LibraryBig },
  { id: "log", label: "Campaign log", icon: BookOpenText },
  { id: "settings", label: "Campaign settings", icon: Settings2 },
  { id: "app-settings", label: "App settings", icon: SlidersHorizontal },
  { id: "models", label: "Models", icon: Bot },
  { id: "health", label: "Health", icon: HeartPulse },
];

function App() {
  const [bootstrap, setBootstrap] = useState<AppBootstrap | null>(null);
  const [library, setLibrary] = useState<CampaignLibrary | null>(null);
  const [activeCampaignId, setActiveCampaignId] = useState<string | null>(null);
  const [activeView, setActiveView] = useState<View>("sessions");
  const [campaignPickerOpen, setCampaignPickerOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [loading, setLoading] = useState(true);
  const [libraryLoading, setLibraryLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeSessionStem, setActiveSessionStem] = useState<string | null>(null);
  const [activeArtifactId, setActiveArtifactId] = useState<ArtifactId | null>(null);
  const [activeArtifactCandidate, setActiveArtifactCandidate] = useState(false);
  const [activeAlternateName, setActiveAlternateName] = useState<string | null>(null);
  const [activeTranscriptLine, setActiveTranscriptLine] = useState<number | null>(null);
  const [healthReport, setHealthReport] = useState<HealthReport | null>(null);
  const [healthLoading, setHealthLoading] = useState(true);
  const [healthError, setHealthError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<DesktopJob[]>([]);
  const [jobsOpen, setJobsOpen] = useState(false);
  const [jobsError, setJobsError] = useState<string | null>(null);
  const [doctorSubmitting, setDoctorSubmitting] = useState(false);
  const [recordDialogOpen, setRecordDialogOpen] = useState(false);
  const [recordSubmitting, setRecordSubmitting] = useState(false);
  const [recordError, setRecordError] = useState<string | null>(null);
  const [processDialogOpen, setProcessDialogOpen] = useState(false);
  const [processSubmitting, setProcessSubmitting] = useState(false);
  const [processError, setProcessError] = useState<string | null>(null);
  const [logRebuildSubmitting, setLogRebuildSubmitting] = useState(false);
  const [logReloadKey, setLogReloadKey] = useState(0);
  const [notesDialogOpen, setNotesDialogOpen] = useState(false);
  const [notesSubmitting, setNotesSubmitting] = useState(false);
  const [notesError, setNotesError] = useState<string | null>(null);
  const [notesStem, setNotesStem] = useState<string | null>(null);
  const [speakerDialogOpen, setSpeakerDialogOpen] = useState(false);
  const [speakerSubmitting, setSpeakerSubmitting] = useState(false);
  const [speakerError, setSpeakerError] = useState<string | null>(null);
  const [speakerStem, setSpeakerStem] = useState<string | null>(null);
  const [candidateResolveSubmitting, setCandidateResolveSubmitting] = useState(false);
  const [candidateResolveError, setCandidateResolveError] = useState<string | null>(null);
  const [campaignLogHandoff, setCampaignLogHandoff] = useState<{ campaignId: string; stem: string } | null>(null);
  const [speakerNotesHandoff, setSpeakerNotesHandoff] = useState<{ campaignId: string; stem: string } | null>(null);
  const [sessionNameHandoff, setSessionNameHandoff] = useState<{ campaignId: string; stem: string } | null>(null);
  const [workspaceReloadKey, setWorkspaceReloadKey] = useState(0);
  const [modelReloadKey, setModelReloadKey] = useState(0);
  const [modelActionError, setModelActionError] = useState<string | null>(null);
  const [importSubmitting, setImportSubmitting] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportSubmitting, setExportSubmitting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [exportInitialStems, setExportInitialStems] = useState<string[] | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchSources, setSearchSources] = useState<SearchSource[]>([]);
  const [searchPreferences, setSearchPreferences] = useState(loadSearchPreferences);
  const [reindexSubmitting, setReindexSubmitting] = useState(false);
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [renameSubmitting, setRenameSubmitting] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [inboxWatchStatus, setInboxWatchStatus] = useState<InboxWatchStatus>(stoppedInboxWatchStatus);
  const [inboxWatchInterval, setInboxWatchInterval] = useState(5);
  const [inboxWatchSubmitting, setInboxWatchSubmitting] = useState(false);
  const [inboxWatchError, setInboxWatchError] = useState<string | null>(null);
  const [onboarding, setOnboarding] = useState<OnboardingState | null>(null);
  const [onboardingLoading, setOnboardingLoading] = useState(true);
  const [onboardingLoadError, setOnboardingLoadError] = useState<string | null>(null);
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  const [onboardingInitialStep, setOnboardingInitialStep] = useState<"health" | "models" | "campaign" | "backend">("health");
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const activeCampaignIdRef = useRef<string | null>(null);
  const candidateResolutionRef = useRef<CandidateResolutionContext | null>(null);
  const speakerJobRef = useRef<SpeakerJobContext | null>(null);
  const sessionRenameJobRef = useRef<SessionRenameJobContext | null>(null);
  const topbarSearchRef = useRef<HTMLInputElement | null>(null);
  const notificationKeysRef = useRef(new Set<string>());
  const notificationIdRef = useRef(0);

  const dismissNotification = useCallback((id: number) => {
    setNotifications((current) => current.filter((notification) => notification.id !== id));
  }, []);

  useEffect(() => {
    activeCampaignIdRef.current = activeCampaignId;
  }, [activeCampaignId]);

  useEffect(() => {
    void refresh();
    void refreshHealth();
    void loadOnboarding();
    void desktop.searchSources().then((sources) => {
      setSearchSources(sources);
      const valid = new Set(sources.map((source) => source.id));
      setSearchPreferences((current) => ({
        ...current,
        sourceKinds: current.sourceKinds.filter((source) => valid.has(source)),
      }));
    }).catch(() => setSearchSources([]));
  }, []);

  useEffect(() => {
    window.localStorage.setItem(searchPreferencesKey, JSON.stringify(searchPreferences));
  }, [searchPreferences]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    void desktop.inboxWatchStatus()
      .then((status) => {
        if (active) setInboxWatchStatus(status);
      })
      .catch((nextError) => {
        if (active) setInboxWatchError(errorMessage(nextError));
      });
    void desktop.inboxWatchListen((status) => {
      if (active) {
        setInboxWatchStatus(status);
        if (status.lastError) {
          pushNotification({
            key: `watch:${status.lastError}`,
            tone: "error",
            title: "Inbox watch error",
            message: status.lastError,
          });
        }
      }
    }).then((stopListening) => {
      if (active) unlisten = stopListening;
      else stopListening();
    }).catch((nextError) => {
      if (active) setInboxWatchError(errorMessage(nextError));
    });

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void desktop.audioListen((transition) => {
      if (active && transition.kind === "error" && transition.snapshot.error) {
        pushNotification({
          key: `audio:${transition.snapshot.sourceId ?? "none"}:${transition.snapshot.error}`,
          tone: "error",
          title: "Audio playback error",
          message: transition.snapshot.error,
        });
      }
    }).then((stopListening) => {
      if (active) unlisten = stopListening;
      else stopListening();
    }).catch((nextError) => {
      if (active) {
        pushNotification({ key: "audio:listener", tone: "error", title: "Audio status unavailable", message: errorMessage(nextError) });
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    function openSearch(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearchOpen(true);
        window.requestAnimationFrame(() => topbarSearchRef.current?.focus());
      }
    }

    window.addEventListener("keydown", openSearch);
    return () => window.removeEventListener("keydown", openSearch);
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    void refreshJobs();
    void desktop.jobListen((payload) => {
      if (!active) {
        return;
      }
      setJobs((currentJobs) => upsertJob(currentJobs, payload));
      if (isTerminalJob(payload)) {
        pushNotification({
          key: `job:${payload.id}:${payload.state}`,
          tone: payload.state === "failed" ? "error" : payload.state === "succeeded" ? "success" : "info",
          title: payload.state === "succeeded" ? `${payload.title} completed` : payload.state === "failed" ? `${payload.title} failed` : `${payload.title} cancelled`,
          message: payload.summary ?? undefined,
        });
      }
      if (payload.kind === "doctor" && isTerminalJob(payload)) {
        void refreshHealth();
      }
      if ((payload.kind === "record" || payload.kind === "import" || payload.kind === "run" || payload.kind === "transcribe" || payload.kind === "notes") && payload.state === "succeeded") {
        const campaignId = activeCampaignIdRef.current;
        if (campaignId) {
          void loadLibrary(campaignId);
        }
      }
      if ((payload.kind === "run" || payload.kind === "transcribe" || payload.kind === "notes") && payload.state === "succeeded") {
        setWorkspaceReloadKey((current) => current + 1);
        const campaignId = activeCampaignIdRef.current;
        const stem = activeSessionStem;
        if (payload.kind === "notes" && campaignId && stem && isDateDerivedSessionStem(stem)) {
          setSessionNameHandoff({ campaignId, stem });
        }
      }
      if (payload.kind === "speakerMap" && payload.state === "succeeded") {
        setWorkspaceReloadKey((current) => current + 1);
        const context = speakerJobRef.current;
        speakerJobRef.current = null;
        if (context && !context.reset) {
          setSpeakerNotesHandoff({ campaignId: context.campaignId, stem: context.stem });
        }
      } else if (payload.kind === "speakerMap" && isTerminalJob(payload)) {
        speakerJobRef.current = null;
      }
      if (payload.kind === "candidateResolve" && isTerminalJob(payload)) {
        const resolution = candidateResolutionRef.current;
        candidateResolutionRef.current = null;
        if (payload.state === "succeeded") {
          setWorkspaceReloadKey((current) => current + 1);
          if (
            resolution
            && resolution.action === "keepCandidate"
            && (resolution.artifactId === "summary" || resolution.artifactId === "bullets")
          ) {
            setCampaignLogHandoff({ campaignId: resolution.campaignId, stem: resolution.stem });
          }
        }
      }
      if (payload.kind === "sessionRename" && isTerminalJob(payload)) {
        const context = sessionRenameJobRef.current;
        sessionRenameJobRef.current = null;
        if (context && payload.state === "succeeded") {
          setActiveSessionStem(context.newStem);
          setWorkspaceReloadKey((current) => current + 1);
          setCampaignLogHandoff({ campaignId: context.campaignId, stem: context.newStem });
          setSessionNameHandoff(null);
          void loadLibrary(context.campaignId);
        } else if (context) {
          setRenameError(payload.summary ?? "The session rename failed.");
          setRenameDialogOpen(true);
        }
      }
      if (payload.kind === "rebuildLog" && payload.state === "succeeded") {
        setLogReloadKey((current) => current + 1);
        setCampaignLogHandoff(null);
      }
      if (payload.kind === "model" && isTerminalJob(payload)) {
        setModelReloadKey((current) => current + 1);
        setModelActionError(payload.state === "failed" ? payload.summary ?? "The local model action failed." : null);
      }
    })
      .then((stopListening) => {
        if (active) {
          unlisten = stopListening;
        } else {
          stopListening();
        }
      })
      .catch((nextError) => {
        if (active) {
          setJobsError(errorMessage(nextError));
        }
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  function pushNotification(notification: Omit<AppNotification, "id">) {
    if (notificationKeysRef.current.has(notification.key)) return;
    if (notificationKeysRef.current.size >= 200) notificationKeysRef.current.clear();
    notificationKeysRef.current.add(notification.key);
    notificationIdRef.current += 1;
    setNotifications((current) => [...current, { ...notification, id: notificationIdRef.current }].slice(-5));
  }

  async function loadOnboarding() {
    setOnboardingLoading(true);
    setOnboardingLoadError(null);
    try {
      const next = await desktop.onboardingState();
      setOnboarding(next);
      setOnboardingOpen(next.required);
    } catch (nextError) {
      setOnboardingLoadError(errorMessage(nextError));
    } finally {
      setOnboardingLoading(false);
    }
  }

  async function completeOnboarding(outcome: "finished" | "skipped") {
    if (!onboarding) return;
    const next = await desktop.onboardingComplete({
      expectedRevision: onboarding.revision,
      version: onboarding.currentVersion,
      outcome,
    });
    setOnboarding(next);
    setOnboardingOpen(false);
    pushNotification({
      key: `onboarding:${next.completedVersion}:${outcome}`,
      tone: outcome === "finished" ? "success" : "info",
      title: outcome === "finished" ? "Setup completed" : "Setup skipped",
      message: "You can run setup again from App Settings.",
    });
  }

  function openOnboarding(step: "health" | "models" | "campaign" | "backend" = "health") {
    setOnboardingInitialStep(step);
    void desktop.onboardingState()
      .then((next) => {
        setOnboarding(next);
        setOnboardingOpen(true);
      })
      .catch((nextError) => {
        pushNotification({ key: `onboarding:open:${errorMessage(nextError)}`, tone: "error", title: "Setup could not be opened", message: errorMessage(nextError) });
      });
  }

  function handoffOnboarding(destination: "health" | "models" | "settings") {
    setOnboardingOpen(false);
    setActiveView(destination);
    setActiveSessionStem(null);
  }

  async function refresh(preferredCampaignId = activeCampaignId) {
    setLoading(true);
    setError(null);
    try {
      const nextBootstrap = await desktop.bootstrap();
      setBootstrap(nextBootstrap);

      const selectedCampaign =
        nextBootstrap.campaigns.find(
          (campaign) => campaign.id === preferredCampaignId && !campaign.loadError,
        ) ?? nextBootstrap.campaigns.find((campaign) => !campaign.loadError);

      if (selectedCampaign) {
        if (inboxWatchStatus.running && inboxWatchStatus.campaignId !== selectedCampaign.id) {
          setInboxWatchStatus(await desktop.inboxWatchStop());
        }
        setActiveCampaignId(selectedCampaign.id);
        await loadLibrary(selectedCampaign.id);
      } else {
        setActiveCampaignId(null);
        setLibrary(null);
        setActiveSessionStem(null);
        setActiveArtifactId(null);
        setActiveArtifactCandidate(false);
        setActiveAlternateName(null);
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
      setBootstrap(null);
      setLibrary(null);
    } finally {
      setLoading(false);
    }
  }

  async function loadLibrary(campaignId: string) {
    setLibraryLoading(true);
    setError(null);
    try {
      setLibrary(await desktop.campaignLibrary(campaignId));
    } catch (nextError) {
      setLibrary(null);
      setError(errorMessage(nextError));
    } finally {
      setLibraryLoading(false);
    }
  }

  async function refreshHealth() {
    setHealthLoading(true);
    setHealthError(null);
    try {
      setHealthReport(await desktop.healthReport());
    } catch (nextError) {
      setHealthError(errorMessage(nextError));
    } finally {
      setHealthLoading(false);
    }
  }

  async function refreshJobs() {
    try {
      setJobs(await desktop.jobsList());
      setJobsError(null);
    } catch (nextError) {
      setJobsError(errorMessage(nextError));
    }
  }

  async function startSystemCheck() {
    setDoctorSubmitting(true);
    setJobsError(null);
    try {
      await desktop.jobSubmitDoctor();
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setJobsError(errorMessage(nextError));
    } finally {
      setDoctorSubmitting(false);
    }
  }

  async function cancelJob(jobId: number) {
    setJobsError(null);
    try {
      await desktop.jobCancel(jobId);
      await refreshJobs();
    } catch (nextError) {
      setJobsError(errorMessage(nextError));
    }
  }

  async function startRecording(name: string) {
    setRecordSubmitting(true);
    setRecordError(null);
    try {
      await desktop.jobSubmitRecord({ name });
      setRecordDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setRecordError(errorMessage(nextError));
    } finally {
      setRecordSubmitting(false);
    }
  }

  async function startAudioImport() {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId) {
      return;
    }

    setImportSubmitting(true);
    setJobsError(null);
    try {
      const selected = await open({
        title: "Import audio into Inbox",
        multiple: true,
        filters: [audioImportFilter],
      });
      const sourcePaths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      if (sourcePaths.length === 0) {
        return;
      }

      await desktop.jobSubmitImport({ campaignId, sourcePaths });
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setJobsError(errorMessage(nextError));
      setJobsOpen(true);
    } finally {
      setImportSubmitting(false);
    }
  }

  async function startInboxWatch() {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId) return;
    setInboxWatchSubmitting(true);
    setInboxWatchError(null);
    try {
      setInboxWatchStatus(await desktop.inboxWatchStart({
        campaignId,
        intervalSecs: inboxWatchInterval,
      }));
    } catch (nextError) {
      const message = errorMessage(nextError);
      setInboxWatchError(message);
      pushNotification({ key: `watch:start:${message}`, tone: "error", title: "Inbox watch did not start", message });
    } finally {
      setInboxWatchSubmitting(false);
    }
  }

  async function stopInboxWatch() {
    setInboxWatchSubmitting(true);
    setInboxWatchError(null);
    try {
      setInboxWatchStatus(await desktop.inboxWatchStop());
    } catch (nextError) {
      const message = errorMessage(nextError);
      setInboxWatchError(message);
      pushNotification({ key: `watch:stop:${message}`, tone: "error", title: "Inbox watch did not stop", message });
    } finally {
      setInboxWatchSubmitting(false);
    }
  }

  async function startExport(request: ExportDialogRequest) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId) {
      return;
    }

    setExportSubmitting(true);
    setExportError(null);
    try {
      await desktop.jobSubmitExport({ campaignId, ...request });
      setExportDialogOpen(false);
      setExportInitialStems(null);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setExportError(errorMessage(nextError));
    } finally {
      setExportSubmitting(false);
    }
  }

  async function startProcessing(request: ProcessDialogRequest) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId) {
      return;
    }

    setProcessSubmitting(true);
    setProcessError(null);
    try {
      if (request.mode === "transcribe") {
        await desktop.jobSubmitTranscribe({
          campaignId,
          sourcePaths: request.sourcePaths,
          allInbox: request.allInbox,
          force: request.force,
          asrModel: request.asrModel,
          language: request.language,
          sessionDate: request.sessionDate,
          diarize: request.diarize,
          vad: request.vad,
          combine: request.combine,
          sessionName: request.sessionName,
        });
      } else {
        const { mode: _mode, ...processRequest } = request;
        await desktop.jobSubmitProcess({ campaignId, ...processRequest });
      }
      setProcessDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setProcessError(errorMessage(nextError));
    } finally {
      setProcessSubmitting(false);
    }
  }

  async function startLogRebuild() {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId) {
      return;
    }

    setLogRebuildSubmitting(true);
    setJobsError(null);
    try {
      await desktop.jobSubmitLogRebuild(campaignId);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setJobsError(errorMessage(nextError));
      setJobsOpen(true);
    } finally {
      setLogRebuildSubmitting(false);
    }
  }

  async function startNotes(request: Omit<NotesRequest, "campaignId" | "stem">) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId || !notesStem) {
      return;
    }

    setNotesSubmitting(true);
    setNotesError(null);
    try {
      await desktop.jobSubmitNotes({ campaignId, stem: notesStem, ...request });
      setNotesDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setNotesError(errorMessage(nextError));
    } finally {
      setNotesSubmitting(false);
    }
  }

  async function startSpeakerMapping(mappings: SpeakerMapping[], defaultMappings: SpeakerMapping[]) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId || !speakerStem) {
      return;
    }

    setSpeakerSubmitting(true);
    setSpeakerError(null);
    speakerJobRef.current = { campaignId, stem: speakerStem, reset: false };
    try {
      if (defaultMappings.length > 0) {
        const settings = await desktop.campaignSettings(campaignId);
        const selected = new Map(defaultMappings.map((mapping) => [mapping.label, mapping.name]));
        const speakers = settings.transcription.speakers
          .filter((mapping) => !selected.has(mapping.label))
          .concat(defaultMappings);
        await desktop.campaignSettingsWrite({
          campaignId,
          players: settings.players,
          vocabulary: settings.transcription.vocabulary,
          replacements: settings.transcription.replacements,
          speakers,
          expectedRevision: settings.revision,
        });
      }
      await desktop.jobSubmitSpeakerMap({ campaignId, stem: speakerStem, mappings });
      setSpeakerDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      speakerJobRef.current = null;
      setSpeakerError(errorMessage(nextError));
    } finally {
      setSpeakerSubmitting(false);
    }
  }

  async function resetSpeakerMapping() {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId || !speakerStem) return;
    setSpeakerSubmitting(true);
    setSpeakerError(null);
    speakerJobRef.current = { campaignId, stem: speakerStem, reset: true };
    try {
      await desktop.jobSubmitSpeakerReset({ campaignId, stem: speakerStem });
      setSpeakerDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      speakerJobRef.current = null;
      setSpeakerError(errorMessage(nextError));
    } finally {
      setSpeakerSubmitting(false);
    }
  }

  async function startSessionRename(newStem: string) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId || !activeSessionStem) return;
    const context = { campaignId, oldStem: activeSessionStem, newStem };
    setRenameSubmitting(true);
    setRenameError(null);
    sessionRenameJobRef.current = context;
    try {
      await desktop.jobSubmitSessionRename(context);
      setRenameDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      sessionRenameJobRef.current = null;
      setRenameError(errorMessage(nextError));
    } finally {
      setRenameSubmitting(false);
    }
  }

  async function startCandidateResolution(artifactId: ArtifactId, action: CandidateAction) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId || !activeSessionStem) {
      return;
    }

    setCandidateResolveSubmitting(true);
    setCandidateResolveError(null);
    const resolution: CandidateResolutionContext = {
      campaignId,
      stem: activeSessionStem,
      artifactId,
      action,
    };
    candidateResolutionRef.current = resolution;
    try {
      await desktop.jobSubmitCandidateResolve({
        campaignId,
        stem: activeSessionStem,
        artifactId,
        action,
      });
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      if (candidateResolutionRef.current === resolution) {
        candidateResolutionRef.current = null;
      }
      setCandidateResolveError(errorMessage(nextError));
    } finally {
      setCandidateResolveSubmitting(false);
    }
  }

  async function openSearchResult(result: SearchResult) {
    setSearchOpen(false);
    setActiveSessionStem(null);
    setActiveArtifactId(null);
    setActiveArtifactCandidate(false);
    setActiveAlternateName(null);
    setActiveTranscriptLine(null);
    if (result.campaignId !== activeCampaignId) {
      if (inboxWatchStatus.running && inboxWatchStatus.campaignId !== result.campaignId) {
        await stopInboxWatch();
      }
      setActiveCampaignId(result.campaignId);
      setActiveView("sessions");
      await loadLibrary(result.campaignId);
    }
    setActiveArtifactId(result.artifactId);
    setActiveArtifactCandidate(result.candidate);
    setActiveAlternateName(result.alternateName);
    setActiveTranscriptLine(result.transcript ? result.transcriptLine : null);
    setActiveSessionStem(result.stem);
  }

  async function startReindex() {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId) {
      return;
    }

    setReindexSubmitting(true);
    setJobsError(null);
    try {
      await desktop.jobSubmitReindex(campaignId);
      setSearchOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setJobsError(errorMessage(nextError));
      setJobsOpen(true);
    } finally {
      setReindexSubmitting(false);
    }
  }

  async function chooseCampaign(campaign: CampaignSummary) {
    setCampaignPickerOpen(false);
    if (inboxWatchStatus.running && inboxWatchStatus.campaignId !== campaign.id) {
      await stopInboxWatch();
    }
    setActiveCampaignId(campaign.id);
    setActiveView("sessions");
    setActiveSessionStem(null);
    setActiveArtifactId(null);
    setActiveArtifactCandidate(false);
    setActiveAlternateName(null);
    setActiveTranscriptLine(null);
    await loadLibrary(campaign.id);
  }

  const healthStatus = getHealthStatus(healthReport, healthLoading, healthError);
  const activeJobCount = jobs.filter((job) => !isTerminalJob(job)).length;
  const doctorRunning = doctorSubmitting || jobs.some(
    (job) => job.kind === "doctor" && !isTerminalJob(job),
  );
  const recordingRunning = jobs.some(
    (job) => job.kind === "record" && !isTerminalJob(job),
  );
  const importRunning = importSubmitting || jobs.some(
    (job) => job.kind === "import" && !isTerminalJob(job),
  );
  const exportRunning = exportSubmitting || jobs.some(
    (job) => job.kind === "export" && !isTerminalJob(job),
  );
  const processingRunning = processSubmitting || jobs.some(
    (job) => (job.kind === "run" || job.kind === "transcribe" || job.kind === "notes") && !isTerminalJob(job),
  );
  const logRebuilding = logRebuildSubmitting || jobs.some(
    (job) => job.kind === "rebuildLog" && !isTerminalJob(job),
  );
  const modelRunning = jobs.some(
    (job) => job.kind === "model" && !isTerminalJob(job),
  );
  const speakerMapping = speakerSubmitting || jobs.some(
    (job) => job.kind === "speakerMap" && !isTerminalJob(job),
  );
  const candidateResolving = candidateResolveSubmitting || jobs.some(
    (job) => job.kind === "candidateResolve" && !isTerminalJob(job),
  );
  const sessionRenaming = renameSubmitting || jobs.some(
    (job) => job.kind === "sessionRename" && !isTerminalJob(job),
  );
  const reindexing = reindexSubmitting || jobs.some(
    (job) => job.kind === "reindex" && !isTerminalJob(job),
  );

  if (onboardingLoading) {
    return <main className="setup-gate" aria-live="polite"><LoaderCircle className="is-spinning" size={22} aria-hidden="true" /><span>Loading operational setup</span></main>;
  }

  if (onboardingLoadError && !onboarding) {
    return <main className="setup-gate setup-gate--error" role="alert"><CircleAlert size={22} aria-hidden="true" /><span>{onboardingLoadError}</span><button className="button button--quiet" type="button" onClick={() => void loadOnboarding()}>Retry</button></main>;
  }

  if (onboardingOpen && onboarding) {
    return (
      <OnboardingScreen
        state={onboarding}
        health={healthReport}
        healthLoading={healthLoading}
        healthError={healthError}
        initialCampaignId={activeCampaignId}
        initialStep={onboardingInitialStep}
        onRefreshHealth={() => void refreshHealth()}
        onRunChecks={() => void startSystemCheck()}
        onHandoff={handoffOnboarding}
        onCampaignCreated={async (result) => {
          await refresh(result.campaignId);
          pushNotification({ key: `campaign:${result.campaignId}`, tone: "success", title: "Campaign created", message: result.name });
        }}
        onComplete={completeOnboarding}
        onClose={onboarding.required ? undefined : () => setOnboardingOpen(false)}
      />
    );
  }

  return (
    <div className={sidebarCollapsed ? "app-shell sidebar-collapsed" : "app-shell"}>
      <header className="topbar">
        <div className="brand-cluster">
          <div className="brand-mark" aria-hidden="true">
            <img src={brandIcon} alt="" />
          </div>
          <span className="brand-name">SessionSmith</span>
        </div>

        <div className="campaign-switcher">
          <button
            className="campaign-picker"
            type="button"
            onClick={() => setCampaignPickerOpen((open) => !open)}
            aria-expanded={campaignPickerOpen}
            aria-haspopup="listbox"
          >
            <span className="campaign-picker__eyebrow">Campaign</span>
            <span className="campaign-picker__name">
              {library?.campaign.name ?? "Choose a campaign"}
            </span>
            <ChevronDown size={16} aria-hidden="true" />
          </button>
          {campaignPickerOpen && (
            <div className="campaign-menu" role="listbox" aria-label="Campaigns">
              {bootstrap?.campaigns.map((campaign) => (
                <button
                  className={
                    campaign.id === activeCampaignId
                      ? "campaign-menu__item campaign-menu__item--active"
                      : "campaign-menu__item"
                  }
                  key={campaign.id}
                  type="button"
                  role="option"
                  aria-selected={campaign.id === activeCampaignId}
                  disabled={Boolean(campaign.loadError)}
                  onClick={() => void chooseCampaign(campaign)}
                >
                  <span>
                    <strong>{campaign.name}</strong>
                    <small>
                      {campaign.sessionCount} {pluralize(campaign.sessionCount, "session")}
                    </small>
                  </span>
                  {campaign.loadError ? (
                    <CircleAlert size={16} aria-label="Campaign could not be loaded" />
                  ) : (
                    <span className="campaign-menu__preset">{campaign.presetId || "Custom"}</span>
                  )}
                </button>
              ))}
              <div className="campaign-menu__divider" />
              <button className="campaign-menu__new" type="button" onClick={() => { setCampaignPickerOpen(false); openOnboarding("campaign"); }}>
                <Plus size={16} aria-hidden="true" />
                New campaign
              </button>
            </div>
          )}
        </div>

        <div className="topbar-actions">
          <span className="workspace-status" title={bootstrap?.workspacePath}>
            <Command size={14} aria-hidden="true" />
            Local workspace
          </span>
          <label className="topbar-search">
            <Search size={17} aria-hidden="true" />
            <span className="sr-only">Search notes and transcripts</span>
            <input
              ref={topbarSearchRef}
              type="search"
              value={searchQuery}
              placeholder="Search notes and transcripts"
              onFocus={() => setSearchOpen(true)}
              onChange={(event) => {
                setSearchQuery(event.target.value);
                setSearchOpen(true);
              }}
            />
            <kbd>Ctrl K</kbd>
          </label>
          <button
            className="icon-button"
            type="button"
            onClick={() => void refresh()}
            disabled={loading || libraryLoading}
            title="Refresh campaign library"
            aria-label="Refresh campaign library"
          >
            <RefreshCw className={loading || libraryLoading ? "is-spinning" : ""} size={17} />
          </button>
        </div>
      </header>

      <aside className="sidebar" aria-label="Campaign navigation">
        <div className="sidebar__top">
          <button
            className="collapse-button"
            type="button"
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
            title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
            aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
          >
            <PanelLeftClose size={17} aria-hidden="true" />
          </button>
          <span className="sidebar__campaign">{library?.campaign.name ?? "Workspace"}</span>
        </div>

        <nav className="nav-list">
          {navItems.map((item) => {
            const Icon = item.icon;
            const isActive = item.id === activeView;
            return (
              <button
                className={isActive ? "nav-item nav-item--active" : "nav-item"}
                key={item.id}
                type="button"
                onClick={() => {
                  setActiveView(item.id);
                  setActiveSessionStem(null);
                  setActiveArtifactId(null);
                  setActiveArtifactCandidate(false);
                  setActiveAlternateName(null);
                }}
                aria-current={isActive ? "page" : undefined}
                title={item.label}
              >
                <Icon size={18} aria-hidden="true" />
                <span>{item.label}</span>
                {item.id === "health" && (
                  <i
                    className={`nav-item__health nav-item__health--${healthStatus}`}
                    aria-label={`Health status ${healthStatusLabel(healthStatus)}`}
                  />
                )}
              </button>
            );
          })}
        </nav>

        <div className="sidebar__bottom">
          <button
            className={jobsOpen ? "jobs-stub jobs-stub--open" : "jobs-stub"}
            type="button"
            onClick={() => setJobsOpen((open) => !open)}
            aria-expanded={jobsOpen}
            aria-label={`Jobs: ${activeJobCount} active`}
            title="Open jobs"
          >
            {activeJobCount > 0 ? (
              <LoaderCircle className="is-spinning" size={17} aria-hidden="true" />
            ) : (
              <CircleDashed size={17} aria-hidden="true" />
            )}
            <span>Jobs</span>
            <span className="jobs-stub__count">{activeJobCount}</span>
          </button>
        </div>
      </aside>

      <main className="content">
        {onboarding?.required && (
          <button className="setup-resume" type="button" onClick={() => openOnboarding()}>
            <HeartPulse size={16} aria-hidden="true" />
            Resume setup
          </button>
        )}
        {activeSessionStem && library ? (
          <SessionWorkspacePage
            campaignId={library.campaign.id}
            stem={activeSessionStem}
            initialArtifactId={activeArtifactId}
            initialViewingCandidate={activeArtifactCandidate}
            initialAlternateName={activeAlternateName}
            initialTranscriptLine={activeTranscriptLine}
            onBack={() => {
              setActiveSessionStem(null);
              setActiveArtifactId(null);
              setActiveArtifactCandidate(false);
              setActiveAlternateName(null);
              setActiveTranscriptLine(null);
            }}
            onGenerateNotes={() => {
              setNotesStem(activeSessionStem);
              setNotesError(null);
              setNotesDialogOpen(true);
            }}
            generating={processingRunning}
            onExport={() => {
              setExportError(null);
              setExportInitialStems([activeSessionStem]);
              setExportDialogOpen(true);
            }}
            exporting={exportRunning}
            onReviewSpeakers={() => {
              setSpeakerStem(activeSessionStem);
              setSpeakerError(null);
              setSpeakerDialogOpen(true);
            }}
            onRename={() => {
              setRenameError(null);
              setRenameDialogOpen(true);
            }}
            renaming={sessionRenaming}
            speakerMapping={speakerMapping}
            candidateResolving={candidateResolving}
            candidateResolveError={candidateResolveError}
            onResolveCandidate={(artifactId, action) => void startCandidateResolution(artifactId, action)}
            campaignLogRebuildRecommended={
              campaignLogHandoff?.campaignId === library.campaign.id
              && campaignLogHandoff.stem === activeSessionStem
            }
            campaignLogRebuilding={logRebuilding}
            onRebuildCampaignLog={() => void startLogRebuild()}
            onDismissCampaignLogRebuild={() => setCampaignLogHandoff(null)}
            speakerNotesRegenerationRecommended={
              speakerNotesHandoff?.campaignId === library.campaign.id
              && speakerNotesHandoff.stem === activeSessionStem
            }
            onRegenerateSpeakerNotes={() => {
              setSpeakerNotesHandoff(null);
              setNotesStem(activeSessionStem);
              setNotesError(null);
              setNotesDialogOpen(true);
            }}
            onDismissSpeakerNotesRegeneration={() => setSpeakerNotesHandoff(null)}
            sessionNameRecommended={
              sessionNameHandoff?.campaignId === library.campaign.id
              && sessionNameHandoff.stem === activeSessionStem
            }
            onNameSession={() => {
              setSessionNameHandoff(null);
              setRenameError(null);
              setRenameDialogOpen(true);
            }}
            onDismissSessionName={() => setSessionNameHandoff(null)}
            refreshKey={workspaceReloadKey}
          />
        ) : activeView === "sessions" ? (
          <SessionLibrary
            bootstrap={bootstrap}
            library={library}
            error={error}
            loading={loading || libraryLoading}
            onOpenSession={(stem) => {
              setActiveArtifactId(null);
              setActiveArtifactCandidate(false);
              setActiveAlternateName(null);
              setActiveSessionStem(stem);
            }}
            onReviewSpeakers={(stem) => {
              setSpeakerStem(stem);
              setSpeakerError(null);
              setSpeakerDialogOpen(true);
            }}
            onRecord={() => {
              setRecordError(null);
              setRecordDialogOpen(true);
            }}
            recording={recordingRunning}
            onImport={() => void startAudioImport()}
            importing={importRunning}
            onExport={() => {
              setExportError(null);
              setExportInitialStems(null);
              setExportDialogOpen(true);
            }}
            exporting={exportRunning}
            onProcess={() => {
              setProcessError(null);
              setProcessDialogOpen(true);
            }}
            processing={processingRunning}
            onCreateCampaign={() => openOnboarding("campaign")}
            inboxWatchStatus={inboxWatchStatus}
            inboxWatchInterval={inboxWatchInterval}
            inboxWatchSubmitting={inboxWatchSubmitting}
            inboxWatchError={inboxWatchError}
            onInboxWatchIntervalChange={setInboxWatchInterval}
            onInboxWatchStart={() => void startInboxWatch()}
            onInboxWatchStop={() => void stopInboxWatch()}
          />
        ) : activeView === "log" ? (
          <CampaignLogPage
            campaign={library?.campaign}
            rebuilding={logRebuilding}
            exporting={exportRunning}
            canExport={Boolean(library?.sessions.some((session) => session.artifacts.length > 0))}
            reloadKey={logReloadKey}
            onRebuild={() => void startLogRebuild()}
            onExport={() => {
              setExportError(null);
              setExportInitialStems(null);
              setExportDialogOpen(true);
            }}
          />
        ) : activeView === "settings" ? (
          <CampaignSettingsPage
            campaign={library?.campaign}
            onCampaignRenamed={(campaignId) => void refresh(campaignId)}
          />
        ) : activeView === "app-settings" ? (
          <AppSettingsPage onOpenSetup={() => openOnboarding()} />
        ) : activeView === "models" ? (
          <ModelInventoryPage
            refreshKey={modelReloadKey}
            modelRunning={modelRunning}
            jobError={modelActionError}
            onActionStarting={() => setModelActionError(null)}
            onJobStarted={async () => {
              setJobsOpen(true);
              await refreshJobs();
            }}
          />
        ) : activeView === "health" ? (
          <HealthPage
            report={healthReport}
            loading={healthLoading}
            error={healthError}
            onRefresh={() => void refreshHealth()}
            onRunChecks={() => void startSystemCheck()}
            onRemedy={(remedy) => {
              if (remedy === "open-models" || remedy === "open-backend-settings") {
                setActiveView(remedy === "open-models" ? "models" : "settings");
              } else {
                void openUrl("https://github.com/mythos/SessionSmith/blob/main/docs/setup.md");
              }
              setActiveSessionStem(null);
              setActiveArtifactId(null);
              setActiveArtifactCandidate(false);
              setActiveAlternateName(null);
            }}
            checking={doctorRunning}
          />
        ) : null}
      </main>
      <JobsFlyout
        jobs={jobs}
        open={jobsOpen}
        error={jobsError}
        onClose={() => setJobsOpen(false)}
        onCancel={(jobId) => void cancelJob(jobId)}
      />
      <SearchDialog
        open={searchOpen}
        campaignId={activeCampaignId}
        campaignName={library?.campaign.name ?? null}
        canReindex={Boolean(activeCampaignId)}
        reindexing={reindexing}
        onClose={() => setSearchOpen(false)}
        onReindex={() => void startReindex()}
        onOpenResult={(result) => void openSearchResult(result)}
        query={searchQuery}
        onQueryChange={setSearchQuery}
        allCampaigns={searchPreferences.allCampaigns}
        onAllCampaignsChange={(allCampaigns) => setSearchPreferences((current) => ({ ...current, allCampaigns }))}
        sourceKinds={searchPreferences.sourceKinds}
        onSourceKindsChange={(sourceKinds) => setSearchPreferences((current) => ({ ...current, sourceKinds }))}
        sourceOptions={searchSources}
      />
      <RecordDialog
        open={recordDialogOpen}
        submitting={recordSubmitting}
        error={recordError}
        onClose={() => setRecordDialogOpen(false)}
        onSubmit={(name) => void startRecording(name)}
      />
      <ProcessDialog
        open={processDialogOpen}
        audio={library?.inbox ?? []}
        submitting={processSubmitting}
        error={processError}
        onClose={() => setProcessDialogOpen(false)}
        onSubmit={(request) => void startProcessing(request)}
      />
      <ExportDialog
        open={exportDialogOpen}
        campaignName={library?.campaign.name ?? null}
        sessions={library?.sessions ?? []}
        initialStems={exportInitialStems}
        submitting={exportSubmitting}
        error={exportError}
        onClose={() => {
          setExportDialogOpen(false);
          setExportInitialStems(null);
        }}
        onSubmit={(request) => void startExport(request)}
      />
      <NotesDialog
        open={notesDialogOpen}
        stem={notesStem}
        submitting={notesSubmitting}
        error={notesError}
        onClose={() => setNotesDialogOpen(false)}
        onSubmit={(request) => void startNotes(request)}
      />
      <SpeakerReviewDialog
        open={speakerDialogOpen}
        campaignId={activeCampaignId}
        stem={speakerStem}
        submitting={speakerSubmitting}
        error={speakerError}
        onClose={() => setSpeakerDialogOpen(false)}
        onSubmit={(mappings, defaultMappings) => void startSpeakerMapping(mappings, defaultMappings)}
        onReset={() => void resetSpeakerMapping()}
      />
      <RenameSessionDialog
        open={renameDialogOpen}
        campaignId={activeCampaignId}
        stem={activeSessionStem}
        submitting={sessionRenaming}
        error={renameError}
        onClose={() => setRenameDialogOpen(false)}
        onClearError={() => setRenameError(null)}
        onSubmit={(newStem) => void startSessionRename(newStem)}
      />
      <NotificationViewport notifications={notifications} onDismiss={dismissNotification} />
    </div>
  );
}

function SessionLibrary({
  bootstrap,
  library,
  error,
  loading,
  onOpenSession,
  onReviewSpeakers,
  onRecord,
  recording,
  onImport,
  importing,
  onExport,
  exporting,
  onProcess,
  processing,
  onCreateCampaign,
  inboxWatchStatus,
  inboxWatchInterval,
  inboxWatchSubmitting,
  inboxWatchError,
  onInboxWatchIntervalChange,
  onInboxWatchStart,
  onInboxWatchStop,
}: {
  bootstrap: AppBootstrap | null;
  library: CampaignLibrary | null;
  error: string | null;
  loading: boolean;
  onOpenSession: (stem: string) => void;
  onReviewSpeakers: (stem: string) => void;
  onRecord: () => void;
  recording: boolean;
  onImport: () => void;
  importing: boolean;
  onExport: () => void;
  exporting: boolean;
  onProcess: () => void;
  processing: boolean;
  onCreateCampaign: () => void;
  inboxWatchStatus: InboxWatchStatus;
  inboxWatchInterval: number;
  inboxWatchSubmitting: boolean;
  inboxWatchError: string | null;
  onInboxWatchIntervalChange: (seconds: number) => void;
  onInboxWatchStart: () => void;
  onInboxWatchStop: () => void;
}) {
  const { settings: appSettings } = useAppSettings();

  if (loading) {
    return <LibraryLoading />;
  }

  if (error) {
    return (
      <section className="state-panel state-panel--error" aria-live="polite">
        <CircleAlert size={22} aria-hidden="true" />
        <div>
          <p className="eyebrow">Desktop backend unavailable</p>
          <h1>The campaign library could not be read.</h1>
          <p>{error}</p>
        </div>
      </section>
    );
  }

  if (!bootstrap || bootstrap.campaigns.length === 0) {
    return (
      <section className="state-panel">
        <LibraryBig size={24} aria-hidden="true" />
        <div>
          <p className="eyebrow">No campaign files found</p>
          <h1>This workspace has no campaigns yet.</h1>
          <p>Create a campaign to establish validated configuration and output paths.</p>
          <button className="button button--primary" type="button" onClick={onCreateCampaign}><Plus size={16} aria-hidden="true" />Create campaign</button>
        </div>
      </section>
    );
  }

  if (!library) {
    return null;
  }

  return (
    <div className="library-page">
      <header className="page-header">
        <div>
          <p className="eyebrow">{library.campaign.presetId || "Custom system"}</p>
          <h1>Sessions</h1>
          <p className="page-subtitle">
            {library.campaign.gm ? `${library.campaign.gm}'s table` : "Campaign library"}
            {library.campaign.setting ? ` · ${library.campaign.setting}` : ""}
          </p>
        </div>
        <div className="page-actions">
          <button
            className="button button--quiet"
            type="button"
            onClick={onRecord}
            disabled={recording}
            title={recording ? "A recording is already active." : "Record from the default audio input"}
          >
            {recording ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Mic size={16} aria-hidden="true" />}
            {recording ? "Recording" : "Record"}
          </button>
          <button
            className="button button--quiet"
            type="button"
            onClick={onImport}
            disabled={importing}
            title={importing ? "An audio import is already active." : "Import audio into the Inbox"}
          >
            {importing ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <FolderOpen size={16} aria-hidden="true" />}
            {importing ? "Importing" : "Import audio"}
          </button>
          <button
            className="button button--quiet"
            type="button"
            onClick={onExport}
            disabled={exporting || !library.sessions.some((session) => session.artifacts.length > 0)}
            title={exporting ? "An export is already active." : "Export generated session notes"}
          >
            {exporting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <FileOutput size={16} aria-hidden="true" />}
            {exporting ? "Exporting" : "Export"}
          </button>
          <button
            className="button button--primary"
            type="button"
            onClick={onProcess}
            disabled={processing || library.inbox.length === 0}
            title={processing ? "A pipeline job is already active." : library.inbox.length === 0 ? "Add audio to the Inbox before processing." : "Process Inbox audio"}
          >
            {processing ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Sparkles size={16} aria-hidden="true" />}
            {processing ? "Processing" : "Process"}
          </button>
        </div>
      </header>

      <section className="inbox-watch" aria-label="Inbox watch">
        <div className={inboxWatchStatus.running ? "inbox-watch__signal inbox-watch__signal--active" : "inbox-watch__signal"}>
          <Radio size={17} aria-hidden="true" />
        </div>
        <div className="inbox-watch__body">
          <strong>Inbox watch</strong>
          <span>{formatInboxWatchStatus(inboxWatchStatus, library.campaign.name)}</span>
          {(inboxWatchError || inboxWatchStatus.lastError) && (
            <small role="status">{inboxWatchError ?? inboxWatchStatus.lastError}</small>
          )}
        </div>
        <label className="inbox-watch__interval">
          <span>Interval</span>
          <select
            value={inboxWatchInterval}
            onChange={(event) => onInboxWatchIntervalChange(Number(event.target.value))}
            disabled={inboxWatchStatus.running || inboxWatchSubmitting}
          >
            <option value={2}>2 seconds</option>
            <option value={5}>5 seconds</option>
            <option value={10}>10 seconds</option>
            <option value={30}>30 seconds</option>
          </select>
        </label>
        <button
          className={inboxWatchStatus.running ? "button button--quiet" : "button button--primary"}
          type="button"
          disabled={inboxWatchSubmitting}
          onClick={inboxWatchStatus.running ? onInboxWatchStop : onInboxWatchStart}
        >
          {inboxWatchSubmitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Radio size={16} aria-hidden="true" />}
          {inboxWatchStatus.running ? "Stop watch" : "Start watch"}
        </button>
      </section>

      {library.inbox.length > 0 && (
        <section className="library-section">
          <SectionHeader label="Inbox" count={library.inbox.length} detail="Audio waiting to become a session" />
          <div className="inbox-grid">
            {library.inbox.map((audio) => (
              <article className="inbox-row" key={audio.path}>
                <div className="inbox-row__icon">
                  <AudioLines size={18} aria-hidden="true" />
                </div>
                <div className="inbox-row__body">
                  <strong>{audio.name}</strong>
                  <span>{formatBytes(audio.sizeBytes)} · {formatTimestamp(audio.modifiedAt, appSettings.dateFormat)}</span>
                </div>
                <span className="inbox-row__status">Ready</span>
              </article>
            ))}
          </div>
        </section>
      )}

      <section className="library-section library-section--sessions">
        <SectionHeader label="Sessions" count={library.sessions.length} detail="Audio, transcript, and notes in one place" />
        {library.sessions.length > 0 ? (
          <div className="session-list">
            {library.sessions.map((session) => (
              <SessionRow key={session.stem} session={session} onOpen={() => onOpenSession(session.stem)} onReviewSpeakers={() => onReviewSpeakers(session.stem)} />
            ))}
          </div>
        ) : (
          <div className="empty-list">
            <AudioLines size={22} aria-hidden="true" />
            <div>
              <strong>No completed sessions yet</strong>
              <span>When SessionSmith finds a transcript or notes folder, it will appear here.</span>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}

function SectionHeader({ label, count, detail }: { label: string; count: number; detail: string }) {
  return (
    <div className="section-header">
      <div>
        <h2>{label} <span>{count}</span></h2>
        <p>{detail}</p>
      </div>
    </div>
  );
}

function formatInboxWatchStatus(status: InboxWatchStatus, selectedCampaignName: string) {
  if (!status.running) return `Stopped · ${selectedCampaignName}`;
  const campaign = status.campaignName ?? selectedCampaignName;
  if (status.processingPath) {
    const name = status.processingPath.split(/[\\/]/).pop() ?? status.processingPath;
    return `${campaign} · Processing ${name} · ${status.queued} queued`;
  }
  return `${campaign} · Watching · ${status.queued} queued`;
}

function SessionRow({ session, onOpen, onReviewSpeakers }: { session: SessionSummary; onOpen: () => void; onReviewSpeakers: () => void }) {
  const { settings: appSettings } = useAppSettings();
  const action = session.artifacts.length > 0 || session.hasTranscript ? "Open session" : "View session";

  return (
    <article className="session-row">
      <div className="session-row__identity">
        <span className={`session-row__stage session-row__stage--${session.stage}`} aria-hidden="true">
          {session.stage === "notes" ? <CircleCheck size={17} /> : <Clock3 size={17} />}
        </span>
        <div>
          <h3>{session.stem}</h3>
          <p>{formatTimestamp(session.modifiedAt, appSettings.dateFormat)}</p>
        </div>
      </div>
      <div className="session-row__pipeline" aria-label={`Pipeline status: ${session.stage}`}>
        <PipelineChip label="Audio" complete={session.hasAudio} />
        <PipelineChip label="Transcript" complete={session.hasTranscript} />
        <PipelineChip label={`Notes${session.artifacts.length ? ` (${session.artifacts.length})` : ""}`} complete={session.artifacts.length > 0} />
      </div>
      <div className="session-row__details">
        {session.artifacts.length > 0 ? (
          <span>{session.artifacts.map(formatArtifact).join(" · ")}</span>
        ) : (
          <span>Awaiting {session.stage === "audio" ? "transcription" : "notes"}</span>
        )}
        {session.unmappedSpeakerCount > 0 && (
          <button className="session-row__speaker-hint" type="button" onClick={onReviewSpeakers}>
            <UsersRound size={13} aria-hidden="true" />
            {session.unmappedSpeakerCount} unnamed {pluralize(session.unmappedSpeakerCount, "speaker")} · Review speakers
          </button>
        )}
      </div>
      <button className="row-action" type="button" onClick={onOpen} title={`Open ${session.stem}`}>
        {action}
      </button>
    </article>
  );
}

function PipelineChip({ label, complete }: { label: string; complete: boolean }) {
  return (
    <span className={complete ? "pipeline-chip pipeline-chip--complete" : "pipeline-chip"}>
      {complete ? <CircleCheck size={13} aria-hidden="true" /> : <CircleDashed size={13} aria-hidden="true" />}
      {label}
    </span>
  );
}

function LibraryLoading() {
  return (
    <div className="library-page library-page--loading" aria-live="polite">
      <header className="page-header">
        <div>
          <p className="eyebrow">Reading workspace</p>
          <h1>Sessions</h1>
        </div>
        <LoaderCircle className="is-spinning" size={22} aria-label="Loading campaign library" />
      </header>
      <div className="skeleton-block skeleton-block--heading" />
      <div className="skeleton-list">
        <div className="skeleton-block" />
        <div className="skeleton-block" />
        <div className="skeleton-block" />
      </div>
    </div>
  );
}

function formatArtifact(artifact: string) {
  return artifact
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024 * 1024) {
    return `${Math.max(1, Math.round(bytes / (1024 * 1024)))} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function pluralize(count: number, singular: string) {
  return count === 1 ? singular : `${singular}s`;
}

function getHealthStatus(
  report: HealthReport | null,
  loading: boolean,
  error: string | null,
): HealthStatus {
  if (loading && !report) {
    return "pending";
  }
  if (!report || error) {
    return "warn";
  }
  if (report.checks.some((check) => check.state === "fail")) {
    return "fail";
  }
  if (report.checks.some((check) => check.state === "warn")) {
    return "warn";
  }
  return "ok";
}

function healthStatusLabel(status: HealthStatus) {
  if (status === "ok") {
    return "ready";
  }
  if (status === "fail") {
    return "needs attention";
  }
  if (status === "warn") {
    return "warning";
  }
  return "pending";
}

function isTerminalJob(job: DesktopJob) {
  return job.state === "succeeded" || job.state === "failed" || job.state === "cancelled";
}

function loadSearchPreferences(): { allCampaigns: boolean; sourceKinds: string[] } {
  try {
    const value = JSON.parse(window.localStorage.getItem(searchPreferencesKey) ?? "null") as unknown;
    if (!value || typeof value !== "object") return { allCampaigns: false, sourceKinds: [] };
    const record = value as Record<string, unknown>;
    return {
      allCampaigns: record.allCampaigns === true,
      sourceKinds: Array.isArray(record.sourceKinds)
        ? record.sourceKinds.filter((source): source is string => typeof source === "string")
        : [],
    };
  } catch {
    return { allCampaigns: false, sourceKinds: [] };
  }
}

function isDateDerivedSessionStem(stem: string) {
  return /^\d{4}-\d{2}-\d{2}(?:[-_T]\d{2}(?:[-_:]?\d{2}){0,2})?$/.test(stem);
}

function upsertJob(currentJobs: DesktopJob[], nextJob: DesktopJob) {
  const nextJobs = currentJobs.filter((job) => job.id !== nextJob.id);
  nextJobs.push(nextJob);
  nextJobs.sort((left, right) => left.id - right.id);
  return nextJobs;
}

export default App;
