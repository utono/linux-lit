use crate::claude::ClaudeError;
use crate::db::models::{Line, Work};

/// Shared Original-Pronunciation (OP) sound rules, embedded in every gloss
/// prompt via the `op_ipa_conventions!` macro. This is the SINGLE source of
/// truth for *what OP sounds like* — the lexical-set vowels, consonants, and
/// connected-speech features. It deliberately does NOT say where the /IPA/ may
/// appear or how sparsely to tag: those placement/sparsity rules differ per
/// prompt (e.g. TEACHER_GENERIC forbids IPA in <gloss> prose and caps tag
/// density) and stay in each prompt's own rule list.
///
/// Add new OP features HERE, once — every prompt picks them up via `concat!`.
/// See docs/superpowers/specs/2026-06-10-richer-op-ipa-conventions-design.md.
///
/// It is a `macro_rules!` returning a single string literal (not a `const`) so
/// it can be `concat!`'d into the other prompt `const`s at compile time without
/// any dependency.
macro_rules! op_ipa_conventions {
    () => {
        "Use Crystal's Shakespearean Original Pronunciation (OP), NOT modern values, \
for the /IPA/. OP is rhotic — sound every written r and let it colour the vowel \
(letter /ˈlɛtɚ/, art /ɑrt/). Pin these lexical-set vowels to the OP value, never \
the modern one: FACE (daily, gave, day) = OP monophthong /eː/ NOT /eɪ/; GOAT \
(go, so) = /oː/ NOT /əʊ/; PRICE (wise, time, I) = /əɪ/ NOT /aɪ/; CHOICE (boy) = \
/əɪ/; MOUTH (house, now) = /əʊ/ NOT /aʊ/; happY (city, money) = /əɪ/ NOT /i/; \
STRUT (love, blood, cut) = /ɤ/; TRAP (bath, path, man) = /a/ (no broad-a); \
LOT/THOUGHT (lot, call) = /ɑ/; DRESS (bed) = /ɛ/ or /ɛː/; FLEECE (meet) = \
/eː/~/iː/; GOOSE (food) = /uː/; KIT (sit) = /ɪ/. MEAT–MEET split: \
great/break/steak keep /ɛː/ (great /ɡrɛːt/, not /ɡriːt/). So daily is /ˈdeːli/ \
(or /ˈdeɪli/), gave /ɡeːv/, wise /wəɪz/ — never modern diphthongs. \
Consonants & connected speech (still OP, applied only to a word you are already \
tagging — never a reason to tag MORE words): suffix -ing → /ɪn/ (calling \
/ˈkɑlɪn/, singing /ˈsɪŋɪn/). Aspirated wh- → /ʍ/ in which /ʍɪtʃ/, when /ʍɛn/, \
why /ʍəɪ/, whither — but who, whom, whole keep /h/. Fuller -sion/-tion → /sɪən/ \
(not /ʃən/) ONLY when the metre admits the extra syllable; otherwise /ʃən/. In \
casual delivery, drop initial /h/ on unstressed his, her, him, he (who's her \
best friend → /huːz ə bɛst/), and elide medial /v/ and /ð/ in common words \
(heaven /ˈhɛən/, even /ˈiːən/, devil /ˈdiːl/, seven /ˈsɛən/, hither /ˈhɪər/). \
Reduce unstressed function words to their weakest form — and /ən/, of /ə/, to \
/tə/, for /fər/, my /mɪ/, thou /ðə/, the /ðə/ — but this tells you HOW to render \
a function word IF you have chosen to tag it for a connected-speech effect; it \
is NOT licence to tag every function word. The operative / accent-bearing word \
rule still governs WHAT gets tagged. Include stress markers on multi-syllable \
tags: primary /ˈ/ and secondary /ˌ/ before the stressed syllable (/ˈdeːli/, \
/əˈpoːzɪn/). But let line structure, not IPA, govern syllable count — leave -ed \
and -ion syllabicity to the metre."
    };
}

