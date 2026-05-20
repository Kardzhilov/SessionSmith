//! Prompt construction. Pure functions; system + preset are injected
//! around base templates so the engine itself stays game-system agnostic.

use crate::config::{CampaignConfig, PromptOverrides};
use crate::presets::Preset;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Artifact {
    Bullets,
    DmNotes,
    Recap,
    Summary,
    Story,
    Quotes,
}

impl Artifact {
    pub fn id(self) -> &'static str {
        match self {
            Self::Bullets => "bullets",
            Self::DmNotes => "dm-notes",
            Self::Recap => "recap",
            Self::Summary => "summary",
            Self::Story => "story",
            Self::Quotes => "quotes",
        }
    }
    pub fn filename(self) -> &'static str {
        match self {
            Self::Bullets => "bullets.md",
            Self::DmNotes => "dm-notes.md",
            Self::Recap => "recap.md",
            Self::Summary => "summary.md",
            Self::Story => "story.md",
            Self::Quotes => "quotes.md",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Bullets => "Bullets",
            Self::DmNotes => "DM Notes",
            Self::Recap => "Player Recap",
            Self::Summary => "Summary",
            Self::Story => "Story",
            Self::Quotes => "Quotes",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "bullets" => Some(Self::Bullets),
            "dm-notes" | "notes" => Some(Self::DmNotes),
            "recap" => Some(Self::Recap),
            "summary" => Some(Self::Summary),
            "story" => Some(Self::Story),
            "quotes" => Some(Self::Quotes),
            _ => None,
        }
    }
}

pub const ALL_ARTIFACTS: &[Artifact] = &[
    Artifact::Bullets, Artifact::DmNotes, Artifact::Recap,
    Artifact::Summary, Artifact::Story, Artifact::Quotes,
];

const BULLETS_BASE: &str = "\
You are a precise TTRPG session analyst. Extract a complete chronological \
bullet-point outline of everything that happened in the session transcript.

