import { lazy, Suspense, useDeferredValue, useEffect, useEffectEvent, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { diffWordsWithSpace } from "diff";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  ArrowLeft,
  AudioLines,
  BookOpenText,
  Check,
  CircleAlert,
  ClipboardCopy,
  Columns2,
  FileOutput,
  FileText,
  Info,
  LoaderCircle,
  LocateFixed,
  Pause,
  Pencil,
  Play,
  RefreshCw,
  Save,
  Search,
  Sparkles,
  Square,
  Trash2,
  UsersRound,
  X,
} from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import type {
  AudioPlayerSnapshot,
  ArtifactDocument,
  ArtifactId,
  CandidateAction,
  CampaignLogDocument,
  CampaignSummary,
  SavedArtifactSummary,
  SessionProvenance,
  SessionWorkspace as SessionWorkspaceData,
  TranscriptPage,
} from "./types";

const artifactOrder: ArtifactId[] = ["summary", "bullets", "dm-notes", "recap", "story", "quotes"];
const documentConflictMessage = "This document changed on disk. Reload it before saving.";
const artifactDraftStoragePrefix = "sessionsmith:artifact-draft:";
const unloadedAudioSnapshot: AudioPlayerSnapshot = {
  status: "unloaded",
  label: null,
  positionMs: 0,
  durationMs: null,
  error: null,
};
const MarkdownEditor = lazy(async () => {
  const module = await import("./MarkdownEditor");
  return { default: module.MarkdownEditor };
});