pub const USER_QUESTION_PROMPT: &str = concat!("\
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
- On each <verse> line, APPEND inline Original-Pronunciation IPA in forward slashes IMMEDIATELY AFTER the operative / accent-bearing / metrically stressed words (e.g. take /tɛːk/), leaving the original words unchanged; per word never per phrase; let line structure govern syllable count.
- ", op_ipa_conventions!(), "
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- No markdown, no bullets, no numbered lists, no headers");

pub const INNER_MONOLOGUE_PROMPT: &str = concat!("\
You are a director using the actioning technique to discover the inner \
monologue beneath a passage from a dramatic text.

METHOD — for each spoken line in the highlighted passage:

1. ACTION the line: name the transitive verb the character performs on \
their target — what they DO TO the other person (e.g. claim, deflect, \
trap, reassure, provoke, dismiss, seduce, warn). The verb must pass \
the 'I ___ you' test.

2. Identify the GOVERNING CONVENTION that shapes the action — courtly \
love (the suitor claims, the lady judges), honor code (challenge, \
defend, yield), filial duty (obey, resist, negotiate), political \
maneuvering (flatter, threaten, ally), religious obligation (confess, \
absolve, submit), or another Elizabethan social code.

3. Identify the COGNITIVE FUNCTION of the line — what the thought \
beneath it is doing:
- ANTECEDENT: the thought that triggers the words (what makes the \
character open their mouth right now)
- PIVOT: the line turns mid-stream — the character discovers something \
or redirects while speaking
- CONCEALMENT: the character holds back what they almost said, hiding \
a secret identity, allegiance, or knowledge
- READING: the character is responding to what they see in the scene \
partner's face, breath, or posture
Choose the dominant function for the line.

4. Find a CROSS-WORK ECHO: a line from a DIFFERENT work in \
Shakespeare's corpus (plays, sonnets, narrative poems) where a \
character performs the SAME transitive verb, under the SAME \
convention, serving the SAME cognitive function. The echo must match \
the dramatic action, not the surface vocabulary.

Output format — use these XML tags exactly, in this order for each line:
- <speaker>NAME</speaker> before each new speaker (ALL CAPS)
- <verse>one line of quoted text</verse> (verbatim, one per line)
- <gloss>[\"echo line\" — Source Work act.scene]</gloss>

Example — Paris CLAIMS Juliet (courtly love / antecedent: the greeting \
is a claim on her body dressed as courtesy — the echo strips the veil \
off the transaction):
<speaker>PARIS</speaker>
<verse>Happily met, my lady and my wife.</verse>
<gloss>[\"Are you meditating on virginity?\" — All's Well That Ends \
Well 1.1]</gloss>

Example — Juliet DEFLECTS Paris (courtly love / concealment: she spins \
a key word from the line back at him to buy time — the echo does the \
same thing with the same word):
<speaker>JULIET</speaker>
<verse>That may be, sir, when I may be a wife.</verse>
<gloss>[\"What may she not? She may, ay, marry, may she—\" — Richard \
III 1.3]</gloss>

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks. \
\"Verbatim\" governs the original words only; inline /IPA/ tags are \
ADDITIONAL markup placed immediately AFTER the word they annotate \
(e.g. take /tɛːk/) and never replace, reorder, or alter the source word.
- On each <verse> line, APPEND inline Original-Pronunciation IPA in \
forward slashes IMMEDIATELY AFTER the operative / accent-bearing / \
metrically stressed words (e.g. take /tɛːk/), leaving the original \
words unchanged; per word never per phrase; let line structure govern \
syllable count.
- ", op_ipa_conventions!(), "
- Do NOT add a <pron> note. The <gloss> remains EXACTLY the single \
bracketed echo — the /IPA/ goes only inside <verse>.
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- ONE <gloss> tag per verse line: the bracketed echo only. Do NOT add \
any actable-subtext sentence or any prose after the echo.
- The bracketed echo contains EXACTLY ONE quoted line and ONE source \
citation — never list alternatives, never show your deliberation, \
never write words like 'not usable', 'replace', 'wait', or 'use'. \
Do your thinking silently; output only the single chosen echo: \
[\"line\" — Source Work act.scene]
- The echo must come from a DIFFERENT work in Shakespeare's corpus — \
cite the source
- Match on TRANSITIVE VERB + CONVENTION + COGNITIVE FUNCTION, never \
on surface words
- The best echoes do one of these: (a) strip the polite surface off \
what the character is really doing, exposing the raw transaction \
beneath the courtesy, or (b) share a KEY WORD that the original \
character is weaponizing, spinning, or hiding behind — find a line \
where another character does the same thing with the same word
- For CONCEALMENT lines, find echoes where a character similarly hides \
a secret (Viola concealing identity, Hal concealing intention, etc.)
- For READING lines, find echoes where a character responds to what \
they see in the other (Iago reading Othello, Portia reading Bassanio)
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers");

pub const INNER_MONOLOGUE_ADD_PROMPT: &str = concat!("\
You are a director using the actioning technique to discover the inner \
monologue beneath a passage from a dramatic text.

The reader has provided lines from elsewhere in Shakespeare's corpus. \
The reader chose these lines because a character in them performs the \
same transitive verb (claim, deflect, trap, warn, etc.) under the \
same governing convention (courtly love, honor code, filial duty, \
etc.) as the character in the original passage.

For each line in the original passage:
1. ACTION the line: name the transitive verb ('I ___ you').
2. Select from the provided lines the phrase where a character \
performs the same action. Cite the source work.

Output format — use these XML tags exactly, in this order for each line:
- <speaker>NAME</speaker> before each new speaker (ALL CAPS)
- <verse>one line of quoted text</verse> (verbatim, one per line)
- <gloss>[\"echo from the provided lines\" — Source Work act.scene]</gloss>

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks. \
\"Verbatim\" governs the original words only; inline /IPA/ tags are \
ADDITIONAL markup placed immediately AFTER the word they annotate \
(e.g. take /tɛːk/) and never replace, reorder, or alter the source word.
- On each <verse> line, APPEND inline Original-Pronunciation IPA in \
forward slashes IMMEDIATELY AFTER the operative / accent-bearing / \
metrically stressed words (e.g. take /tɛːk/), leaving the original \
words unchanged; per word never per phrase.
- ", op_ipa_conventions!(), "
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- ONE <gloss> tag per verse line: the bracketed echo only. Do NOT add \
any actable-subtext sentence or any prose after the echo.
- The bracketed echo contains EXACTLY ONE quoted line and ONE source \
citation — never list alternatives or show your deliberation
- Draw the echoes FROM THE PROVIDED LINES, not your own knowledge
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers");

pub const INNER_MONOLOGUE_EDIT_PROMPT: &str = concat!("\
You are a director using the actioning technique to discover the inner \
monologue beneath a passage from a dramatic text.

The reader is viewing an existing gloss and has provided new lines \
from elsewhere in Shakespeare's corpus to replace the current echoes. \
The reader chose these lines because a character in them performs the \
same transitive verb under the same governing convention as the \
character in the original passage. Re-gloss using the provided lines.

For each line in the original passage:
1. ACTION the line: name the transitive verb ('I ___ you').
2. Select from the provided lines the phrase where a character \
performs the same action. Cite the source work.

Output format — use these XML tags exactly, in this order for each line:
- <speaker>NAME</speaker> before each new speaker (ALL CAPS)
- <verse>one line of quoted text</verse> (verbatim, one per line)
- <gloss>[\"echo from the provided lines\" — Source Work act.scene]</gloss>

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks. \
\"Verbatim\" governs the original words only; inline /IPA/ tags are \
ADDITIONAL markup placed immediately AFTER the word they annotate \
(e.g. take /tɛːk/) and never replace, reorder, or alter the source word.
- On each <verse> line, APPEND inline Original-Pronunciation IPA in \
forward slashes IMMEDIATELY AFTER the operative / accent-bearing / \
metrically stressed words (e.g. take /tɛːk/), leaving the original \
words unchanged; per word never per phrase.
- ", op_ipa_conventions!(), "
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- ONE <gloss> tag per verse line: the bracketed echo only. Do NOT add \
any actable-subtext sentence or any prose after the echo.
- The bracketed echo contains EXACTLY ONE quoted line and ONE source \
citation — never list alternatives or show your deliberation
- Draw the echoes FROM THE PROVIDED LINES, not your own knowledge
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers");

pub const EDIT_GLOSS_PROMPT: &str = concat!("\
You are a literary scholar revising an existing gloss about a passage \
from a literary text.

The reader is viewing an existing gloss and has provided additional \
lines or context to improve it. Rewrite the gloss incorporating the \
new material the reader has provided.

Use the same output format as the original gloss — use these XML tags:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks
- Never use / to join verse lines
- On each <verse> line, APPEND inline Original-Pronunciation IPA in forward slashes IMMEDIATELY AFTER the operative / accent-bearing / metrically stressed words (e.g. take /tɛːk/), leaving the original words unchanged; per word never per phrase; let line structure govern syllable count.
- ", op_ipa_conventions!(), "
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- Incorporate the reader's provided lines as evidence or context
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers");

pub const FIX_IPA_PROMPT: &str = "\
Return ONLY the Original-Pronunciation IPA for the given English word, wrapped in \
forward slashes (e.g. /ˈdeɪli/). Use Shakespearean Original Pronunciation (rhotic; \
FACE as the monophthong /ɛː/ or diphthong per the hint; PRICE /əɪ/; MOUTH /əʊ/). \
Honor the user's hint about the desired sound. Output the slash-wrapped IPA and \
nothing else — no prose, no the word, no explanation.";

const TEACHER_GENERIC_PROMPT: &str = concat!("\
You are a performance-focused teacher helping a reader understand a passage from a literary text.

Given a passage with speaker names and dialogue, provide an actor's explication that:
- Paraphrases the passage in clear, modern English
- Explains archaic vocabulary, allusions, and complex syntax
- Notes rhetorical devices, verse structure, and breath patterns that shape delivery
- On each <verse> line, tag for Original Pronunciation ONLY the few words you have already identified as operative / accent-bearing / metrically stressed — per word, never per phrase. Tagging every word destabilizes synthesis and muddies the teaching. Append the pronunciation as IPA in forward slashes IMMEDIATELY AFTER the operative word, leaving the original word unchanged, e.g. take /tɛːk/, suffer /ˈsʊfər/. \
", op_ipa_conventions!(), "
- Identifies the speaker's intention, operative words, and emotional arc
- References classical pedagogy where relevant (Barton, Berry, Hall, Rodenburg, Linklater)
- Defines literary terminology on first use (enjambment, caesura, anaphora, antithesis, etc.)

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS, no period)
- <verse>one line of quoted text</verse> for each quoted line (one tag per line, verbatim from source, preserving exact words and spelling)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks from the source. \"Verbatim\" governs the original words only; inline /IPA/ tags are ADDITIONAL markup placed immediately AFTER the word they annotate (e.g. take /tɛːk/) and never replace, reorder, or alter the source word.
- /IPA/ appears ONLY inside <verse> lines (where it is hidden from the reader and used only to generate audio). NEVER write IPA, phonetic symbols, or slash-wrapped pronunciations anywhere in <gloss> prose — do not mention how a word is pronounced using symbols. Explain meaning and delivery in plain words only.
- Never use / to join verse lines
- Never truncate with ...
- Tag sparsely: only operative/accent-bearing words get /IPA/. A 40-word line should have far fewer than 40 tags.
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-4 sentences preferred, never exceed 6)
- For long speeches (over 8 lines), break into 4-8 line chunks with analysis between each chunk
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines, even when the speaker has not changed — every quoted block must be preceded by its speaker name
- No markdown, no bullets, no numbered lists, no headers");

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

pub fn build_edit_gloss_message(
    ctx: &GlossContext,
    existing_gloss: &str,
    pasted_lines: &str,
) -> String {
    format!(
        "Play: {}\nAct: {}, Scene: {}\nSpeaker: {}\n\n\
         --- ORIGINAL PASSAGE ---\n{}\n\n\
         --- EXISTING GLOSS ---\n{}\n\n\
         --- USER-PROVIDED LINES (use as subtext/context) ---\n{}",
        ctx.work_title, ctx.act, ctx.scene, ctx.speaker,
        ctx.source_text,
        existing_gloss,
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

pub fn verify_echo_citations(gloss_text: &str, source_work: &str) -> String {
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return gloss_text.to_string(),
    };

    let mut result = String::new();
    for line in gloss_text.lines() {
        if !result.is_empty() {
            result.push('\n');
        }

        if !(line.starts_with("<gloss>[\"") || line.starts_with("<gloss>[\\\"")) {
            result.push_str(line);
            continue;
        }

        let quote_text = extract_echo_quote(line);
        if quote_text.is_empty() {
            result.push_str(line);
            continue;
        }

        if let Some(citation) = lookup_citation(&conn, &quote_text, source_work) {
            let corrected = replace_citation(line, &citation);
            result.push_str(&corrected);
            crate::logging::log(&format!("GLOSS VERIFY: corrected citation to {}", citation));
        } else {
            let flagged = flag_unverified(line);
            result.push_str(&flagged);
            crate::logging::log(&format!("GLOSS VERIFY: could not verify \"{}\"", quote_text));
        }
    }
    result
}

fn extract_echo_quote(line: &str) -> String {
    let inner = line
        .trim_start_matches("<gloss>")
        .trim_end_matches("</gloss>");
    let start = if let Some(pos) = inner.find("[\"") {
        pos + 2
    } else if let Some(pos) = inner.find("[\\\"") {
        pos + 3
    } else {
        return String::new();
    };
    let rest = &inner[start..];
    let end = rest.find("\"")
        .or_else(|| rest.find("\\\""))
        .unwrap_or(rest.len());
    let raw = &rest[..end];
    raw.trim().to_string()
}

fn lookup_citation(
    conn: &rusqlite::Connection,
    quote: &str,
    source_work: &str,
) -> Option<String> {
    let base_source = source_work.strip_suffix("-Amb").unwrap_or(source_work);

    let exact: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT work_abbrev, div1, div2 FROM line_mapping \
             WHERE canonical_text = ?1 AND work_abbrev != ?2 AND work_abbrev NOT LIKE '%-Amb' \
             LIMIT 1",
            rusqlite::params![quote, base_source],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    if let Some((abbrev, act, scene)) = exact {
        let title = abbrev_to_title(conn, &abbrev);
        return Some(format!("{} {}.{}", title, act, scene));
    }

    let like_pattern = format!("%{}%", quote);
    let fuzzy: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT work_abbrev, div1, div2 FROM line_mapping \
             WHERE canonical_text LIKE ?1 AND work_abbrev != ?2 AND work_abbrev NOT LIKE '%-Amb' \
             LIMIT 1",
            rusqlite::params![like_pattern, base_source],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    if let Some((abbrev, act, scene)) = fuzzy {
        let title = abbrev_to_title(conn, &abbrev);
        return Some(format!("{} {}.{}", title, act, scene));
    }

    None
}

fn abbrev_to_title(conn: &rusqlite::Connection, abbrev: &str) -> String {
    conn.query_row(
        "SELECT title FROM works WHERE abbrev = ?1",
        rusqlite::params![abbrev],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| abbrev.to_string())
}

fn replace_citation(line: &str, correct_citation: &str) -> String {
    if let Some(dash_pos) = line.rfind(" — ") {
        if let Some(close_pos) = line[dash_pos..].find("]") {
            let before = &line[..dash_pos];
            let after = &line[dash_pos + close_pos..];
            return format!("{} — {}{}", before, correct_citation, after);
        }
    }
    if let Some(dash_pos) = line.rfind(" - ") {
        if let Some(close_pos) = line[dash_pos..].find("]") {
            let before = &line[..dash_pos];
            let after = &line[dash_pos + close_pos..];
            return format!("{} — {}{}", before, correct_citation, after);
        }
    }
    line.to_string()
}

fn flag_unverified(line: &str) -> String {
    if line.contains("(unverified)") {
        return line.to_string();
    }
    line.replace("</gloss>", " (unverified)</gloss>")
}
