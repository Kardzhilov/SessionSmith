import { markdown } from "@codemirror/lang-markdown";
import { EditorView } from "@codemirror/view";
import CodeMirror from "@uiw/react-codemirror";
import { diffWordsWithSpace } from "diff";
import { CircleAlert } from "lucide-react";
import type { ArtifactDocument } from "../../api/types";

const markdownEditorExtensions = [markdown(), EditorView.lineWrapping];
const markdownEditorSetup = {
  autocompletion: false,
  foldGutter: false,
  lineNumbers: true,
};

export function MarkdownEditor({
  draft,
  onChange,
  saveError,
  saveWarning,
  conflictDocument,
  comparingConflict,
  onCompareConflict,
  onKeepMine,
  onTakeDisk,
}: {
  draft: string;
  onChange: (value: string) => void;
  saveError: string | null;
  saveWarning: string | null;
  conflictDocument: ArtifactDocument | null;
  comparingConflict: boolean;
  onCompareConflict: () => void;
  onKeepMine: () => void;
  onTakeDisk: () => void;
}) {
  const changes = conflictDocument
    ? diffWordsWithSpace(conflictDocument.markdown, draft)
    : null;

  return (
    <section className="markdown-editor" aria-label="Markdown document editor">
      {saveError && (
        <div className="editor-message editor-message--error" role="alert">
          <CircleAlert size={16} aria-hidden="true" />
          <span>{saveError}</span>
          {conflictDocument ? (
            <div className="editor-conflict__actions">
              <button className="button button--quiet button--compact" type="button" onClick={onCompareConflict}>
                {comparingConflict ? "Hide comparison" : "Compare"}
              </button>
              <button className="button button--primary button--compact" type="button" onClick={onKeepMine}>
                Keep mine
              </button>
              <button className="button button--quiet button--compact" type="button" onClick={onTakeDisk}>
                Take disk
              </button>
            </div>
          ) : saveError.startsWith("This document changed on disk") ? (
            <span className="editor-message__detail">Reading the latest disk version.</span>
          ) : null}
        </div>
      )}
      {saveWarning && (
        <div className="editor-message editor-message--warning" role="status">
          <CircleAlert size={16} aria-hidden="true" />
          <span>{saveWarning}</span>
        </div>
      )}
      <CodeMirror
        value={draft}
        height="100%"
        minHeight="500px"
        extensions={markdownEditorExtensions}
        basicSetup={markdownEditorSetup}
        indentWithTab
        onChange={onChange}
        aria-label="Markdown document editor"
      />
      {changes && comparingConflict && (
        <section className="editor-conflict-comparison" aria-label="Disk and draft comparison">
          <p>Changed words are highlighted in the disk version and your draft.</p>
          <div className="editor-conflict-comparison__grid">
            <ConflictComparisonColumn title="Disk version" changes={changes} showDraft={false} />
            <ConflictComparisonColumn title="Your draft" changes={changes} showDraft />
          </div>
        </section>
      )}
    </section>
  );
}

function ConflictComparisonColumn({
  title,
  changes,
  showDraft,
}: {
  title: string;
  changes: ReturnType<typeof diffWordsWithSpace>;
  showDraft: boolean;
}) {
  return (
    <section className={showDraft ? "editor-conflict-comparison__column editor-conflict-comparison__column--draft" : "editor-conflict-comparison__column"}>
      <h3>{title}</h3>
      <pre>
        {changes.map((change, index) => {
          if ((showDraft && change.removed) || (!showDraft && change.added)) {
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