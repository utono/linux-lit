use crate::claude::ClaudeError;
use crate::db::models::{Line, Work};

const TEACHER_GENERIC_PROMPT: &str = "\
You are a performance-focused teacher helping a reader understand a passage from a literary text.

Given a passage with speaker names and dialogue, provide an actor's explication that:
- Paraphrases the passage in clear, modern English
- Explains archaic vocabulary, allusions, and complex syntax
- Notes rhetorical devices, verse structure, and breath patterns that shape delivery
- Identifies the speaker's intention, operative words, and emotional arc
- References classical pedagogy where relevant (Barton, Berry, Hall, Rodenburg, Linklater)
- Defines literary terminology on first use (enjambment, caesura, anaphora, antithesis, etc.)

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS, no period)
- <verse>one line of quoted text</verse> for each quoted line (one tag per line, verbatim from source, preserving exact words and spelling)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks from the source
- Never use / to join verse lines
- Never truncate with ...
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-4 sentences preferred, never exceed 6)
- For long speeches (over 8 lines), break into 4-8 line chunks with analysis between each chunk
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines, even when the speaker has not changed — every quoted block must be preceded by its speaker name
- No markdown, no bullets, no numbered lists, no headers";

#[derive(Clone)]
pub struct GlossContext {
    pub work_abbrev: String,
    pub work_title: String,
    pub start_citation: String,
    pub end_citation: String,
    pub act: i64,
    pub scene: i64,
    pub speaker: String,
    pub source_text: String,
    pub source_line_numbers: Vec<i64>,
    pub hash: String,
}

impl GlossContext {
    pub fn source_line_pairs(&self) -> Vec<(String, i64)> {
        self.source_text
            .lines()
            .zip(self.source_line_numbers.iter())
            .map(|(text, &num)| (text.to_string(), num))
            .collect()
    }
}

fn normalize_abbrev(abbrev: &str) -> &str {
    abbrev.strip_suffix("-Amb").unwrap_or(abbrev)
}

pub fn build_context(work: &Work, lines: &[Line]) -> Option<GlossContext> {
    if lines.is_empty() {
        return None;
    }
    let base_abbrev = normalize_abbrev(&work.abbrev);
    let first = lines.first().unwrap();
    let last = lines.last().unwrap();
    let start_citation = format!("{}.{}.{}.{}", base_abbrev, first.div1, first.div2, first.line_in_div);
    let end_citation = format!("{}.{}.{}.{}", base_abbrev, last.div1, last.div2, last.line_in_div);

    let mut speakers: Vec<&str> = Vec::new();
    for line in lines {
        if let Some(ref s) = line.speaker {
            if speakers.last() != Some(&s.as_str()) {
                speakers.push(s);
            }
        }
    }
    let speaker = if speakers.is_empty() {
        "UNKNOWN".to_string()
    } else {
        speakers.join(", ")
    };

    let source_text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    let source_line_numbers: Vec<i64> = lines.iter().map(|l| l.line_in_div).collect();

    let hash_input = format!("{}:{}:{}:teacher-generic", base_abbrev, start_citation, end_citation);
    let hash = format!("{:x}", md5::compute(hash_input.as_bytes()));

    Some(GlossContext {
        work_abbrev: base_abbrev.to_string(),
        work_title: work.title.clone(),
        start_citation,
        end_citation,
        act: first.div1,
        scene: first.div2,
        speaker,
        source_text,
        source_line_numbers,
        hash,
    })
}

pub fn build_user_message(
    ctx: &GlossContext,
    amend_prompt: Option<&str>,
    existing_gloss: Option<&str>,
) -> String {
    let mut msg = format!(
        "Play: {}\nAct: {}, Scene: {}\nSpeaker: {}\n\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker, ctx.source_text
    );

    if let (Some(existing), Some(prompt)) = (existing_gloss, amend_prompt) {
        msg.push_str(&format!(
            "\n\n---\nPrevious gloss:\n{}\n\n---\nEnhancement request: {}",
            existing, prompt
        ));
    }

    msg
}

pub async fn call_claude(
    user_message: &str,
    model: &str,
) -> Result<String, ClaudeError> {
    crate::claude::send_message(TEACHER_GENERIC_PROMPT, user_message, model).await
}
