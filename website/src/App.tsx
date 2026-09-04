import { useEffect, useState } from 'react'
import {
  ArrowDown,
  ArrowRight,
  AudioLines,
  BookOpenText,
  Check,
  ChevronRight,
  CircleDot,
  Cloud,
  Command,
  Download,
  ExternalLink,
  FileAudio,
  FileText,
  GitFork,
  HardDrive,
  Library,
  LockKeyhole,
  Search,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
  Users,
} from 'lucide-react'
import ScrollScene from './ScrollScene'
import './App.css'

type Platform = 'linux' | 'macos' | 'windows'

const version = '1.2.0'
const releaseUrl = 'https://github.com/Kardzhilov/SessionSmith/releases/latest'
const repositoryUrl = 'https://github.com/Kardzhilov/SessionSmith'
const docsUrl = `${repositoryUrl}/tree/main/docs`
const setupUrl = `${repositoryUrl}/blob/main/docs/setup.md`
const asset = (name: string) => `${import.meta.env.BASE_URL}assets/${name}`
const releaseAsset = (name: string) => `${repositoryUrl}/releases/download/v${version}/${name}`

const installOptions: Record<Platform, {
  label: string
  eyebrow: string
  note: string
  downloads: Array<{ label: string; detail: string; href: string }>
}> = {
  linux: {
    label: 'Linux',
    eyebrow: 'Linux · x86_64',
    note: 'AppImage runs without installation. Debian/Ubuntu and RPM packages integrate with your desktop environment.',
    downloads: [
      { label: 'Download AppImage', detail: 'Universal · 91 MB', href: releaseAsset(`SessionSmith_${version}_amd64.AppImage`) },
      { label: 'Download DEB', detail: 'Debian / Ubuntu · 19 MB', href: releaseAsset(`SessionSmith_${version}_amd64.deb`) },
      { label: 'Download RPM', detail: 'Fedora / RHEL · 19 MB', href: releaseAsset(`SessionSmith-${version}-1.x86_64.rpm`) },
    ],
  },
  macos: {
    label: 'macOS',
    eyebrow: 'macOS · Apple silicon',
    note: 'Open the DMG, drag SessionSmith into Applications, then complete the guided local model setup.',
    downloads: [
      { label: 'Download DMG', detail: 'Apple silicon · 16 MB', href: releaseAsset(`SessionSmith_${version}_aarch64.dmg`) },
    ],
  },
  windows: {
    label: 'Windows',
    eyebrow: 'Windows · x64',
    note: 'Use the guided setup executable for the simplest install, or the MSI for managed environments.',
    downloads: [
      { label: 'Download setup', detail: 'Recommended · 11 MB', href: releaseAsset(`SessionSmith_${version}_x64-setup.exe`) },
      { label: 'Download MSI', detail: 'Windows Installer · 16 MB', href: releaseAsset(`SessionSmith_${version}_x64_en-US.msi`) },
    ],
  },
}

const workflow = [
  {
    number: '01',
    icon: FileAudio,
    title: 'Bring the table audio',
    body: 'Record in the app or import the file you already have. Every campaign gets its own clean workspace.',
  },
  {
    number: '02',
    icon: AudioLines,
    title: 'Transcribe and inspect',
    body: 'Local Whisper turns speech into timestamped text. Listen back, search lines, and map speakers before notes are made.',
  },
  {
    number: '03',
    icon: Sparkles,
    title: 'Forge campaign memory',
    body: 'Focused passes produce GM notes, recaps, summaries, stories, and quotes, then update the living campaign log.',
  },
]

const capabilities = [
  { icon: Library, title: 'Campaign workspace', body: 'Audio, transcript, generated documents, edits, and history stay together.' },
  { icon: Search, title: 'Cross-session search', body: 'Find the name, promise, place, or loose thread hiding three sessions back.' },
  { icon: Users, title: 'Speaker mapping', body: 'Turn diarized labels into the players, characters, and voices you recognize.' },
  { icon: BookOpenText, title: 'Living campaign log', body: 'Carry decisions and open arcs forward without rereading every transcript.' },
  { icon: FileText, title: 'Reviewable notes', body: 'Compare candidate generations and edit ordinary Markdown in the app.' },
  { icon: HardDrive, title: 'Portable exports', body: 'Take the campaign with you as offline HTML or an Obsidian-ready folder.' },
]

const systems = ['D&D 5e', 'Pathfinder 2e', 'Call of Cthulhu', 'Blades in the Dark', 'Daggerheart', 'Wordsmith', 'Any RPG']

function detectPlatform(): Platform {
  const hint = `${navigator.platform} ${navigator.userAgent}`.toLowerCase()
  if (hint.includes('mac')) return 'macos'
  if (hint.includes('win')) return 'windows'
  return 'linux'
}

