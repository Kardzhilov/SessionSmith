# TUI Improvement Report — Learning from Posting

> How to evolve SessionSmith's terminal UX toward the polish of
> [Posting](https://github.com/darrenburns/posting) (Darren Burns) — a
> mouse-driven, resizable, themeable terminal HTTP client built on
> [Textual](https://github.com/textualize/textual).
>
> Posting and SessionSmith are different apps, so this is about matching the
> *general qualities* (clickable, responsive, themed, discoverable), not the
> feature set.

---

## 1. The honest starting point

SessionSmith today is **not a TUI** in the Posting sense. It is a *line-oriented
interactive CLI*:

- Interaction is a sequence of [`inquire`](https://docs.rs/inquire) prompts —
  `Select`, `MultiSelect`, `Confirm`, `Text` — in
  [src/commands/home.rs](src/commands/home.rs), [src/commands/init.rs](src/commands/init.rs),
  [src/commands/models.rs](src/commands/models.rs), [src/commands/notes.rs](src/commands/notes.rs),
  [src/commands/transcribe.rs](src/commands/transcribe.rs), [src/session.rs](src/session.rs),
  and the campaign picker in [src/commands/mod.rs](src/commands/mod.rs).
- Output is printed and scrolls away: colored status lines, bordered panels,
  spinners (`indicatif`) and tables (`comfy-table`) in [src/ui.rs](src/ui.rs).

That means:

| Quality | Posting | SessionSmith today |
|---|---|---|
| Full-screen persistent layout | ✅ (Textual screens/regions) | ❌ prints then scrolls |
| Mouse / clickable | ✅ | ❌ keyboard-only prompts |
| Resizes & reflows live | ✅ (CSS layout re-solves) | ⚠️ "works" only because it just prints lines; no layout to reflow |
| Themes (built-in + user) | ✅ | ❌ hardcoded `owo-colors` |
| Command palette | ✅ | ❌ |
| Configurable keymaps | ✅ | ❌ |
| Contextual help (F1) | ✅ | ❌ (`--help` text only) |
| Live progress in-place | ✅ | ⚠️ spinners, but they block and vanish |

So "improving the TUI to match Posting" really means **introducing a real TUI
layer** — which in Rust means adopting an immediate-mode rendering stack and an
event loop, then porting the interactive flows into it.

---

## 2. How Posting achieves its polish

Posting inherits almost everything from **Textual**, a retained-mode,
CSS-styled, reactive TUI framework. The transferable ideas:

1. **A declarative layout engine.** Textual lays widgets out with a CSS-like
   language (TCSS): docking, fractional sizing, min/max, grids. When the
   terminal resizes, the layout is re-solved and everything reflows. This is the
   root of "scales up and down and is resizable".
2. **Widgets as first-class, focusable, clickable objects.** Every list item,
   tab, button and input is a widget with mouse hit-testing and focus. Clicking
   or `Tab`-cycling just works.
3. **An event loop + message passing.** Input events (key, mouse, resize) and
   app messages flow through a single loop; long work runs in workers and posts
   messages back, so the UI never freezes.
4. **A theme system.** Named color roles (primary/secondary/accent/…) resolved
   at runtime; built-in themes + user themes loaded from files, switchable live.
5. **A command palette.** A fuzzy-searchable overlay to reach any action —
   "discoverability without menus".
6. **Configurable keymaps + contextual help.** Keybindings are data; a footer
   shows the active bindings; `F1` explains the focused widget.
7. **Consistent chrome.** Header, footer/keybar, bordered titled panels,
   syntax highlighting — a coherent visual language.

None of these require Textual specifically; they're patterns any TUI can adopt.

---

## 3. The Rust equivalent stack

There is no 1:1 Textual in Rust, but the mature, idiomatic stack is:

| Concern | Recommended crate | Notes |
|---|---|---|
| Rendering / layout / widgets | **`ratatui`** | The standard Rust TUI lib. Immediate-mode: you redraw each frame into a size-aware buffer, so **resize/reflow is automatic**. Ships `List`, `Table`, `Tabs`, `Paragraph`, `Gauge`, `Scrollbar`, `Block` (titled borders), `Layout` (constraint solver). |
| Terminal backend + input | **`crossterm`** | Raw mode, alternate screen, **mouse capture**, resize & key/mouse events. Already an indirect dep via other crates. |
| Fuzzy matching (palette, pickers) | **`nucleo`** (or `fuzzy-matcher`) | Fast fuzzy matcher (used by Helix/Television) for the command palette and file/model pickers. |
| Text input widgets | **`tui-input`** / **`tui-textarea`** | Single-line and multi-line editing inside the TUI (replaces `inquire::Text`; `tui-textarea` is great for the `[prompts]` overrides). |
| In-TUI spinner | **`throbber-widgets-tui`** (or hand-rolled) | `indicatif` can't render inside a ratatui frame; progress must become app state drawn each frame. |

Key architectural consequence: **`inquire` and a full-screen `ratatui` app are
mutually exclusive.** `inquire` owns the terminal in cooked/line mode; ratatui
runs in raw mode on the alternate screen. Every interactive prompt must be
reimplemented as an in-TUI widget (a focusable list, a text field, a confirm
modal). This is the main migration cost — and the main payoff (that's what makes
it clickable and cohesive).

---

## 4. Proposed architecture

A single full-screen "app" mode, with the existing non-interactive subcommands
(`run --all`, `transcribe <file>`, `notes`, `search`, …) untouched for
scripting/CI.

```mermaid
flowchart TD
    subgraph UI thread (ratatui + crossterm)
        EL[Event loop] -->|key/mouse/resize| ST[App state]
        ST --> DR[draw&#40;frame&#41;]
        DR --> EL
    end
    subgraph Work (tokio tasks)
        W[pipeline / transcribe / llm]
    end
    EL -- start job --> W
    W -- progress/log/done events --> CH[(mpsc channel)]
    CH --> ST
```

- **`App` state struct**: current screen/route, selected campaign, session list,
  focus target, scroll offsets, theme, in-flight job progress, palette state.
- **`draw(frame, &App)`**: pure function that lays out the frame from the
  current terminal size every tick → inherently responsive/resizable.
- **Event loop**: `crossterm` events (`Key`, `Mouse`, `Resize`) + a `tokio::mpsc`
  receiver for worker events (ASR device chosen, streaming token counts, "wrote
  bullets.md", errors). Because work runs in tasks and reports via the channel,
  the UI stays live and progress renders in-place instead of blocking spinners.
- **Reuse the library**: the TUI is just another front-end over
  `sessionsmith::{pipeline, transcribe, config, index, presets, …}`. No business
  logic moves into the UI.

Screen sketch (a real layout that reflows on resize):

```
┌ SessionSmith ───────────────────────────────── qwen2.5:32b · large-v3 ┐
│ Campaigns          │ MyGame — session "start1_combined"                │
│ ▸ MyGame           │ ┌ Artifacts ───────────────────────────────────┐ │
│   DnDThursday      │ │ bullets  dm-notes  recap  summary  story …   │ │
│   TestGame         │ └───────────────────────────────────────────────┘ │
│                    │ ┌ Preview: recap.md ───────────────────── ▲ ─┐ │
│ Audio              │ │ The party descends into the crypt…          │ │
│ ▸ start1.flac  12m │ │ …                                        ▼  │ │
│   Wordsmith2 08m   │ └──────────────────────────────────────────────┘ │
├────────────────────┴───────────────────────────────────────────────────┤
│ ↑↓ move  ⏎ open  r run  t transcribe  / search  : palette  ? help  q quit│
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 5. Mapping Posting's qualities → SessionSmith features

### 🔴 Foundations (unlock everything else)

1. **Adopt `ratatui` + `crossterm` with a proper terminal lifecycle.**
   Enter alternate screen + raw mode + mouse capture on start; restore on exit
   **and on panic** (install a panic hook that restores the terminal, or the
   user's shell is left broken). This is the single most important correctness
   detail. Reuse the existing `Ctrl-C`/ASR-kill handling from
   [src/main.rs](src/main.rs).

2. **Full-screen home dashboard** replacing the `inquire` menu in
   [src/commands/home.rs](src/commands/home.rs): a sidebar (campaigns + audio),
   a main content pane, a header (campaign/model summary — reuse the data from
   `ui::panel`) and a footer keybar. Immediate-mode redraw = free resize/reflow.

### 🟠 The "Posting feel"

3. **Mouse support.** Enable `crossterm` mouse capture; on `Mouse` click events,
   hit-test the click against the `Rect`s you laid out (ratatui gives you the
   areas) to select list rows, switch tabs, hit buttons. This is what makes it
   "clickable".

4. **Theme system.** Define a `Theme` of named roles → `ratatui::style::Style`
   (primary, accent, success, warn, error, muted, border, selection). Ship a few
   built-ins and load user themes from `~/.config/sessionsmith/themes/*.toml`,
   selectable at runtime and via `[ui] theme` in config. Replaces the hardcoded
   `owo-colors` calls in [src/ui.rs](src/ui.rs).

5. **Command palette.** A modal overlay with a `nucleo` fuzzy filter over all
   actions ("Run pipeline", "Transcribe…", "Search notes", "Switch campaign",
   "Change theme", "Rebuild log", "Pull model…"). Bind to `:` or `Ctrl-P`.
   Instant discoverability without deep menus.

6. **Footer keybar + configurable keymaps.** Always-visible contextual bindings
   (like Posting/Textual). Store bindings as data in config
   (`[keymap]`), so users can remap. A `?`/`F1` overlay lists bindings for the
   focused pane (contextual help).

### 🟡 Content & flow

7. **Live pipeline view.** Turn the blocking `indicatif` spinners
   ([src/ui.rs](src/ui.rs), [src/pipeline.rs](src/pipeline.rs),
   [src/transcribe.rs](src/transcribe.rs)) into a streamed progress panel: ASR
   device + elapsed, then per-artifact rows (`bullets ✓`, `recap ⠹ ~1.2k tokens`)
   fed by an `mpsc` channel from the worker tasks. The LLM layer already streams
   token counts (`llm::collect`) — pipe those events to the UI instead of a
   spinner message.

8. **Scrollable artifact viewer.** A `Paragraph` + `Scrollbar` pane to read
   `bullets.md`/`recap.md`/etc. in-app (with `PageUp/Down`, mouse wheel), plus an
   "open in `$EDITOR`/`$PAGER`" action — a signature Posting convenience.

9. **In-TUI pickers replace `inquire`.** Port the audio multi-select
   ([src/commands/transcribe.rs](src/commands/transcribe.rs)), campaign picker,
   model/backend pickers ([src/commands/models.rs](src/commands/models.rs)),
   artifact multi-select ([src/commands/notes.rs](src/commands/notes.rs)) and the
   `init` wizard ([src/commands/init.rs](src/commands/init.rs)) into focusable
   list/checkbox/text widgets. Use `tui-textarea` for the multi-line prompt
   overrides.

10. **Search UX.** The new SQLite `search` ([src/index.rs](src/index.rs)) becomes
    a live-filtering pane (type-to-filter with `nucleo`, results update per
    keystroke, `⏎` opens the artifact in the viewer).

### 🟢 Nice-to-have

11. **Compact mode** (Posting-style) — a density toggle for small terminals.
12. **Syntax/markdown highlighting** in the artifact viewer (e.g. a lightweight
    markdown-to-`ratatui::text::Text` renderer) for prettier notes.
13. **Minimum-size guard** — if the terminal is too small, show a friendly
    "resize me" screen instead of a broken layout (Textual does this).

---

## 6. Migration strategy (incremental, low-risk)

Keep every non-interactive subcommand working throughout; the TUI is additive.

1. **Phase 0 — plumbing.** Add `ratatui`, `crossterm`, terminal setup/teardown
   with a panic-safe restore, and an empty full-screen app that draws a header +
   footer and quits on `q`. Wire it as the no-subcommand entry (replacing the
   `inquire` home menu), behind a `--tui`/`--no-tui` flag while it matures.
2. **Phase 1 — dashboard + navigation + mouse + theme.** Sidebar/content/footer
   layout, keyboard + mouse selection, one built-in theme via a `Theme` struct.
3. **Phase 2 — actions & live jobs.** Trigger `run`/`transcribe`/`notes` from the
   UI, streaming worker events into a live progress pane (retire blocking
   spinners in interactive mode). Add the scrollable artifact viewer.
4. **Phase 3 — in-TUI pickers & wizard.** Replace the remaining `inquire` flows
   (init wizard, model/backend config, audio ordering) with native widgets.
5. **Phase 4 — palette, keymaps, themes-from-config, contextual help.** The final
   layer of Posting-grade polish.

Guiding principles:
- **Immediate-mode is your friend for resize** — draw from the live size each
  frame and constraints reflow automatically; don't cache pixel positions.
- **Never block the UI thread** — all ASR/LLM/HTTP work stays in `tokio` tasks
  reporting via channels (the codebase is already async).
- **Restore the terminal on every exit path** — normal, error, `Ctrl-C`, panic.
- **One source of truth** — the TUI calls the same `sessionsmith::*` library
  functions the CLI does; no logic duplication.

---

## 7. Effort & payoff

The big cost is item 9 (re-creating the `inquire` prompts as widgets) and the
event-loop discipline for async progress (item 7). Everything else is
incremental. But this is exactly the work that converts SessionSmith from a
"nice interactive CLI" into a Posting-class TUI: **clickable, resizable, themed,
and discoverable**, while preserving the scriptable subcommands that make it
useful in pipelines and CI.

Suggested first PR: **Phase 0 + Phase 1** — a mouse-clickable, themed, resizable
home dashboard behind `--tui`. That alone delivers most of the perceived polish
and establishes the architecture the rest builds on.
