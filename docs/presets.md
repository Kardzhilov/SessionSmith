# Game System Presets

Presets teach the LLM what matters for your game. They inject terminology,
capture priorities, and structural expectations into every prompt — so the
model knows to track spell slots in D&D, stress in Blades, or sanity in
Call of Cthulhu without you writing custom prompts.

---

## Bundled presets

| ID | System | Focus |
|---|---|---|
| `dnd5e` | D&D 5th Edition | Combat, spell slots, loot, XP/milestones, advantage/disadvantage |
| `pf2e` | Pathfinder 2e | Three-action economy, hero points, conditions, exploration mode |
| `coc` | Call of Cthulhu | Sanity, luck, clue tracking, mythos vs. mundane |
| `blades` | Blades in the Dark | Position/effect, stress, clocks, entanglements, downtime |
| `daggerheart` | Daggerheart | Hope/Fear, armor slots, community bonds |
| `generic` | Any system | Universal capture: combats, NPCs, loot, decisions, cliffhangers |
| `wordsmith` | Wordsmith | Narrative dice, word-based advantages, fictional consequences |

---

## Preset anatomy

A preset is a TOML file with these fields:

```toml
name = "D&D 5e"
description = "Dungeons & Dragons 5th Edition."

terminology = """
Use D&D 5e terms: HP, AC, spell slots, advantage/disadvantage, short rest,
long rest, inspiration, exhaustion levels, concentration. Class features
and spells should be named explicitly when they meaningfully change the scene.
"""

capture = """
- Combat: initiative order, big hits, crits, downed PCs, monsters defeated
- Spells cast and concentration broken
- Loot: magic items, gold, consumables (note attunement)
- XP awarded or milestone advancements
- Resources spent: spell slots of consequence, Hit Dice, class features
- Quest hooks and faction reputation changes
"""

extra_sections = """
### Loot & Treasure
Itemise everything acquired: magic items (with attunement state), gold,
gems, and consumables.

### Combat Notes
Notable rolls, downed characters, and any rulings made at the table.
"""

forbidden_phrases = [
  "shadowy", "tapestry", "whisper", "realm", "delve",
  "testament", "threads", "myriad", "pivotal", "vibrant", "bustling"
]
```

### Field reference

| Field | Required | Purpose |
|---|---|---|
| `name` | yes | Human-readable name shown in UI |
| `description` | yes | Short description |
| `terminology` | yes | Injected into prompts to teach the LLM your system's vocabulary |
| `capture` | yes | Bullet list of what to watch for — the LLM prioritises these during extraction |
| `extra_sections` | no | Additional markdown sections appended to the dm-notes artifact |
| `forbidden_phrases` | no | Words/phrases the LLM is instructed to never use (avoids cliché AI prose) |

---

## Creating a custom preset

1. Create a new file in `presets/`:

```bash
cp presets/generic.toml presets/my-system.toml
```

2. Edit the fields to match your game system's terminology and priorities.

3. Register it in `src/presets.rs` — add the filename to the `BUNDLED`
   array so it gets embedded in the binary at compile time:

```rust
const BUNDLED: &[(&str, &str)] = &[
    ("dnd5e", include_str!("../presets/dnd5e.toml")),
    ("pf2e", include_str!("../presets/pf2e.toml")),
    // ... existing entries ...
    ("my-system", include_str!("../presets/my-system.toml")),
];
```

4. Rebuild:

```bash
cargo build --release
```

5. Reference it in your campaign config:

```toml
[system]
preset = "my-system"
```

---

## Using overrides instead

If you don't want to create a full preset, use the `overrides` field in
your campaign config. This text is appended after the preset's terminology
and capture blocks:

```toml
[system]
preset    = "generic"
overrides = """
We use a homebrew stress mechanic. Track stress gained/spent per character.
Our magic system uses "resonance points" — note when they're spent or recovered.
Faction reputation is tracked on a -5 to +5 scale.
"""
```

This is the fastest way to customise without touching source code.

---

## How presets affect output

The preset influences every artifact differently:

| Artifact | How the preset is used |
|---|---|
| **bullets** | `capture` priorities determine what gets included in the outline |
| **dm-notes** | `extra_sections` are appended as additional headings; `terminology` ensures correct naming |
| **recap** | `terminology` keeps the recap in-voice for the system |
| **summary** | `capture` priorities determine hierarchy |
| **story** | `forbidden_phrases` prevent cliché prose; `terminology` keeps it authentic |
| **quotes** | Minimal preset influence — captures raw dialogue |
| **campaign log** | `capture` determines what's tracked across sessions |

---

## Tips for good presets

- **Be specific in `terminology`.** Don't just list terms — explain when
  they matter. "Note spell slots *of consequence*" is better than "track
  all spell slots."

- **Prioritise in `capture`.** Put the most important items first. The LLM
  treats earlier items as higher priority.

- **Keep `forbidden_phrases` short.** 10–15 words that your group finds
  cringeworthy. The LLM replaces them with more natural alternatives.

- **Use `extra_sections` for structure.** If your game has a specific
  bookkeeping need (clock trackers, downtime actions, faction standings),
  define it as a section header with brief instructions.

---

## Browsing presets

```bash
# List all available presets
sessionsmith systems list

# Show a preset's full content
sessionsmith systems show dnd5e
```