export function SessionWorkspacePage({
  campaignId,
  stem,
  initialArtifactId,
  initialViewingCandidate,
  initialAlternateName,
  onBack,
  onGenerateNotes,
  generating,
  onExport,
  exporting,
  onReviewSpeakers,
  speakerMapping,
  candidateResolving,
  candidateResolveError,
  onResolveCandidate,
  campaignLogRebuildRecommended,
  campaignLogRebuilding,
  onRebuildCampaignLog,
  onDismissCampaignLogRebuild,
  refreshKey,
}: {
  campaignId: string;
  stem: string;
  initialArtifactId: ArtifactId | null;
  initialViewingCandidate: boolean;
  initialAlternateName: string | null;
  onBack: () => void;
  onGenerateNotes: () => void;
  generating: boolean;
  onExport: () => void;
  exporting: boolean;
  onReviewSpeakers: () => void;
  speakerMapping: boolean;
  candidateResolving: boolean;
  candidateResolveError: string | null;
  onResolveCandidate: (artifactId: ArtifactId, action: CandidateAction) => void;
  campaignLogRebuildRecommended: boolean;
  campaignLogRebuilding: boolean;
  onRebuildCampaignLog: () => void;
  onDismissCampaignLogRebuild: () => void;
  refreshKey: number;
}) {
  const [workspace, setWorkspace] = useState<SessionWorkspaceData | null>(null);
  const [workspaceLoading, setWorkspaceLoading] = useState(true);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);
  const [selectedArtifact, setSelectedArtifact] = useState<ArtifactId>("summary");
  const [selectedSavedArtifact, setSelectedSavedArtifact] = useState<SavedArtifactSummary | null>(null);
  const [viewingCandidate, setViewingCandidate] = useState(false);
  const [comparingCandidate, setComparingCandidate] = useState(false);
  const [metaOpen, setMetaOpen] = useState(false);
  const [candidateActionPending, setCandidateActionPending] = useState<CandidateAction | null>(null);
  const [artifactDocument, setArtifactDocument] = useState<ArtifactDocument | null>(null);
  const [documentLoading, setDocumentLoading] = useState(false);
  const [documentError, setDocumentError] = useState<string | null>(null);
  const [transcriptOpen, setTranscriptOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [savedMarkdown, setSavedMarkdown] = useState("");
  const [draftRevision, setDraftRevision] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveWarning, setSaveWarning] = useState<string | null>(null);
  const [conflictDocument, setConflictDocument] = useState<ArtifactDocument | null>(null);
  const [comparingEditConflict, setComparingEditConflict] = useState(false);
  const [audioSnapshot, setAudioSnapshot] = useState<AudioPlayerSnapshot>(unloadedAudioSnapshot);
  const [audioBusy, setAudioBusy] = useState<"toggle" | "seek" | "stop" | null>(null);
  const [audioError, setAudioError] = useState<string | null>(null);
  const draftRef = useRef("");

  useEffect(() => {
    let cancelled = false;
    setWorkspaceLoading(true);
    setWorkspaceError(null);
    setWorkspace(null);
    setArtifactDocument(null);
    setDocumentError(null);
    setCopied(false);
    setComparingCandidate(false);
    setMetaOpen(false);
    setCandidateActionPending(null);
    setEditing(false);
    setDraft("");
    setSavedMarkdown("");
    setDraftRevision(null);
    setSaving(false);
    setSaveError(null);
    setSaveWarning(null);
    setConflictDocument(null);
    setComparingEditConflict(false);
    draftRef.current = "";

    void desktop
      .sessionWorkspace(campaignId, stem)
      .then((nextWorkspace) => {
        if (cancelled) {
          return;
        }
        const requestedSavedArtifact = initialArtifactId && initialAlternateName
          ? nextWorkspace.savedArtifacts.find((artifact) => (
            artifact.artifactId === initialArtifactId && artifact.filename === initialAlternateName
          )) ?? null
          : null;
        const requestedArtifact = initialArtifactId && nextWorkspace.artifacts.find((artifact) => (
          artifact.id === initialArtifactId && (artifact.available || artifact.candidateAvailable)
        ));
        const defaultArtifact = requestedSavedArtifact?.artifactId ?? requestedArtifact?.id ?? artifactOrder.find((artifactId) => {
          const artifact = nextWorkspace.artifacts.find((item) => item.id === artifactId);
          return artifact?.available || artifact?.candidateAvailable;
        }) ?? "summary";
        const defaultSummary = nextWorkspace.artifacts.find((item) => item.id === defaultArtifact);

        setWorkspace(nextWorkspace);
        setSelectedArtifact(defaultArtifact);
        setSelectedSavedArtifact(requestedSavedArtifact ?? null);
        setViewingCandidate(Boolean(
          (!requestedSavedArtifact && requestedArtifact?.candidateAvailable && initialViewingCandidate)
            || (defaultSummary?.candidateAvailable && !defaultSummary.available),
        ));
        setTranscriptOpen(Boolean(nextWorkspace.transcript));
      })
      .catch((nextError) => {
        if (!cancelled) {
          setWorkspaceError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setWorkspaceLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [campaignId, initialAlternateName, initialArtifactId, initialViewingCandidate, stem, refreshKey]);

  useEffect(() => {
    if (!workspace || editing) {
      return;
    }

    const artifact = workspace.artifacts.find((item) => item.id === selectedArtifact);
    const savedArtifact = selectedSavedArtifact
      ? workspace.savedArtifacts.find((item) => item.filename === selectedSavedArtifact.filename) ?? null
      : null;
    const canRead = savedArtifact
      ? true
      : viewingCandidate ? artifact?.candidateAvailable : artifact?.available;
    if (!canRead) {
      setArtifactDocument(null);
      setDocumentError(null);
      setDocumentLoading(false);
      return;
    }

    let cancelled = false;
    setDocumentLoading(true);
    setDocumentError(null);
    setCopied(false);
    void desktop
      .artifactRead(
        workspace.campaign.id,
        workspace.session.stem,
        selectedArtifact,
        viewingCandidate,
        savedArtifact?.filename,
      )
      .then((nextDocument) => {
        if (!cancelled) {
          setArtifactDocument(nextDocument);
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setArtifactDocument(null);
          setDocumentError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDocumentLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [editing, workspace, selectedArtifact, selectedSavedArtifact, viewingCandidate]);

  const saveDraft = useEffectEvent(async (revisionOverride?: string) => {
    if (!workspace || !artifactDocument || !editing || !draftRevision || saving) {
      return;
    }

    const markdown = draft;
    const expectedRevision = revisionOverride ?? draftRevision;
    setSaving(true);
    setSaveError(null);
    setSaveWarning(null);
    setConflictDocument(null);
    setComparingEditConflict(false);
    try {
      const result = await desktop.artifactWrite({
        campaignId: workspace.campaign.id,
        stem: workspace.session.stem,
        artifactId: selectedArtifact,
        markdown,
        expectedRevision,
      });
      setArtifactDocument((current) => (
        current && !current.candidate && current.id === selectedArtifact
          ? { ...current, markdown, modifiedAt: result.modifiedAt, revision: result.revision }
          : current
      ));
      setDraftRevision(result.revision);
      setSavedMarkdown(markdown);
      const latestDraft = draftRef.current;
      if (latestDraft === markdown) {
        clearArtifactDraft(workspace.campaign.id, workspace.session.stem, selectedArtifact);
        setSaveWarning(result.indexWarning);
      } else {
        const stored = storeArtifactDraft(
          workspace.campaign.id,
          workspace.session.stem,
          selectedArtifact,
          latestDraft,
          result.revision,
        );
        setSaveWarning(
          stored
            ? result.indexWarning
            : result.indexWarning ?? "This draft could not be stored locally. Save it before leaving the editor.",
        );
      }
    } catch (nextError) {
      const message = errorMessage(nextError);
      setSaveError(message);
      if (message === documentConflictMessage) {
        void desktop
          .artifactRead(workspace.campaign.id, workspace.session.stem, selectedArtifact, false)
          .then((document) => setConflictDocument(document))
          .catch(() => setConflictDocument(null));
      }
    } finally {
      setSaving(false);
    }
  });

  const draftIsDirty = editing && draft !== savedMarkdown;
  const navigationLocked = editing && (saving || draftIsDirty);

  useEffect(() => {
    if (!draftIsDirty || saving || saveError) {
      return;
    }
    const timer = window.setTimeout(() => {
      void saveDraft();
    }, 1_500);
    return () => window.clearTimeout(timer);
  }, [draft, draftIsDirty, saveError, saving]);

  useEffect(() => {
    if (!editing) {
      return;
    }
    const handleSave = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void saveDraft();
      }
    };
    window.addEventListener("keydown", handleSave, true);
    return () => window.removeEventListener("keydown", handleSave, true);
  }, [editing]);

  useEffect(() => {
    setAudioSnapshot(unloadedAudioSnapshot);
    setAudioBusy(null);
    setAudioError(null);
    return () => {
      void desktop.audioStop().catch(() => undefined);
    };
  }, [campaignId, stem]);

  useEffect(() => {
    if (audioSnapshot.status !== "playing") {
      return;
    }

    let cancelled = false;
    const refreshPlayback = async () => {
      try {
        const nextSnapshot = await desktop.audioState();
        if (!cancelled) {
          setAudioSnapshot(nextSnapshot);
          setAudioError(nextSnapshot.error);
        }
      } catch (nextError) {
        if (!cancelled) {
          setAudioError(errorMessage(nextError));
          setAudioSnapshot((current) => ({ ...current, status: "paused" }));
        }
      }
    };
    const timer = window.setInterval(() => void refreshPlayback(), 750);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [audioSnapshot.status]);

  async function toggleAudioPlayback() {
    if (audioBusy) {
      return;
    }
    setAudioBusy("toggle");
    setAudioError(null);
    try {
      if (audioSnapshot.status === "playing") {
        const nextSnapshot = await desktop.audioPause();
        setAudioSnapshot(nextSnapshot);
        setAudioError(nextSnapshot.error);
      } else {
        if (audioSnapshot.status === "unloaded") {
          const loadedSnapshot = await desktop.audioLoad(campaignId, stem);
          setAudioSnapshot(loadedSnapshot);
        }
        const nextSnapshot = await desktop.audioPlay();
        setAudioSnapshot(nextSnapshot);
        setAudioError(nextSnapshot.error);
      }
    } catch (nextError) {
      setAudioError(errorMessage(nextError));
    } finally {
      setAudioBusy(null);
    }
  }

  async function seekAudio(positionMs: number) {
    if (audioBusy) {
      return;
    }
    const target = Math.max(0, Math.min(Number.MAX_SAFE_INTEGER, Math.round(positionMs)));
    setAudioBusy("seek");
    setAudioError(null);
    try {
      if (audioSnapshot.status === "unloaded") {
        const loadedSnapshot = await desktop.audioLoad(campaignId, stem);
        setAudioSnapshot(loadedSnapshot);
      }
      const nextSnapshot = await desktop.audioSeek(target);
      setAudioSnapshot(nextSnapshot);
      setAudioError(nextSnapshot.error);
    } catch (nextError) {
      setAudioError(errorMessage(nextError));
    } finally {
      setAudioBusy(null);
    }
  }

  async function stopAudioPlayback() {
    if (audioBusy || audioSnapshot.status === "unloaded") {
      return;
    }
    setAudioBusy("stop");
    setAudioError(null);
    try {
      const nextSnapshot = await desktop.audioStop();
      setAudioSnapshot(nextSnapshot);
      setAudioError(nextSnapshot.error);
    } catch (nextError) {
      setAudioError(errorMessage(nextError));
    } finally {
      setAudioBusy(null);
    }
  }

  function selectArtifact(artifactId: ArtifactId) {
    if (navigationLocked) {
      return;
    }
    const artifact = workspace?.artifacts.find((item) => item.id === artifactId);
    closeEditor();
    setSelectedArtifact(artifactId);
    setSelectedSavedArtifact(null);
    setViewingCandidate(Boolean(artifact?.candidateAvailable && !artifact.available));
    setComparingCandidate(false);
    setMetaOpen(false);
    setCandidateActionPending(null);
  }

  function selectSavedArtifact(artifact: SavedArtifactSummary) {
    if (navigationLocked) {
      return;
    }
    closeEditor();
    setSelectedArtifact(artifact.artifactId);
    setSelectedSavedArtifact(artifact);
    setViewingCandidate(false);
    setComparingCandidate(false);
    setMetaOpen(false);
    setCandidateActionPending(null);
  }

  function copyDocument() {
    if (!artifactDocument || !navigator.clipboard) {
      return;
    }
    void navigator.clipboard.writeText(artifactDocument.markdown).then(() => setCopied(true));
  }

  function updateDraft(nextDraft: string) {
    setDraft(nextDraft);
    draftRef.current = nextDraft;
    if (!workspace || !draftRevision) {
      return;
    }
    if (!storeArtifactDraft(
      workspace.campaign.id,
      workspace.session.stem,
      selectedArtifact,
      nextDraft,
      draftRevision,
    )) {
      setSaveWarning("This draft could not be stored locally. Save it before leaving the editor.");
    }
  }

  function startEditing() {
    if (!artifactDocument || artifactDocument.candidate) {
      return;
    }
    const storedDraft = loadArtifactDraft(workspace?.campaign.id, workspace?.session.stem, selectedArtifact);
    const recoveredDraft = storedDraft?.markdown !== artifactDocument.markdown ? storedDraft : null;
    const markdown = recoveredDraft?.markdown ?? artifactDocument.markdown;
    const revision = recoveredDraft?.revision ?? artifactDocument.revision;
    setEditing(true);
    setDraft(markdown);
    setSavedMarkdown(artifactDocument.markdown);
    setDraftRevision(revision);
    setSaveError(null);
    setSaveWarning(recoveredDraft ? "Recovered an unsaved draft from this device." : null);
    setConflictDocument(null);
    setComparingEditConflict(false);
    draftRef.current = markdown;
  }

  function closeEditor() {
    if (navigationLocked) {
      return;
    }
    setEditing(false);
    setSaveError(null);
    setSaveWarning(null);
    setConflictDocument(null);
    setComparingEditConflict(false);
    if (workspace) {
      clearArtifactDraft(workspace.campaign.id, workspace.session.stem, selectedArtifact);
    }
  }

  function takeDiskVersion() {
    if (!conflictDocument) {
      return;
    }
    setArtifactDocument(conflictDocument);
    setDraft(conflictDocument.markdown);
    setSavedMarkdown(conflictDocument.markdown);
    setDraftRevision(conflictDocument.revision);
    setSaveError(null);
    setConflictDocument(null);
    setComparingEditConflict(false);
    draftRef.current = conflictDocument.markdown;
    if (workspace) {
      clearArtifactDraft(workspace.campaign.id, workspace.session.stem, selectedArtifact);
    }
  }

  if (workspaceLoading) {
    return <WorkspaceLoading />;
  }

  if (workspaceError || !workspace) {
    return (
      <section className="state-panel state-panel--error" aria-live="polite">
        <CircleAlert size={22} aria-hidden="true" />
        <div>
          <p className="eyebrow">Session unavailable</p>
          <h1>The session workspace could not be read.</h1>
          <p>{workspaceError ?? "The selected session no longer exists in this campaign."}</p>
          <button className="button button--quiet" type="button" onClick={onBack}>
            <ArrowLeft size={16} aria-hidden="true" />
            Back to sessions
          </button>
        </div>
      </section>
    );
  }

  const activeArtifact = workspace.artifacts.find((item) => item.id === selectedArtifact);
  const activeSavedArtifact = selectedSavedArtifact
    ? workspace.savedArtifacts.find((item) => item.filename === selectedSavedArtifact.filename) ?? null
    : null;
  const candidateOnly = Boolean(activeArtifact?.candidateAvailable && !activeArtifact.available);
  const candidateAction = candidateActionPending
    ? candidateResolutionCopy(candidateActionPending, candidateOnly)
    : null;
  const canResolveCandidate = Boolean(activeArtifact?.candidateAvailable && !activeSavedArtifact);
  const canCompareCandidate = Boolean(
    activeArtifact?.candidateAvailable && activeArtifact.available && !activeSavedArtifact,
  );
  const canEdit = Boolean(
    activeArtifact?.available
      && artifactDocument
      && !artifactDocument.candidate
      && !activeSavedArtifact
      && !viewingCandidate
      && !comparingCandidate
      && !metaOpen,
  );
  const hasSessionAudio = workspace.session.hasAudio || Boolean(workspace.provenance?.sourceAudio);
  const hasExportableNotes = workspace.artifacts.some((artifact) => artifact.available);

  return (
    <div className="workspace-page">
      <header className="workspace-header">
        <div className="workspace-header__title">
          <button className="back-button" type="button" onClick={onBack} disabled={navigationLocked}>
            <ArrowLeft size={16} aria-hidden="true" />
            Sessions
          </button>
          <div>
            <p className="eyebrow">{workspace.campaign.name}</p>
            <h1>{workspace.session.stem}</h1>
            <p className="page-subtitle">
              {workspace.session.hasTranscript ? "Transcript available" : "No transcript found"}
              {workspace.session.modifiedAt ? ` · Updated ${formatDate(workspace.session.modifiedAt)}` : ""}
            </p>
          </div>
        </div>
        <div className="page-actions">
          <button
            className="button button--quiet"
            type="button"
            onClick={onExport}
            disabled={exporting || !hasExportableNotes}
            title={exporting ? "An export is already active." : "Export this session's generated notes"}
          >
            {exporting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <FileOutput size={16} aria-hidden="true" />}
            {exporting ? "Exporting" : "Export"}
          </button>
          {workspace.session.hasTranscript && (
            <button
              className="button button--primary"
              type="button"
              onClick={onGenerateNotes}
              disabled={generating}
              title={generating ? "A pipeline job is already active." : "Generate notes from this transcript"}
            >
              {generating ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Sparkles size={16} aria-hidden="true" />}
              {generating ? "Generating" : "Generate notes"}
            </button>
          )}
          {workspace.session.hasTranscript && (
            <button
              className="button button--quiet"
              type="button"
              onClick={onReviewSpeakers}
              disabled={speakerMapping}
              title={speakerMapping ? "A speaker mapping update is already active." : "Review diarized speaker labels"}
            >
              {speakerMapping ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <UsersRound size={16} aria-hidden="true" />}
              {speakerMapping ? "Mapping" : "Review speakers"}
            </button>
          )}
          {workspace.transcript && (
            <button
              className={transcriptOpen ? "button button--quiet button--selected" : "button button--quiet"}
              type="button"
              onClick={() => setTranscriptOpen((open) => !open)}
              aria-pressed={transcriptOpen}
            >
              <AudioLines size={16} aria-hidden="true" />
              Transcript
            </button>
          )}
        </div>
      </header>

      {hasSessionAudio && (
        <AudioTransport
          snapshot={audioSnapshot}
          busy={audioBusy}
          error={audioError}
          onToggle={() => void toggleAudioPlayback()}
          onSeek={(positionMs) => void seekAudio(positionMs)}
          onStop={() => void stopAudioPlayback()}
        />
      )}

      <div className={transcriptOpen ? "workspace-grid workspace-grid--transcript-open" : "workspace-grid"}>
        <aside className="document-nav" aria-label="Session documents">
          <p className="document-nav__label">Documents</p>
          <div className="document-nav__list">
            {artifactOrder.map((artifactId) => {
              const artifact = workspace.artifacts.find((item) => item.id === artifactId);
              const available = Boolean(artifact?.available || artifact?.candidateAvailable);
              const selected = artifactId === selectedArtifact && !activeSavedArtifact && !metaOpen;
              return (
                <button
                  className={selected ? "document-nav__item document-nav__item--active" : "document-nav__item"}
                  key={artifactId}
                  type="button"
                  disabled={!available || navigationLocked}
                  onClick={() => selectArtifact(artifactId)}
                  title={available ? `Read ${artifact?.label}` : `${artifact?.label ?? formatArtifact(artifactId)} is not generated`}
                >
                  <FileText size={15} aria-hidden="true" />
                  <span>{artifact?.label ?? formatArtifact(artifactId)}</span>
                  {artifact?.candidateAvailable && <i className="candidate-marker" aria-label="Candidate available" />}
                </button>
              );
            })}
          </div>
          {workspace.savedArtifacts.length > 0 && (
            <>
              <div className="document-nav__divider" />
              <p className="document-nav__label">Saved versions</p>
              <div className="document-nav__list">
                {workspace.savedArtifacts.map((artifact) => (
                  <button
                    className={activeSavedArtifact?.filename === artifact.filename ? "document-nav__item document-nav__item--active" : "document-nav__item"}
                    key={artifact.filename}
                    type="button"
                    onClick={() => selectSavedArtifact(artifact)}
                    disabled={navigationLocked}
                    title={`Read ${artifact.label}`}
                  >
                    <BookOpenText size={15} aria-hidden="true" />
                    <span>{artifact.label}</span>
                  </button>
                ))}
              </div>
            </>
          )}
          <div className="document-nav__divider" />
          <button
            className={transcriptOpen ? "document-nav__item document-nav__item--active" : "document-nav__item"}
            type="button"
            disabled={!workspace.transcript}
            onClick={() => setTranscriptOpen((open) => !open)}
          >
            <AudioLines size={15} aria-hidden="true" />
            <span>Transcript</span>
          </button>
          <button
            className={metaOpen ? "document-nav__item document-nav__item--active" : "document-nav__item"}
            type="button"
            onClick={() => {
              if (navigationLocked) {
                return;
              }
              closeEditor();
              setMetaOpen((open) => !open);
              setComparingCandidate(false);
              setCandidateActionPending(null);
            }}
            disabled={navigationLocked}
            aria-pressed={metaOpen}
          >
            <Info size={15} aria-hidden="true" />
            <span>Meta</span>
          </button>
        </aside>

        <section className="reader-pane" aria-label="Artifact reader">
          <header className="reader-pane__header">
            <div>
              <p className="eyebrow">{metaOpen ? "Session data" : activeSavedArtifact?.label ?? activeArtifact?.label ?? "Document"}</p>
              <h2>{metaOpen ? "Provenance" : comparingCandidate ? "Compare versions" : activeSavedArtifact?.label ?? (viewingCandidate ? "Candidate version" : activeArtifact?.label ?? "Not generated")}</h2>
            </div>
            <div className="reader-pane__actions">
              {!metaOpen && !editing && canCompareCandidate && (
                <button
                  className="button button--quiet button--compact"
                  type="button"
                  onClick={() => setComparingCandidate((comparing) => !comparing)}
                  aria-pressed={comparingCandidate}
                >
                  <Columns2 size={15} aria-hidden="true" />
                  {comparingCandidate ? "View document" : "Compare"}
                </button>
              )}
              {!metaOpen && !editing && !activeSavedArtifact && activeArtifact?.candidateAvailable && activeArtifact.available && (
                <button
                  className="button button--quiet button--compact"
                  type="button"
                  onClick={() => {
                    if (navigationLocked) {
                      return;
                    }
                    closeEditor();
                    setViewingCandidate((candidate) => !candidate);
                  }}
                  disabled={navigationLocked}
                  aria-pressed={viewingCandidate}
                >
                  {viewingCandidate ? "View current" : "View candidate"}
                </button>
              )}
              {!metaOpen && !editing && !activeSavedArtifact && candidateOnly && <span className="reader-status">Candidate only</span>}
              {!metaOpen && !editing && canResolveCandidate && (
                candidateAction ? (
                  <div className="candidate-resolution candidate-resolution--confirm" role="group" aria-label={candidateAction.confirmation}>
                    <span>{candidateAction.confirmation}</span>
                    <button
                      className="icon-button"
                      type="button"
                      disabled={candidateResolving}
                      onClick={() => setCandidateActionPending(null)}
                      title="Cancel candidate resolution"
                      aria-label="Cancel candidate resolution"
                    >
                      <X size={15} aria-hidden="true" />
                    </button>
                    <button
                      className={candidateAction.action === "discardCandidate" ? "button button--compact candidate-resolution__discard" : "button button--primary button--compact"}
                      type="button"
                      disabled={candidateResolving}
                      onClick={() => {
                        if (activeArtifact) {
                          onResolveCandidate(activeArtifact.id, candidateAction.action);
                          setCandidateActionPending(null);
                        }
                      }}
                    >
                      {candidateResolving ? <LoaderCircle className="is-spinning" size={15} aria-hidden="true" /> : candidateAction.action === "discardCandidate" ? <Trash2 size={15} aria-hidden="true" /> : <Check size={15} aria-hidden="true" />}
                      {candidateAction.confirmLabel}
                    </button>
                  </div>
                ) : (
                  <div className="candidate-resolution" role="group" aria-label="Resolve candidate artifact">
                    <button
                      className="button button--primary button--compact"
                      type="button"
                      disabled={candidateResolving}
                      onClick={() => setCandidateActionPending("keepCandidate")}
                    >
                      {candidateResolving ? <LoaderCircle className="is-spinning" size={15} aria-hidden="true" /> : <Check size={15} aria-hidden="true" />}
                      {candidateResolving ? "Resolving" : candidateOnly ? "Keep candidate" : "Replace current"}
                    </button>
                    {!candidateOnly && (
                      <button
                        className="button button--quiet button--compact"
                        type="button"
                        disabled={candidateResolving}
                        onClick={() => setCandidateActionPending("keepBoth")}
                      >
                        Keep both
                      </button>
                    )}
                    <button
                      className="button button--quiet button--compact candidate-resolution__discard"
                      type="button"
                      disabled={candidateResolving}
                      onClick={() => setCandidateActionPending("discardCandidate")}
                    >
                      <Trash2 size={15} aria-hidden="true" />
                      Discard candidate
                    </button>
                  </div>
                )
              )}
              {canEdit && !editing && (
                <button
                  className="icon-button"
                  type="button"
                  onClick={startEditing}
                  title="Edit document"
                  aria-label="Edit document"
                >
                  <Pencil size={16} aria-hidden="true" />
                </button>
              )}
              {editing && (
                <>
                  <span className={saving ? "editor-status editor-status--saving" : draftIsDirty ? "editor-status editor-status--dirty" : "editor-status"}>
                    {saving ? "Saving" : draftIsDirty ? "Unsaved" : "Saved"}
                  </span>
                  <button
                    className="icon-button"
                    type="button"
                    disabled={!draftIsDirty || saving}
                    onClick={() => void saveDraft()}
                    title="Save document"
                    aria-label="Save document"
                  >
                    <Save size={16} aria-hidden="true" />
                  </button>
                  <button
                    className="icon-button"
                    type="button"
                    disabled={navigationLocked}
                    onClick={closeEditor}
                    title="Exit editor"
                    aria-label="Exit editor"
                  >
                    <Check size={16} aria-hidden="true" />
                  </button>
                </>
              )}
              <button
                className="icon-button"
                type="button"
                disabled={!artifactDocument || metaOpen || comparingCandidate || editing}
                onClick={copyDocument}
                title={copied ? "Copied document" : "Copy document"}
                aria-label={copied ? "Copied document" : "Copy document"}
              >
                <ClipboardCopy size={16} aria-hidden="true" />
              </button>
            </div>
          </header>

          {candidateResolveError && !metaOpen && !activeSavedArtifact && (
            <p className="candidate-resolution__error" role="alert">{candidateResolveError}</p>
          )}

          {campaignLogRebuildRecommended && (
            <aside className="campaign-log-handoff" aria-label="Campaign log update">
              <BookOpenText size={17} aria-hidden="true" />
              <div>
                <strong>Campaign log needs an update</strong>
                <span>A summary or bullets candidate is now current.</span>
              </div>
              <button
                className="button button--primary button--compact"
                type="button"
                disabled={campaignLogRebuilding}
                onClick={onRebuildCampaignLog}
              >
                {campaignLogRebuilding ? <LoaderCircle className="is-spinning" size={15} aria-hidden="true" /> : <RefreshCw size={15} aria-hidden="true" />}
                {campaignLogRebuilding ? "Rebuilding" : "Rebuild log"}
              </button>
              <button
                className="icon-button"
                type="button"
                disabled={campaignLogRebuilding}
                onClick={onDismissCampaignLogRebuild}
                title="Dismiss campaign log update"
                aria-label="Dismiss campaign log update"
              >
                <X size={16} aria-hidden="true" />
              </button>
            </aside>
          )}

          <div className="reader-pane__body">
            {editing ? (
              <Suspense fallback={<ReaderLoading />}>
                <MarkdownEditor
                  draft={draft}
                  onChange={updateDraft}
                  saveError={saveError}
                  saveWarning={saveWarning}
                  conflictDocument={conflictDocument}
                  comparingConflict={comparingEditConflict}
                  onCompareConflict={() => setComparingEditConflict((comparing) => !comparing)}
                  onKeepMine={() => void saveDraft(conflictDocument?.revision)}
                  onTakeDisk={takeDiskVersion}
                />
              </Suspense>
            ) : metaOpen ? (
              <SessionProvenancePanel provenance={workspace.provenance} />
            ) : comparingCandidate && activeArtifact ? (
              <CandidateComparison
                campaignId={workspace.campaign.id}
                stem={workspace.session.stem}
                artifactId={activeArtifact.id}
              />
            ) : documentLoading ? (
              <ReaderLoading />
            ) : documentError ? (
              <ReaderState error={documentError} />
            ) : artifactDocument ? (
              <MarkdownDocument markdown={artifactDocument.markdown} />
            ) : (
              <ReaderState />
            )}
          </div>
        </section>

        {transcriptOpen && workspace.transcript && (
          <TranscriptPanel
            campaignId={workspace.campaign.id}
            stem={workspace.session.stem}
            totalLines={workspace.transcript.totalLines}
            refreshKey={refreshKey}
            onSeekTo={hasSessionAudio ? (positionMs) => void seekAudio(positionMs) : undefined}
            playbackPositionMs={audioSnapshot.positionMs}
            playing={audioSnapshot.status === "playing"}
          />
        )}
      </div>
    </div>
  );
}

export function CampaignLogPage({
  campaign,
  rebuilding,
  exporting,
  canExport,
  reloadKey,
  onRebuild,
  onExport,
}: {
  campaign?: CampaignSummary;
  rebuilding: boolean;
  exporting: boolean;
  canExport: boolean;
  reloadKey: number;
  onRebuild: () => void;
  onExport: () => void;
}) {
  const [document, setDocument] = useState<CampaignLogDocument | null>(null);
  const [loading, setLoading] = useState(Boolean(campaign));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!campaign) {
      setDocument(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    void desktop
      .campaignLogRead(campaign.id)
      .then((nextDocument) => {
        if (!cancelled) {
          setDocument(nextDocument);
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setDocument(null);
          setError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [campaign?.id, reloadKey]);

  return (
    <div className="log-page">
      <header className="page-header">
        <div>
          <p className="eyebrow">{campaign?.name ?? "SessionSmith"}</p>
          <h1>Campaign log</h1>
          <p className="page-subtitle">
            {document?.modifiedAt ? `Updated ${formatDate(document.modifiedAt)}` : "Campaign history and continuity notes"}
          </p>
        </div>
        <div className="page-actions">
          <button
            className="button button--quiet"
            type="button"
            onClick={onExport}
            disabled={!campaign || !canExport || exporting}
            title={exporting ? "An export is already active." : "Export campaign notes and log"}
          >
            {exporting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <FileOutput size={16} aria-hidden="true" />}
            {exporting ? "Exporting" : "Export"}
          </button>
          <button
            className="button button--quiet"
            type="button"
            onClick={onRebuild}
            disabled={!campaign || rebuilding}
            title={rebuilding ? "A campaign log rebuild is already active." : "Rebuild campaign log"}
          >
            <RefreshCw className={rebuilding ? "is-spinning" : ""} size={16} aria-hidden="true" />
            {rebuilding ? "Rebuilding" : "Rebuild log"}
          </button>
        </div>
      </header>
      <section className="log-reader" aria-label="Campaign log reader">
        {loading ? (
          <ReaderLoading />
        ) : error ? (
          <ReaderState error={error} />
        ) : document ? (
          <MarkdownDocument markdown={document.markdown} />
        ) : (
          <ReaderState />
        )}
      </section>
    </div>
  );
}

function WorkspaceLoading() {
  return (
    <div className="workspace-page workspace-page--loading" aria-live="polite">
      <header className="workspace-header">
        <div>
          <p className="eyebrow">Reading session</p>
          <h1>Session workspace</h1>
        </div>
        <LoaderCircle className="is-spinning" size={22} aria-label="Loading session workspace" />
      </header>
      <div className="workspace-loading-grid">
        <div className="skeleton-block skeleton-block--rail" />
        <div className="skeleton-block skeleton-block--reader" />
      </div>
    </div>
  );
}

function ReaderLoading() {
  return (
    <div className="reader-loading" aria-live="polite">
      <LoaderCircle className="is-spinning" size={20} aria-label="Loading document" />
      <span>Reading document</span>
    </div>
  );
}

function ReaderState({ error }: { error?: string }) {
  return (
    <div className={error ? "reader-state reader-state--error" : "reader-state"}>
      <BookOpenText size={24} aria-hidden="true" />
      <strong>{error ? "The document could not be read." : "This document has not been generated yet."}</strong>
      {error && <span>{error}</span>}
    </div>
  );
}

function MarkdownDocument({ markdown }: { markdown: string }) {
  return (
    <div className="markdown-document">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{markdown}</ReactMarkdown>
    </div>
  );
}

function CandidateComparison({
  campaignId,
  stem,
  artifactId,
}: {
  campaignId: string;
  stem: string;
  artifactId: ArtifactId;
}) {
  const [documents, setDocuments] = useState<{ current: ArtifactDocument; candidate: ArtifactDocument } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDocuments(null);
    void Promise.all([
      desktop.artifactRead(campaignId, stem, artifactId, false),
      desktop.artifactRead(campaignId, stem, artifactId, true),
    ])
      .then(([current, candidate]) => {
        if (!cancelled) {
          setDocuments({ current, candidate });
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [artifactId, campaignId, stem]);

  if (loading) {
    return <ReaderLoading />;
  }
  if (error || !documents) {
    return <ReaderState error={error ?? "The candidate comparison could not be read."} />;
  }

  const changes = diffWordsWithSpace(documents.current.markdown, documents.candidate.markdown);

  return (
    <section className="candidate-comparison" aria-label="Current and candidate document comparison">
      <p className="candidate-comparison__detail">Changed words are highlighted in each version.</p>
      <div className="candidate-comparison__grid">
        <ComparisonColumn title="Current" changes={changes} showCandidate={false} />
        <ComparisonColumn title="Candidate" changes={changes} showCandidate />
      </div>
    </section>
  );
}

function SessionProvenancePanel({ provenance }: { provenance: SessionProvenance | null }) {
  if (!provenance) {
    return <ReaderState error="This session has no readable transcription metadata." />;
  }

  const sourceFiles = provenance.sourceFiles.filter((file, index, files) => files.indexOf(file) === index);

  return (
    <section className="provenance-panel" aria-label="Session provenance">
      <dl className="provenance-panel__facts">
        <div>
          <dt>Transcription model</dt>
          <dd>{provenance.model || "Unavailable"}</dd>
        </div>
        <div>
          <dt>Engine</dt>
          <dd>{provenance.engine || "Unavailable"}</dd>
        </div>
        <div>
          <dt>Language</dt>
          <dd>{provenance.language || "Auto"}</dd>
        </div>
        <div>
          <dt>Session date</dt>
          <dd>{provenance.sessionDate || "Unavailable"}</dd>
        </div>
        <div>
          <dt>Transcript created</dt>
          <dd>{provenance.createdAt ? formatDate(provenance.createdAt) : "Unavailable"}</dd>
        </div>
        <div>
          <dt>Voice activity detection</dt>
          <dd>{provenance.vad ? "Enabled" : "Not used"}</dd>
        </div>
        <div>
          <dt>Mapped speakers</dt>
          <dd>{provenance.mappedSpeakers.toLocaleString()}</dd>
        </div>
      </dl>
      <div className="provenance-panel__sources">
        <div>
          <h3>Primary audio</h3>
          <p>{provenance.sourceAudio || "Unavailable"}</p>
        </div>
        <div>
          <h3>Session inputs</h3>
          {sourceFiles.length > 0 ? (
            <ul>
              {sourceFiles.map((file) => <li key={file}>{file}</li>)}
            </ul>
          ) : (
            <p>Unavailable</p>
          )}
        </div>
      </div>
    </section>
  );
}

function ComparisonColumn({
  title,
  changes,
  showCandidate,
}: {
  title: string;
  changes: ReturnType<typeof diffWordsWithSpace>;
  showCandidate: boolean;
}) {
  return (
    <section className={showCandidate ? "candidate-comparison__column candidate-comparison__column--candidate" : "candidate-comparison__column"}>
      <h3>{title}</h3>
      <pre>
        {changes.map((change, index) => {
          if ((showCandidate && change.removed) || (!showCandidate && change.added)) {
            return null;
          }
          const className = change.added
            ? "candidate-comparison__change candidate-comparison__change--added"
            : change.removed
              ? "candidate-comparison__change candidate-comparison__change--removed"
              : undefined;
          return <span className={className} key={index}>{change.value}</span>;
        })}
      </pre>
    </section>
  );
}

function TranscriptPanel({
  campaignId,
  stem,
  totalLines,
  refreshKey,
  onSeekTo,
  playbackPositionMs,
  playing,
}: {
  campaignId: string;
  stem: string;
  totalLines: number;
  refreshKey: number;
  onSeekTo?: (positionMs: number) => void;
  playbackPositionMs: number;
  playing: boolean;
}) {
  const [page, setPage] = useState<TranscriptPage | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [followingPlayback, setFollowingPlayback] = useState(true);
  const transcriptBodyRef = useRef<HTMLDivElement | null>(null);
  const deferredFilter = useDeferredValue(filter);
  const rowVirtualizer = useVirtualizer({
    count: page?.lines.length ?? 0,
    getScrollElement: () => transcriptBodyRef.current,
    estimateSize: () => 52,
    overscan: 8,
  });

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setPage(null);
    void desktop
      .transcriptRead(campaignId, stem, 0, 240, deferredFilter)
      .then((nextPage) => {
        if (!cancelled) {
          setPage(nextPage);
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [campaignId, deferredFilter, refreshKey, stem]);

  async function loadMore() {
    if (!page || loadingMore) {
      return;
    }
    setLoadingMore(true);
    try {
      const nextPage = await desktop.transcriptRead(
        campaignId,
        stem,
        page.offsetLine + page.lines.length,
        240,
        deferredFilter,
      );
      setPage((currentPage) => (
        currentPage
          ? { ...nextPage, offsetLine: currentPage.offsetLine, lines: [...currentPage.lines, ...nextPage.lines] }
          : nextPage
      ));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setLoadingMore(false);
    }
  }

  const loadedLines = page?.lines.length ?? 0;
  const hasMore = Boolean(page && page.offsetLine + loadedLines < page.totalLines);
  const displayedTotal = page?.totalLines ?? totalLines;
  const filtering = filter.trim().length > 0;
  const activeLineNumber = findActiveTranscriptLine(page?.lines ?? [], playbackPositionMs);
  const activeLineIndex = activeLineNumber === null
    ? -1
    : page?.lines.findIndex((line) => line.lineNumber === activeLineNumber) ?? -1;

  useEffect(() => {
    if (followingPlayback && playing && activeLineIndex >= 0) {
      rowVirtualizer.scrollToIndex(activeLineIndex, { align: "auto", behavior: "smooth" });
    }
  }, [activeLineIndex, followingPlayback, playing, rowVirtualizer]);

  return (
    <aside className="transcript-panel" aria-label="Transcript">
      <header className="transcript-panel__header">
        <div>
          <p className="eyebrow">Transcript</p>
          <h2>{displayedTotal.toLocaleString()} {filtering ? "matches" : "lines"}</h2>
        </div>
        <div className="transcript-panel__header-actions">
          <button
            className={followingPlayback ? "icon-button transcript-follow transcript-follow--active" : "icon-button transcript-follow"}
            type="button"
            onClick={() => setFollowingPlayback((following) => !following)}
            aria-pressed={followingPlayback}
            title={followingPlayback ? "Stop following playback" : "Follow playback"}
            aria-label={followingPlayback ? "Stop following playback" : "Follow playback"}
          >
            <LocateFixed size={15} aria-hidden="true" />
          </button>
          <label className="transcript-filter">
            <Search size={14} aria-hidden="true" />
            <span className="sr-only">Filter transcript</span>
            <input
              type="search"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="Filter text or speaker"
            />
          </label>
        </div>
      </header>
      <div className="transcript-panel__body" ref={transcriptBodyRef}>
        {loading ? (
          <ReaderLoading />
        ) : error ? (
          <ReaderState error={error} />
        ) : page?.lines.length ? (
          <ol
            className="transcript-lines transcript-lines--virtual"
            start={page.offsetLine + 1}
            style={{ height: `${rowVirtualizer.getTotalSize()}px` }}
          >
            {rowVirtualizer.getVirtualItems().map((virtualRow) => {
              const line = page.lines[virtualRow.index];
              const timestamp = line.t0;
              return (
                <li
                  className={line.lineNumber === activeLineNumber ? "transcript-lines__item--active" : undefined}
                  data-transcript-line={line.lineNumber}
                  data-index={virtualRow.index}
                  key={line.lineNumber}
                  ref={rowVirtualizer.measureElement}
                  value={line.lineNumber}
                  aria-posinset={line.lineNumber}
                  aria-setsize={displayedTotal}
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  {timestamp !== null && (onSeekTo ? (
                  <button
                    className="transcript-time-link"
                    type="button"
                    onClick={() => onSeekTo(Math.round(timestamp * 1_000))}
                    title={`Seek audio to ${formatTranscriptTime(timestamp)}`}
                  >
                    <time dateTime={`PT${timestamp}S`}>{formatTranscriptTime(timestamp)}</time>
                  </button>
                ) : (
                  <time dateTime={`PT${timestamp}S`}>{formatTranscriptTime(timestamp)}</time>
                ))}
                  {line.speaker && <strong>{line.speaker}</strong>}
                  <span>{line.text}</span>
                </li>
              );
            })}
          </ol>
        ) : (
          <ReaderState />
        )}
      </div>
      {hasMore && (
        <button className="button button--quiet transcript-panel__more" type="button" onClick={() => void loadMore()} disabled={loadingMore}>
          {loadingMore ? "Loading lines" : `Load more (${Math.max(0, displayedTotal - loadedLines).toLocaleString()} remaining)`}
        </button>
      )}
    </aside>
  );
}

function AudioTransport({
  snapshot,
  busy,
  error,
  onToggle,
  onSeek,
  onStop,
}: {
  snapshot: AudioPlayerSnapshot;
  busy: "toggle" | "seek" | "stop" | null;
  error: string | null;
  onToggle: () => void;
  onSeek: (positionMs: number) => void;
  onStop: () => void;
}) {
  const [scrubbing, setScrubbing] = useState(false);
  const [seekPosition, setSeekPosition] = useState(0);
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

  function beginSeeking(positionMs: number) {
    setScrubbing(true);
    setSeekPosition(positionMs);
  }

  function finishSeeking(positionMs: number) {
    if (!scrubbing) {
      return;
    }
    const target = Math.max(0, Math.min(maximum, Math.round(positionMs)));
    setScrubbing(false);
    setSeekPosition(target);
    onSeek(target);
  }

  return (
    <section className="audio-transport" aria-label="Session audio">
      <div className="audio-transport__identity">
        <AudioLines size={17} aria-hidden="true" />
        <span title={snapshot.label ?? "Session audio"}>{snapshot.label ?? "Session audio"}</span>
      </div>
      <div className="audio-transport__controls">
        <button
          className="icon-button audio-transport__button audio-transport__button--play"
          type="button"
          onClick={onToggle}
          disabled={busy !== null}
          title={playing ? "Pause audio" : loaded ? "Play audio" : "Load and play audio"}
          aria-label={playing ? "Pause audio" : loaded ? "Play audio" : "Load and play audio"}
        >
          {busy === "toggle" ? <LoaderCircle className="is-spinning" size={17} aria-hidden="true" /> : playing ? <Pause size={17} aria-hidden="true" /> : <Play size={17} aria-hidden="true" />}
        </button>
        <button
          className="icon-button audio-transport__button"
          type="button"
          onClick={onStop}
          disabled={!loaded || busy !== null}
          title="Stop audio"
          aria-label="Stop audio"
        >
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
          onPointerDown={(event) => beginSeeking(Number(event.currentTarget.value))}
          onChange={(event) => beginSeeking(Number(event.currentTarget.value))}
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
      <span className="audio-transport__status">{formatAudioStatus(snapshot.status)}</span>
      {message && (
        <p className="audio-transport__error" role="alert">
          <CircleAlert size={15} aria-hidden="true" />
          {message}
        </p>
      )}
    </section>
  );
}

function formatArtifact(artifact: string) {
  return artifact
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function findActiveTranscriptLine(lines: TranscriptPage["lines"], playbackPositionMs: number) {
  const playbackSeconds = Math.max(0, playbackPositionMs / 1_000);
  let mostRecentLine: number | null = null;
  for (const line of lines) {
    if (line.t0 === null || line.t0 > playbackSeconds) {
      continue;
    }
    mostRecentLine = line.lineNumber;
    if (line.t1 !== null && playbackSeconds <= line.t1) {
      return line.lineNumber;
    }
  }
  return mostRecentLine;
}

function formatTranscriptTime(seconds: number) {
  const wholeSeconds = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(wholeSeconds / 3_600);
  const minutes = Math.floor((wholeSeconds % 3_600) / 60);
  const remainingSeconds = wholeSeconds % 60;
  const paddedMinutes = minutes.toString().padStart(2, "0");
  const paddedSeconds = remainingSeconds.toString().padStart(2, "0");
  return hours > 0 ? `${hours}:${paddedMinutes}:${paddedSeconds}` : `${paddedMinutes}:${paddedSeconds}`;
}

function formatPlaybackTime(milliseconds: number) {
  return formatTranscriptTime(milliseconds / 1_000);
}

function formatAudioStatus(status: AudioPlayerSnapshot["status"]) {
  switch (status) {
    case "playing":
      return "Playing";
    case "paused":
      return "Paused";
    case "stopped":
      return "Stopped";
    case "ended":
      return "Finished";
    default:
      return "Ready";
  }
}

function artifactDraftStorageKey(
  campaignId: string | undefined,
  stem: string | undefined,
  artifactId: ArtifactId,
) {
  if (!campaignId || !stem || typeof window === "undefined") {
    return null;
  }
  return `${artifactDraftStoragePrefix}${encodeURIComponent(campaignId)}:${encodeURIComponent(stem)}:${artifactId}`;
}

function loadArtifactDraft(
  campaignId: string | undefined,
  stem: string | undefined,
  artifactId: ArtifactId,
) {
  const key = artifactDraftStorageKey(campaignId, stem, artifactId);
  if (!key) {
    return null;
  }
  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) {
      return null;
    }
    const stored = JSON.parse(raw) as { markdown?: unknown; revision?: unknown };
    if (
      typeof stored.markdown !== "string"
      || stored.markdown.length > 4 * 1024 * 1024
      || typeof stored.revision !== "string"
      || !/^[a-f0-9]{64}$/i.test(stored.revision)
    ) {
      window.localStorage.removeItem(key);
      return null;
    }
    return { markdown: stored.markdown, revision: stored.revision };
  } catch {
    return null;
  }
}

function storeArtifactDraft(
  campaignId: string,
  stem: string,
  artifactId: ArtifactId,
  markdown: string,
  revision: string,
) {
  const key = artifactDraftStorageKey(campaignId, stem, artifactId);
  if (!key || markdown.length > 4 * 1024 * 1024 || !/^[a-f0-9]{64}$/i.test(revision)) {
    return false;
  }
  try {
    window.localStorage.setItem(key, JSON.stringify({ markdown, revision }));
    return true;
  } catch {
    return false;
  }
}

function clearArtifactDraft(campaignId: string, stem: string, artifactId: ArtifactId) {
  const key = artifactDraftStorageKey(campaignId, stem, artifactId);
  if (!key) {
    return;
  }
  try {
    window.localStorage.removeItem(key);
  } catch {
    // Storage cleanup should not prevent closing an already-saved document.
  }
}

function candidateResolutionCopy(action: CandidateAction, candidateOnly: boolean) {
  switch (action) {
    case "keepCandidate":
      return {
        action,
        confirmation: candidateOnly ? "Make this candidate the current document?" : "Replace the current document with this candidate?",
        confirmLabel: candidateOnly ? "Keep" : "Replace",
      };
    case "keepBoth":
      return {
        action,
        confirmation: "Save this candidate alongside the current document?",
        confirmLabel: "Keep both",
      };
    case "discardCandidate":
      return {
        action,
        confirmation: "Discard this candidate version?",
        confirmLabel: "Discard",
      };
  }
}

function formatDate(timestamp: number | null) {
  if (!timestamp) {
    return "No modified date";
  }
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" }).format(
    new Date(timestamp * 1000),
  );
}
