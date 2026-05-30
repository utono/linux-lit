use crate::claude::ClaudeError;
use crate::db::models::{Line, Work};

pub const USER_QUESTION_PROMPT: &str = "\
You are a literary scholar answering a reader's question about a passage from a literary text.

The reader has asked a specific question. Answer it thoroughly, drawing on the passage provided.
Use your knowledge of the text, its historical context, performance tradition, and literary criticism.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution when quoting verse (ALL CAPS, no period)
- <verse>one line of quoted text</verse> for each quoted line (one tag per line, verbatim, preserving exact words and spelling)
- <gloss>paragraph of answer</gloss> for each paragraph of your response

Rules:
- Focus on answering the reader's question directly
- Support your answer with evidence from the passage and the wider work
- When quoting verse from the text, use <speaker> and <verse> tags — never embed verse lines inside <gloss> tags
- Quote verbatim — exact words, exact spelling, exact line breaks from the source
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- No markdown, no bullets, no numbered lists, no headers";

pub const INNER_MONOLOGUE_PROMPT: &str = "\
You are a director helping actors discover the inner monologue beneath \
a passage from a dramatic text.

Given a scene and a highlighted passage within it, explore what each \
character present is thinking, hearing, and feeling — the subtext \
beneath the spoken words.

For each character in the highlighted passage:
- What do they actually hear when the other character speaks? \
(e.g., Claudio hears \"Speak, count, 'tis your cue\" but what he \
really hears is \"speak now or your silence will offend\")
- What inner monologue drives their response? What are they telling \
themselves before they open their mouth?
- What words or phrases could an actor use as inner cues — short, \
actable thoughts that sit beneath each line?
- How does the surrounding scene (lines before AND after the passage) \
illuminate what the character is really saying?

Draw on the full scene provided for evidence. Reference specific lines \
that echo, foreshadow, or reframe the passage's meaning.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each character's analysis section (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers";

pub const INNER_MONOLOGUE_ADD_PROMPT: &str = "\
You are a director helping actors discover the inner monologue beneath \
a passage from a dramatic text.

The reader has selected a passage and provided lines from elsewhere in \
Shakespeare's corpus that share thematic or verbal echoes. Treat the \
provided lines as the unspoken inner voice — what the characters in the \
original passage might be thinking or hearing beneath their spoken words.

For each character in the original passage:
- How do the cross-work lines illuminate what this character is really \
thinking or feeling?
- What verbal echoes connect the two passages (shared words, inverted \
meanings, parallel structures)?
- What actable inner cues can an actor draw from the cross-work lines — \
short thoughts that sit beneath each spoken line?

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each character's analysis section (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers";

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
    pub gloss_type: String,
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
        gloss_type: "teacher-generic".to_string(),
    })
}

pub fn build_context_for_type(work: &Work, lines: &[Line], gloss_type: &str) -> Option<GlossContext> {
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

    let hash_input = format!("{}:{}:{}:{}", base_abbrev, start_citation, end_citation, gloss_type);
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
        gloss_type: gloss_type.to_string(),
    })
}

pub fn build_user_message(
    ctx: &GlossContext,
    user_prompt: Option<&str>,
    existing_gloss: Option<&str>,
) -> String {
    let mut msg = format!(
        "Play: {}\nAct: {}, Scene: {}\nSpeaker: {}\n\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker, ctx.source_text
    );

    if let Some(prompt) = user_prompt {
        msg.push_str(&format!("\n\n---\nUser question: {}", prompt));
    }

    if let Some(existing) = existing_gloss {
        msg.push_str(&format!("\n\n---\nPrevious gloss for reference:\n{}", existing));
    }

    msg
}

pub fn build_inner_monologue_message(
    ctx: &GlossContext,
    scene_lines: &[Line],
) -> String {
    let mut scene_text = String::new();
    let mut last_speaker: Option<&str> = None;
    for line in scene_lines {
        if let Some(ref s) = line.speaker {
            if last_speaker != Some(s.as_str()) {
                scene_text.push_str(&format!("\n{}\n", s));
                last_speaker = Some(s);
            }
        }
        scene_text.push_str(&format!("  {}\n", line.text));
    }

    format!(
        "Play: {}\nAct: {}, Scene: {}\nSpeaker: {}\n\n\
         --- FULL SCENE ---\n{}\n\
         --- HIGHLIGHTED PASSAGE ---\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker,
        scene_text.trim(),
        ctx.source_text,
    )
}

pub fn build_inner_monologue_add_message(
    ctx: &GlossContext,
    pasted_lines: &str,
) -> String {
    format!(
        "Play: {}\nAct: {}, Scene: {}\nSpeaker: {}\n\n\
         --- ORIGINAL PASSAGE ---\n{}\n\n\
         --- CROSS-WORK LINES (inner voice) ---\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker,
        ctx.source_text,
        pasted_lines,
    )
}

pub async fn call_claude(
    user_message: &str,
    model: &str,
) -> Result<String, ClaudeError> {
    crate::claude::send_message(TEACHER_GENERIC_PROMPT, user_message, model).await
}

pub async fn call_claude_with_prompt(
    system_prompt: &str,
    user_message: &str,
    model: &str,
) -> Result<String, ClaudeError> {
    crate::claude::send_message(system_prompt, user_message, model).await
}