function App() {
  const [platform, setPlatform] = useState<Platform>('linux')
  const selectedInstall = installOptions[platform]

  useEffect(() => setPlatform(detectPlatform()), [])

  return (
    <div className="site-shell">
      <a className="skip-link" href="#main-content">Skip to content</a>
      <ScrollScene screenshotUrl={asset('desktop-workspace.png')} />

      <header className="site-header" aria-label="Primary navigation">
        <a className="brand-lockup" href="#top" aria-label="SessionSmith home">
          <img src={asset('sessionsmith-app-icon.png')} alt="" width="38" height="38" />
          <span>SessionSmith</span>
        </a>
        <nav className="nav-links" aria-label="Main navigation">
          <a href="#how-it-works">How it works</a>
          <a href="#features">Features</a>
          <a href="#privacy">Privacy</a>
          <a href="#install">Install</a>
        </nav>
        <a className="header-source" href={repositoryUrl} target="_blank" rel="noreferrer">
          <GitFork aria-hidden="true" size={18} /><span>GitHub</span>
        </a>
      </header>

      <main id="main-content">
        <section className="hero" id="top" aria-labelledby="hero-title">
          <div className="hero-copy">
            <p className="eyebrow"><span />Open source · Local first · v{version}</p>
            <h1 id="hero-title">SessionSmith</h1>
            <p className="hero-tagline">The session ends.<br />The story stays.</p>
            <p className="hero-deck">
              Turn tabletop audio into a searchable transcript, useful GM notes,
              a player-ready recap, and campaign history you can pick up next week.
            </p>
            <div className="hero-actions">
              <a className="button button--primary" href="#install">
                <Download aria-hidden="true" size={19} />Get SessionSmith
              </a>
              <a className="button button--ghost" href="#product">
                See it in action <ArrowRight aria-hidden="true" size={18} />
              </a>
            </div>
            <p className="hero-assurance"><LockKeyhole aria-hidden="true" size={15} />Your recordings stay on your machine by default.</p>
          </div>
          <a className="scroll-cue" href="#how-it-works">
            <span>Follow the signal</span><ArrowDown aria-hidden="true" size={17} />
          </a>
        </section>

        <section className="scene-journey" id="how-it-works" data-scene-journey aria-labelledby="journey-title">
          <header className="journey-intro">
            <p className="section-kicker">One visible pipeline</p>
            <h2 id="journey-title">From voices in a room<br />to a world you remember.</h2>
          </header>
          <ol className="journey-steps">
            {workflow.map(({ number, icon: Icon, title, body }) => (
              <li key={number}>
                <div className="step-index"><span>{number}</span><Icon aria-hidden="true" size={22} /></div>
                <div><h3>{title}</h3><p>{body}</p></div>
              </li>
            ))}
          </ol>
        </section>

        <section className="product-proof" id="product" aria-labelledby="product-title">
          <div className="proof-heading section-wrap">
            <div>
              <p className="section-kicker">Built for the week between games</p>
              <h2 id="product-title">Not another transcript dump.</h2>
            </div>
            <p>SessionSmith keeps the source recording, timestamped transcript, generated documents, corrections, and campaign context in one working surface.</p>
          </div>
          <figure className="app-capture">
            <img src={asset('desktop-workspace.png')} alt="SessionSmith desktop workspace showing The Bell in the Fog summary beside a speaker-labelled transcript" />
            <figcaption><span>Session workspace</span><span>Notes and evidence, side by side</span></figcaption>
          </figure>
        </section>

        <section className="feature-section" id="features" aria-labelledby="features-title">
          <div className="section-wrap">
            <div className="section-heading">
              <p className="section-kicker">Campaign continuity</p>
              <h2 id="features-title">Everything that makes<br />a recording useful.</h2>
            </div>
            <div className="feature-grid">
              {capabilities.map(({ icon: Icon, title, body }, index) => (
                <article key={title}>
                  <span className="feature-number">0{index + 1}</span>
                  <Icon aria-hidden="true" size={24} />
                  <h3>{title}</h3>
                  <p>{body}</p>
                </article>
              ))}
            </div>
          </div>
        </section>

        <section className="search-story" aria-labelledby="search-title">
          <div className="search-copy">
            <p className="section-kicker">Three months later</p>
            <h2 id="search-title">Find the thread before the players do.</h2>
            <p>Search every note and transcript, jump to the right session, and hear the moment in context. Names, promises, clues, and accidental prophecies stay within reach.</p>
            <a className="text-link" href={docsUrl}>Explore the documentation <ArrowRight aria-hidden="true" size={17} /></a>
          </div>
          <figure>
            <img src={asset('desktop-library.png')} alt="SessionSmith campaign library showing three complete sessions and an audio Inbox" />
            <figcaption>A campaign library that grows with the table.</figcaption>
          </figure>
        </section>

        <section className="privacy-section" id="privacy" aria-labelledby="privacy-title">
          <div className="privacy-icon" aria-hidden="true"><ShieldCheck size={52} strokeWidth={1.35} /></div>
          <div className="privacy-main">
            <p className="section-kicker">Local-first means local</p>
            <h2 id="privacy-title">Your campaign is not training data.</h2>
            <p>Built-in Whisper can transcribe on your machine. Ollama can generate every document locally. Your workspace remains ordinary audio, Markdown, JSON, TOML, and a local search index.</p>
          </div>
          <div className="privacy-choice">
            <Cloud aria-hidden="true" size={22} />
            <h3>Cloud when you choose it</h3>
            <p>OpenAI, Anthropic, OpenRouter, Groq, LM Studio, vLLM, and compatible endpoints are explicit options, never invisible defaults.</p>
          </div>
        </section>

        <section className="systems-section" aria-labelledby="systems-title">
          <div className="systems-heading">
            <p className="section-kicker">System-aware notes</p>
            <h2 id="systems-title">It speaks your game.</h2>
          </div>
          <ul>{systems.map((system) => <li key={system}><CircleDot aria-hidden="true" size={13} />{system}</li>)}</ul>
        </section>

        <section className="install-section" id="install" aria-labelledby="install-title">
          <div className="install-shell section-wrap">
            <div className="install-intro">
              <p className="section-kicker">Desktop release · v{version}</p>
              <h2 id="install-title">Ready before<br />the next session.</h2>
              <p>Choose your platform, install ffmpeg, then let the first-run guide check your machine and set up local or hosted models.</p>
              <a className="text-link text-link--dark" href={releaseUrl}>See every release asset <ExternalLink aria-hidden="true" size={16} /></a>
            </div>

            <div className="installer">
              <div className="platform-tabs" role="tablist" aria-label="Operating system">
                {(Object.keys(installOptions) as Platform[]).map((id) => (
                  <button
                    key={id}
                    type="button"
                    role="tab"
                    aria-selected={platform === id}
                    className={platform === id ? 'platform-tab platform-tab--active' : 'platform-tab'}
                    onClick={() => setPlatform(id)}
                  >
                    {installOptions[id].label}
                  </button>
                ))}
              </div>
              <div className="platform-panel" role="tabpanel">
                <p className="platform-eyebrow">{selectedInstall.eyebrow}</p>
                <p className="platform-note">{selectedInstall.note}</p>
                <div className="download-list">
                  {selectedInstall.downloads.map((download, index) => (
                    <a className={index === 0 ? 'download-row download-row--primary' : 'download-row'} href={download.href} key={download.label}>
                      <Download aria-hidden="true" size={20} />
                      <span><strong>{download.label}</strong><small>{download.detail}</small></span>
                      <ChevronRight aria-hidden="true" size={18} />
                    </a>
                  ))}
                </div>
                <div className="install-requirements">
                  <span><Check aria-hidden="true" size={15} />Built-in local Whisper</span>
                  <span><Check aria-hidden="true" size={15} />Signed updates</span>
                  <span><Check aria-hidden="true" size={15} />MIT licensed</span>
                </div>
              </div>
            </div>
          </div>

          <div className="source-install section-wrap">
            <div>
              <TerminalSquare aria-hidden="true" size={25} />
              <span><strong>Prefer to build it yourself?</strong><small>Rust, Node.js, npm, CMake, Clang, and platform webview dependencies required.</small></span>
            </div>
            <code>git clone https://github.com/Kardzhilov/SessionSmith.git<br />cd SessionSmith &amp;&amp; make app</code>
            <a href={setupUrl}>Full setup guide <ArrowRight aria-hidden="true" size={16} /></a>
          </div>
        </section>

        <section className="closing-section" aria-labelledby="closing-title">
          <div className="closing-mark" aria-hidden="true"><AudioLines size={48} strokeWidth={1.3} /></div>
          <p className="section-kicker">The table made a story</p>
          <h2 id="closing-title">Keep it alive.</h2>
          <div className="closing-actions">
            <a className="button button--primary" href="#install"><Download aria-hidden="true" size={18} />Download v{version}</a>
            <a className="button button--ghost" href={repositoryUrl}><GitFork aria-hidden="true" size={18} />View on GitHub</a>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <a className="footer-brand" href="#top"><img src={asset('sessionsmith-app-icon.png')} alt="" width="34" height="34" /><span>SessionSmith</span></a>
        <nav aria-label="Footer navigation">
          <a href={setupUrl}>Setup</a><a href={docsUrl}>Docs</a><a href={repositoryUrl}>Source</a><a href={`${repositoryUrl}/issues`}>Issues</a>
        </nav>
        <p><Command aria-hidden="true" size={14} />Built in the open · MIT License</p>
      </footer>
    </div>
  )
}

export default App
