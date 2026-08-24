import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
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
  Search,
  Settings2,
  Sparkles,
} from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import { CampaignSettingsPage } from "./CampaignSettings";
import { ExportDialog, type ExportDialogRequest } from "./ExportDialog";
import { NotesDialog } from "./NotesDialog";
import { ProcessDialog, type ProcessDialogRequest } from "./ProcessDialog";
import { RecordDialog } from "./RecordDialog";
import { SearchDialog } from "./SearchDialog";
import { SpeakerReviewDialog } from "./SpeakerReviewDialog";
import "./App.css";
import { HealthPage } from "./Health";
import { JobsFlyout } from "./Jobs";
import { ModelInventoryPage } from "./Models";
import { CampaignLogPage, SessionWorkspacePage } from "./Workspace";
import type {
  AppBootstrap,
  ArtifactId,
  CandidateAction,
  CampaignLibrary,
  CampaignSummary,
  DesktopJob,
  HealthReport,
  NotesRequest,
  SearchResult,
  SessionSummary,
  SpeakerMapping,
} from "./types";

type View = "sessions" | "log" | "settings" | "models" | "health";
type HealthStatus = "pending" | "ok" | "warn" | "fail";
type CandidateResolutionContext = {
  campaignId: string;
  stem: string;
  artifactId: ArtifactId;
  action: CandidateAction;
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
  const [workspaceReloadKey, setWorkspaceReloadKey] = useState(0);
  const [modelReloadKey, setModelReloadKey] = useState(0);
  const [importSubmitting, setImportSubmitting] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportSubmitting, setExportSubmitting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [exportInitialStems, setExportInitialStems] = useState<string[] | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [reindexSubmitting, setReindexSubmitting] = useState(false);
  const activeCampaignIdRef = useRef<string | null>(null);
  const candidateResolutionRef = useRef<CandidateResolutionContext | null>(null);

  useEffect(() => {
    activeCampaignIdRef.current = activeCampaignId;
  }, [activeCampaignId]);

  useEffect(() => {
    void refresh();
    void refreshHealth();
  }, []);

  useEffect(() => {
    function openSearch(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearchOpen(true);
      }
    }

    window.addEventListener("keydown", openSearch);
    return () => window.removeEventListener("keydown", openSearch);
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    void refreshJobs();
    void listen<DesktopJob>("job://updated", ({ payload }) => {
      if (!active) {
        return;
      }
      setJobs((currentJobs) => upsertJob(currentJobs, payload));
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
      }
      if (payload.kind === "speakerMap" && payload.state === "succeeded") {
        setWorkspaceReloadKey((current) => current + 1);
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
      if (payload.kind === "rebuildLog" && payload.state === "succeeded") {
        setLogReloadKey((current) => current + 1);
        setCampaignLogHandoff(null);
      }
      if (payload.kind === "model" && payload.state === "succeeded") {
        setModelReloadKey((current) => current + 1);
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

  async function startSpeakerMapping(mappings: SpeakerMapping[]) {
    const campaignId = activeCampaignIdRef.current;
    if (!campaignId || !speakerStem) {
      return;
    }

    setSpeakerSubmitting(true);
    setSpeakerError(null);
    try {
      await desktop.jobSubmitSpeakerMap({ campaignId, stem: speakerStem, mappings });
      setSpeakerDialogOpen(false);
      setJobsOpen(true);
      await refreshJobs();
    } catch (nextError) {
      setSpeakerError(errorMessage(nextError));
    } finally {
      setSpeakerSubmitting(false);
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
    if (result.campaignId !== activeCampaignId) {
      setActiveCampaignId(result.campaignId);
      setActiveView("sessions");
      await loadLibrary(result.campaignId);
    }
    setActiveArtifactId(result.artifactId);
    setActiveArtifactCandidate(result.candidate);
    setActiveAlternateName(result.alternateName);
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
    setActiveCampaignId(campaign.id);
    setActiveView("sessions");
    setActiveSessionStem(null);
    setActiveArtifactId(null);
    setActiveArtifactCandidate(false);
    setActiveAlternateName(null);
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
  const reindexing = reindexSubmitting || jobs.some(
    (job) => job.kind === "reindex" && !isTerminalJob(job),
  );

  return (
    <div className={sidebarCollapsed ? "app-shell sidebar-collapsed" : "app-shell"}>
      <header className="topbar">
        <div className="brand-cluster">
          <div className="brand-mark" aria-hidden="true">
            <AudioLines size={19} strokeWidth={2.2} />
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
              <button className="campaign-menu__new" type="button" disabled title="Campaign creation is next in the GUI rewrite.">
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
          <button
            className="icon-button"
            type="button"
            onClick={() => setSearchOpen(true)}
            title="Search indexed notes"
            aria-label="Search indexed notes"
          >
            <Search size={17} aria-hidden="true" />
          </button>
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
        {activeSessionStem && library ? (
          <SessionWorkspacePage
            campaignId={library.campaign.id}
            stem={activeSessionStem}
            initialArtifactId={activeArtifactId}
            initialViewingCandidate={activeArtifactCandidate}
            initialAlternateName={activeAlternateName}
            onBack={() => {
              setActiveSessionStem(null);
              setActiveArtifactId(null);
              setActiveArtifactCandidate(false);
              setActiveAlternateName(null);
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
          <CampaignSettingsPage campaign={library?.campaign} />
        ) : activeView === "models" ? (
          <ModelInventoryPage
            refreshKey={modelReloadKey}
            modelRunning={modelRunning}
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
            onOpenModels={() => {
              setActiveView("models");
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
        onSubmit={(mappings) => void startSpeakerMapping(mappings)}
      />
    </div>
  );
}

function SessionLibrary({
  bootstrap,
  library,
  error,
  loading,
  onOpenSession,
  onRecord,
  recording,
  onImport,
  importing,
  onExport,
  exporting,
  onProcess,
  processing,
}: {
  bootstrap: AppBootstrap | null;
  library: CampaignLibrary | null;
  error: string | null;
  loading: boolean;
  onOpenSession: (stem: string) => void;
  onRecord: () => void;
  recording: boolean;
  onImport: () => void;
  importing: boolean;
  onExport: () => void;
  exporting: boolean;
  onProcess: () => void;
  processing: boolean;
}) {
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
          <p>Create a campaign in the existing CLI/TUI, or set `SESSIONSMITH_WORKSPACE` to a workspace with a `campaigns/` directory.</p>
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
                  <span>{formatBytes(audio.sizeBytes)} · {formatDate(audio.modifiedAt)}</span>
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
              <SessionRow key={session.stem} session={session} onOpen={() => onOpenSession(session.stem)} />
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

function SessionRow({ session, onOpen }: { session: SessionSummary; onOpen: () => void }) {
  const action = session.artifacts.length > 0 || session.hasTranscript ? "Open session" : "View session";

  return (
    <article className="session-row">
      <div className="session-row__identity">
        <span className={`session-row__stage session-row__stage--${session.stage}`} aria-hidden="true">
          {session.stage === "notes" ? <CircleCheck size={17} /> : <Clock3 size={17} />}
        </span>
        <div>
          <h3>{session.stem}</h3>
          <p>{formatDate(session.modifiedAt)}</p>
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

function formatDate(timestamp: number | null) {
  if (!timestamp) {
    return "No modified date";
  }
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" }).format(
    new Date(timestamp * 1000),
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) {
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
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

function upsertJob(currentJobs: DesktopJob[], nextJob: DesktopJob) {
  const nextJobs = currentJobs.filter((job) => job.id !== nextJob.id);
  nextJobs.push(nextJob);
  nextJobs.sort((left, right) => left.id - right.id);
  return nextJobs;
}

export default App;
