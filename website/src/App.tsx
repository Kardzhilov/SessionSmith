import {
  ArrowRight,
  AudioLines,
  BookOpen,
  Check,
  ChevronRight,
  CircleDot,
  Cloud,
  Cpu,
  Download,
  ExternalLink,
  FileAudio,
  FileText,
  GitFork,
  HardDrive,
  Library,
  LockKeyhole,
  Search,
  Settings2,
  ShieldCheck,
  SquareTerminal,
  Users,
} from 'lucide-react'
import './App.css'

const releaseUrl = 'https://github.com/Kardzhilov/SessionSmith/releases/latest'
const repositoryUrl = 'https://github.com/Kardzhilov/SessionSmith'
const docsUrl = `${repositoryUrl}/tree/main/docs`
const setupUrl = `${repositoryUrl}/blob/main/docs/setup.md`
const configurationUrl = `${repositoryUrl}/blob/main/docs/configuration.md`
const asset = (name: string) => `${import.meta.env.BASE_URL}assets/${name}`

const workflow = [
  {
    number: '01',
    icon: FileAudio,
    title: 'Bring the table audio',
    body: 'Record from the desktop app or import an existing session. SessionSmith keeps each campaign and its Inbox separate.',
  },
  {
    number: '02',
    icon: AudioLines,
    title: 'Transcribe and review',
    body: 'Run local Whisper or an advanced ASR model, inspect timestamped lines, and map detected speakers to your players and NPCs.',
  },
  {
    number: '03',
    icon: FileText,
    title: 'Forge useful notes',
    body: 'Generate bullets, GM notes, recaps, summaries, stories, and quotes, then roll each session into a living campaign log.',
  },
]

const capabilities = [
  { icon: Library, title: 'Campaign workspace', body: 'Sessions, source audio, transcripts, notes, and the campaign log stay organized under one campaign.' },
  { icon: Cpu, title: 'Model catalog', body: 'Browse, filter, install, select, and remove supported local transcription and Ollama models.' },
  { icon: Search, title: 'Search across sessions', body: 'Find a name, place, quote, or loose thread across notes and transcripts, with filters and direct jumps.' },
  { icon: Users, title: 'Speaker mapping', body: 'Review diarized labels beside transcript samples and consistently map them to campaign participants.' },
  { icon: BookOpen, title: 'Living campaign log', body: 'Rebuild a rolling, arc-aware campaign record from the summaries you choose to keep.' },
  { icon: HardDrive, title: 'Portable exports', body: 'Export campaign material as offline HTML or an Obsidian-ready folder for use away from the app.' },
]

const presets = ['D&D 5e', 'Pathfinder 2e', 'Call of Cthulhu', 'Blades in the Dark', 'Daggerheart', 'Wordsmith', 'Generic RPG']