Rules:
- One bullet per discrete event. Keep each to a single sentence.
- Stay strictly chronological.
- Distinguish player actions (\"Sarah decides...\") from in-world character actions \
(\"Ragano sneaks toward...\"). Mark uncertain attribution with \"(?)\".
- Include memorable quotes from players or in-game characters.
- Capture: NPCs introduced or developed, locations visited, loot acquired, quests \
given/advanced/resolved, significant rolls and their outcomes, PC deaths or major \
injuries, important revelations.
- Ignore: out-of-character jokes, rules debates, scheduling, audio glitches, filler \
words, recaps of prior sessions (unless new info is revealed).
- Do NOT include any preamble. Begin with the first bullet point.";

const DM_NOTES_BASE: &str = "\
You are an expert TTRPG game master assistant. Given a bullet-point session outline, \
write structured DM notes to prepare for the next session.

Cover all that apply:
- **Where we left off** — exact party situation at session end
- **NPCs** — anyone introduced or significantly developed (name, role, relationship to party)
- **Locations** — new places visited or mentioned
- **Loot & Resources** — items, currency, information, abilities gained or lost
- **Active Quests / Threads** — current status of each; what advanced or resolved
- **Open Hooks** — unresolved threads, dangling questions, things to plan for
- **PC Consequences** — decisions made this session with future implications

Format: Markdown with headers per section. Be specific — use names and details. \
Focus on what the GM needs going forward, not retelling. No preamble.";

const RECAP_BASE: &str = "\
You are writing a short, spoiler-safe player-facing recap of the last session, \
to be read aloud (or pasted in chat) at the start of the next session.

Rules:
- 150–300 words, present tense, in-character framing where appropriate.
- ONLY include events, names, places and details that are explicitly present in the \
transcript or the bullet-point outline provided to you. Do NOT invent lore, NPCs, \
locations, plot points or dialogue that are not in the source material.
- If the session contained little or no gameplay (e.g. a test recording, setup chatter, \
or a session that ended early), say so briefly and honestly rather than fabricating content.
- Do NOT reveal GM secrets, future twists, or anything the party did not learn.
- End on the cliffhanger or the decision the party is about to make.
- No preamble, no headers — just flowing prose.";

const SUMMARY_BASE: &str = "\
You are a TTRPG note-taker. Produce a concise quick-reference summary from the \
bullet-point outline.

Format: short bullet points under thematic `##` headers (e.g. ## Exploration, \
## Combat, ## Story, ## Loot, ## Cliffhanger). Only the most important events. \
A player who missed the session should grasp what happened in under two minutes. \
No preamble.";

const STORY_BASE: &str = "\
You are a skilled fantasy author. Given a bullet-point session outline, write a \
narrative chapter of the session as if it were fiction.

Rules:
- Third person, past tense, flowing paragraphs — no bullet lists.
- Focus on characters' actions, motivations and the living world.
- Capture drama, tension and distinct character voices.
- Do not mention dice, hit points, spell slots, or any out-of-fiction mechanics.
- Begin with the narrative — no preamble such as \"Here is the story\".";

const QUOTES_BASE: &str = "\
You are extracting the most memorable quotes from a session. Produce a markdown \
list of quotes. For each quote: the line in italics, then on a new line `— Speaker \
(player) — context`. Include both in-character and table-side quotes when they're \
memorable. Skip anything mundane. 8–20 quotes total. No preamble.";

const CAMPAIGN_LOG_BASE: &str = "\
You are maintaining a long-running campaign log. You will be given the current log \
(or an empty slate) and a new session summary. Produce the COMPLETE updated log:

- Preserve all existing `## Session N — …` sections verbatim and in order.
- Append a new section for this session with the next index, the date, and a short title.
- Maintain a `## Ongoing Threads` block at the top, updated to reflect what is now \
open vs. resolved (add, edit or remove bullets as appropriate).

Output the entire updated markdown file. No preamble.";

fn compose_system(base: &str, campaign: &CampaignConfig, preset: &Preset, extra: &str, strip_roster: bool) -> String {
    let mut s = String::with_capacity(base.len() + 1024);
    s.push_str(base);
    s.push_str("\n\n--- Campaign context ---\n");
    if strip_roster {
        s.push_str(&campaign.render_context_no_roster());
    } else {
        s.push_str(&campaign.render_context());
    }
    s.push_str("\n--- Game system ---\n");
    s.push_str(&preset.render());
    if !campaign.system.overrides.trim().is_empty() {
        s.push_str("\nCampaign-specific overrides:\n");
        s.push_str(campaign.system.overrides.trim_end());
        s.push('\n');
    }
    let extras = preset.render_extra_sections();
    if !extras.trim().is_empty() {
        s.push_str("\nInclude these extra sections in your output where relevant:\n");
        s.push_str(extras.trim_end());
        s.push('\n');
    }
    if !preset.forbidden_phrases.is_empty() {
        s.push_str("\nAvoid these AI-typical filler words: ");
        s.push_str(&preset.forbidden_phrases.join(", "));
        s.push('\n');
    }
    if !extra.is_empty() {
        s.push_str("\n");
        s.push_str(extra);
    }
    s
}

pub fn system_for(artifact: Artifact, campaign: &CampaignConfig, preset: &Preset) -> String {
    let overrides = &campaign.prompts;
    let base = artifact_override(artifact, overrides).unwrap_or_else(|| artifact_base(artifact).to_string());
    // Strip the character roster from ALL artifact prompts.  The roster maps
    // player names to character names; the LLM uses that mapping to substitute
    // names it sees in the transcript (e.g. "Emilia" → "Fatethrial") even when
    // the transcript contains no gameplay.  Characters present in the session
    // will be named in the transcript itself, so the roster is not needed.
    let extra = if artifact == Artifact::Recap {
        "IMPORTANT: Use only the exact names and events from the bullet-point outline. \
          Do NOT substitute names with character names or player names. \
          If the outline contains no TTRPG gameplay (test recording, setup chatter, etc.), \
          respond with exactly one sentence saying so and stop."
    } else {
        ""
    };
    compose_system(&base, campaign, preset, extra, true)
}

pub fn campaign_log_system(campaign: &CampaignConfig, preset: &Preset) -> String {
    let base = campaign.prompts.campaign_log.clone()
        .unwrap_or_else(|| CAMPAIGN_LOG_BASE.to_string());
    compose_system(&base, campaign, preset, "", false)
}

fn artifact_base(a: Artifact) -> &'static str {
    match a {
        Artifact::Bullets => BULLETS_BASE,
        Artifact::DmNotes => DM_NOTES_BASE,
        Artifact::Recap => RECAP_BASE,
        Artifact::Summary => SUMMARY_BASE,
        Artifact::Story => STORY_BASE,
        Artifact::Quotes => QUOTES_BASE,
    }
}

fn artifact_override(a: Artifact, o: &PromptOverrides) -> Option<String> {
    match a {
        Artifact::Bullets => o.bullets.clone(),
        Artifact::DmNotes => o.dm_notes.clone(),
        Artifact::Recap => o.recap.clone(),
        Artifact::Summary => o.summary.clone(),
        Artifact::Story => o.story.clone(),
        Artifact::Quotes => o.quotes.clone(),
    }
}

pub fn user_bullets_from_transcript(transcript: &str) -> String {
    format!("Transcript:\n\n{transcript}")
}

pub fn user_from_bullets(bullets: &str) -> String {
    format!("Bullet-point outline of the session:\n\n{bullets}")
}

pub fn user_campaign_log_merge(existing: &str, summary: &str, session_date: &str, session_title: &str) -> String {
    format!(
        "Existing campaign log (may be empty):\n\n{existing}\n\n---\n\nNew session date: {session_date}\nSuggested session title: {session_title}\n\nNew session summary:\n\n{summary}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Campaign, CampaignConfig, OutputsConfig, Player, PromptOverrides, SystemRef};
    use crate::presets;

    fn camp() -> CampaignConfig {
        CampaignConfig {
            campaign: Campaign { name: "Test".into(), gm: "Mike".into(), setting: "Damasus".into(), notes: String::new() },
            players: vec![Player { player: "Bob".into(), character: "Drokel".into(), ancestry: "Dwarf".into(), class: "Fighter".into() }],
            system: SystemRef { preset: "dnd5e".into(), overrides: "We use milestone XP.".into() },
            outputs: OutputsConfig::default(),
            prompts: PromptOverrides::default(),
        }
    }

    #[test]
    fn injection_contains_preset_and_campaign() {
        let c = camp();
        let p = presets::load("dnd5e").unwrap();
        let s = system_for(Artifact::DmNotes, &c, &p);
        assert!(!s.contains("Drokel"), "character roster is stripped from all artifact prompts");
        assert!(s.contains("D&D 5e"));
        assert!(s.contains("milestone XP"));
        assert!(s.contains("HP"));
    }
}