function App() {
  return (
    <div className="site-shell">
      <a className="skip-link" href="#main-content">Skip to content</a>

      <header className="site-header" aria-label="Primary navigation">
        <a className="brand-lockup" href="#top" aria-label="SessionSmith home">
          <img src={asset('sessionsmith-icon.png')} alt="" width="36" height="36" />
          <span>SessionSmith</span>
        </a>
        <nav className="nav-links" aria-label="Main navigation">
          <a href="#workflow">Workflow</a>
          <a href="#features">Features</a>
          <a href="#local-first">Privacy</a>
          <a href={docsUrl}>Docs</a>
        </nav>
        <a className="header-github" href={repositoryUrl} target="_blank" rel="noreferrer">
          <GitFork aria-hidden="true" size={19} /><span>GitHub</span>
        </a>
      </header>

      <main id="main-content">
        <section className="hero" id="top" aria-labelledby="hero-title">
          <img className="hero-background" src={asset('dashboard.svg')} alt="" aria-hidden="true" />
          <div className="hero-shade" aria-hidden="true" />
          <div className="hero-content">
            <p className="eyebrow"><span className="status-dot" aria-hidden="true" />Desktop release v1.0.0</p>
            <h1 id="hero-title">SessionSmith</h1>
            <p className="hero-deck">
              Turn a night of table audio into a searchable transcript, useful session notes,
              and a campaign history you can actually pick up next week.
            </p>
            <div className="hero-actions" aria-label="Download and source links">
              <a className="button button-primary" href={releaseUrl}><Download aria-hidden="true" size={20} />Download v1.0.0</a>
              <a className="button button-quiet" href={repositoryUrl} target="_blank" rel="noreferrer"><GitFork aria-hidden="true" size={20} />View source</a>
            </div>
            <p className="hero-note"><LockKeyhole aria-hidden="true" size={16} />Local-first by default. Cloud backends are always an explicit choice.</p>
          </div>
          <div className="hero-index" aria-hidden="true">
            <span>Audio</span><ChevronRight size={14} /><span>Transcript</span><ChevronRight size={14} /><span>Notes</span>
          </div>
        </section>

        <section className="opening-statement section-pad" aria-labelledby="opening-title">
          <p className="section-label">The working loop</p>
          <h2 id="opening-title">Your session ends. The story stays usable.</h2>
          <p>
            SessionSmith is a desktop workbench for tabletop campaign continuity. It keeps the
            source recording, transcript, generated artifacts, corrections, and campaign context
            together, so prep starts from what really happened at the table.
          </p>
        </section>

        <section className="workflow section-pad" id="workflow" aria-labelledby="workflow-title">
          <div className="section-heading">
            <div><p className="section-label">Audio to campaign memory</p><h2 id="workflow-title">A visible pipeline, not a black box.</h2></div>
            <p>Run the whole flow or stop after transcription. Every stage stays reviewable, and candidate notes can be compared before replacing current work.</p>
          </div>
          <ol className="workflow-list">
            {workflow.map(({ number, icon: Icon, title, body }) => (
              <li key={title}>
                <span className="step-number">{number}</span><Icon className="step-icon" aria-hidden="true" size={25} />
                <h3>{title}</h3><p>{body}</p>
              </li>
            ))}
          </ol>
          <figure className="wide-capture">
            <img src={asset('pipeline.svg')} alt="SessionSmith pipeline view showing transcription and notes jobs progressing in the application" />
            <figcaption><span>Live pipeline</span>Progress, phases, logs, and cancellation stay visible in the desktop Jobs Center.</figcaption>
          </figure>
        </section>

        <section className="capabilities section-pad" id="features" aria-labelledby="features-title">
          <div className="section-heading compact-heading"><div><p className="section-label">Built around the campaign</p><h2 id="features-title">The parts that make recordings useful.</h2></div></div>
          <div className="capability-grid">
            {capabilities.map(({ icon: Icon, title, body }) => (
              <article key={title}><Icon aria-hidden="true" size={23} /><h3>{title}</h3><p>{body}</p></article>
            ))}
          </div>
        </section>

        <section className="workspace-showcase section-pad" aria-labelledby="workspace-title">
          <div className="showcase-copy">
            <p className="section-label">Fast in a long campaign</p>
            <h2 id="workspace-title">Find the thread before the players do.</h2>
            <p>Search across every transcript and note, jump back to the exact artifact, then work with keyboard-first commands or the mouse. Themes can match the room without changing your campaign data.</p>
            <a className="text-link" href={configurationUrl}>Explore configuration <ArrowRight aria-hidden="true" size={17} /></a>
          </div>
          <div className="capture-pair">
            <figure><img src={asset('palette.svg')} alt="SessionSmith command palette listing searchable desktop actions" /><figcaption>Command palette</figcaption></figure>
            <figure><img src={asset('themes.svg')} alt="SessionSmith theme picker showing interface theme choices" /><figcaption>Theme picker</figcaption></figure>
          </div>
        </section>

        <section className="local-first section-pad" id="local-first" aria-labelledby="privacy-title">
          <div className="privacy-mark" aria-hidden="true"><ShieldCheck size={44} strokeWidth={1.5} /></div>
          <div className="privacy-copy">
            <p className="section-label">Local-first architecture</p><h2 id="privacy-title">Your campaign files live on your machine.</h2>
            <p>Local Whisper handles speech recognition and Ollama can handle note generation without sending recordings or notes to a hosted service. Campaigns are isolated on disk, and exports are written to a host-managed local destination.</p>
          </div>
          <div className="privacy-caveat">
            <Cloud aria-hidden="true" size={22} /><h3>Cloud is optional, not invisible</h3>
            <p>If you configure OpenAI, Anthropic, OpenRouter, Groq, or another OpenAI-compatible backend, the text required for that request leaves your machine and is subject to that provider&apos;s privacy and retention policies.</p>
          </div>
        </section>

        <section className="presets section-pad" aria-labelledby="presets-title">
          <div>
            <p className="section-label">System-aware output</p><h2 id="presets-title">Notes that speak your game.</h2>
            <p>Presets give the notes pipeline the terminology and structure that matter to each system. Start bundled, then customize the TOML when your table needs its own vocabulary.</p>
          </div>
          <ul aria-label="Bundled game system presets">{presets.map((preset) => <li key={preset}><CircleDot aria-hidden="true" size={14} />{preset}</li>)}</ul>
        </section>

        <section className="requirements section-pad" id="requirements" aria-labelledby="requirements-title">
          <div className="requirements-intro">
            <p className="section-label">Before the first session</p><h2 id="requirements-title">Bring a machine and an LLM backend.</h2>
            <p>Current releases support the full local workflow on Linux and macOS. Windows has portable build coverage, while local audio tooling still needs validation on the target machine.</p>
          </div>
          <ul className="requirements-list">
            <li><Check aria-hidden="true" size={18} /><span><strong>ffmpeg + ffprobe</strong> for audio decoding and inspection</span></li>
            <li><Check aria-hidden="true" size={18} /><span><strong>Ollama</strong> for fully local notes, or a configured external backend</span></li>
            <li><Check aria-hidden="true" size={18} /><span><strong>Whisper is built in</strong>; WhisperX is only needed for speaker diarization</span></li>
            <li><Check aria-hidden="true" size={18} /><span><strong>GPU acceleration is optional</strong> through CUDA, Vulkan, or Metal builds</span></li>
          </ul>
          <div className="requirements-links">
            <a href={setupUrl}><BookOpen aria-hidden="true" size={18} /> Setup guide</a>
            <a href={configurationUrl}><Settings2 aria-hidden="true" size={18} /> Configuration</a>
          </div>
        </section>

        <section className="download-section section-pad" id="download" aria-labelledby="download-title">
          <div className="download-heading">
            <img src={asset('sessionsmith-icon.png')} alt="" width="72" height="72" />
            <div><p className="section-label">Latest release · v1.0.0</p><h2 id="download-title">Put the next session to work.</h2><p>Choose your platform on GitHub Releases. Downloads are never selected automatically.</p></div>
          </div>
          <div className="platform-links" aria-label="Platform download links">
            <a href={releaseUrl}><Download aria-hidden="true" size={19} /><span>Linux <small>release files</small></span></a>
            <a href={releaseUrl}><Download aria-hidden="true" size={19} /><span>macOS <small>release files</small></span></a>
            <a href={releaseUrl}><Download aria-hidden="true" size={19} /><span>Windows <small>release files</small></span></a>
          </div>
          <p className="download-footnote">Building from source? Follow the setup guide for the Rust, CMake, Clang, and optional GPU requirements.</p>
        </section>
      </main>

      <footer className="site-footer">
        <div className="footer-brand"><img src={asset('sessionsmith-icon.png')} alt="" width="34" height="34" /><span>SessionSmith</span></div>
        <nav aria-label="Footer navigation">
          <a href={setupUrl}>Setup</a><a href={configurationUrl}>Configuration</a><a href={docsUrl}>Docs</a>
          <a href={repositoryUrl} target="_blank" rel="noreferrer">GitHub <ExternalLink aria-hidden="true" size={13} /></a>
        </nav>
        <p><SquareTerminal aria-hidden="true" size={15} /> Open source under the MIT License · 2026 SessionSmith contributors</p>
      </footer>
    </div>
  )
}

export default App
