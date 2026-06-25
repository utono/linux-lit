use crate::claude::ClaudeError;
use crate::db::models::{Line, Work};
use std::sync::LazyLock;

/// Active template for `key` from lit.db, or the compiled `fallback` verbatim.
///
/// The lit.db row (managed by the `claude-api-prompts` repo) is authoritative
/// and may have been edited past the compiled `fallback`. Each `FALLBACK` const
/// below is the seed/v1 text and only renders when the DB row is missing or the
/// DB is unreachable — so a `FALLBACK` deliberately diverging from the active DB
/// prompt is expected, not a bug.
fn template_or(key: &str, fallback: &str) -> String {
    crate::db::prompts::active_prompt(key).unwrap_or_else(|| fallback.to_string())
}

/// Master switch for inline OP-IPA tagging on `<verse>` source lines.
///
/// `false` (current) — every gloss prompt (and `FIX_IPA_PROMPT`) is built with
/// its IPA-tagging instructions replaced by a one-line directive telling the
/// model NOT to add any `/IPA/` markup. New glosses quote the source words
/// exactly, with no phonetic tags.
///
/// `true` — restores the original behavior: prompts instruct the model to
/// append inline OP-IPA after operative words, using the OP conventions block.
///
/// **To revert** (re-enable IPA wrapping): flip this to `true`, or `git revert`
/// the commit that introduced this flag. Nothing else needs to change — the
/// `IPA_*` fragments below are selected from this single constant. Existing
/// stored glosses are untouched either way (display strips `/IPA/`; TTS still
/// reads it from stored verse).
const APPEND_IPA: bool = false;

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

/// The IPA-instruction block spliced into each *gloss* prompt's rule list.
/// When `APPEND_IPA` is false this is a single suppression line; when true it is
/// the full "APPEND inline IPA" directive plus the shared OP conventions.
/// Selected once at first use from the single `APPEND_IPA` switch above.
static IPA_VERSE_RULES: LazyLock<String> = LazyLock::new(|| {
    if APPEND_IPA {
        concat!(
            "On each <verse> line, APPEND inline Original-Pronunciation IPA in forward slashes IMMEDIATELY AFTER the operative / accent-bearing / metrically stressed words (e.g. take /tɛːk/), leaving the original words unchanged; per word never per phrase; let line structure govern syllable count.\n- ",
            op_ipa_conventions!()
        )
        .to_string()
    } else {
        template_or(
            "ipa.verse-rules",
            "Do NOT add /IPA/ pronunciation tags to verse lines. Quote the source words exactly as written, with no phonetic markup of any kind.",
        )
    }
});

/// The IPA-tagging clause for `TEACHER_GENERIC_PROMPT` (its sparse-tagging
/// wording) when `APPEND_IPA` is true, or the same no-IPA suppression line when
/// false.
static IPA_VERSE_RULES_SPARSE: LazyLock<String> = LazyLock::new(|| {
    if APPEND_IPA {
        concat!(
            "On each <verse> line, tag for Original Pronunciation ONLY the few words you have already identified as operative / accent-bearing / metrically stressed — per word, never per phrase. Tagging every word destabilizes synthesis and muddies the teaching. A 40-word line should have far fewer than 40 tags. /IPA/ appears ONLY inside <verse> lines (hidden from the reader, used only to generate audio). Append the pronunciation as IPA in forward slashes IMMEDIATELY AFTER the operative word, leaving the original word unchanged, e.g. take /tɛːk/, suffer /ˈsʊfər/. \n",
            op_ipa_conventions!()
        )
        .to_string()
    } else {
        template_or(
            "ipa.verse-rules-sparse",
            "Do NOT add /IPA/ pronunciation tags to verse lines. Quote the source words exactly as written, with no phonetic markup of any kind.",
        )
    }
});

pub static USER_QUESTION_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
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
- Never use the = sign. Write paraphrases as prose (write 'X means Y' or 'X, i.e. Y', not 'X = Y').
- {}
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.user-question", FALLBACK);
    let rules: &str = &IPA_VERSE_RULES;
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", rules)
    } else {
        template.replacen("{}", rules, 1)
    }
});

pub static JOURNAL_QA_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a literary interlocutor in conversation with a reader who is working through a play, one scene at a time. The reader has asked a question while reading a specific scene. The verbatim text of that scene is provided.

Answer the question substantively and in plain prose. Ground your answer in the scene text provided, but DO situate the scene within the whole play: trace how this moment echoes earlier scenes and foreshadows or is answered by later ones, and how it participates in the work's larger arcs of character, theme, and image. Drawing such connections across the full play is encouraged — this is a study companion for a reader engaging the entire work, not a spoiler-free first-read assistant, so do not withhold connections to later scenes.

Write for a thoughtful reader: clear, specific, and concrete. Quote sparingly from the scene where it helps. No markdown, no bullet lists, no numbered lists, no headers — flowing prose paragraphs only. Do not use the = sign; write paraphrases as prose. Be substantive but not padded.";
    template_or("journal.qa", FALLBACK)
});

pub static INNER_MONOLOGUE_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
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
- Quote verbatim — exact words, exact spelling, exact line breaks.
- {}
- Do NOT add a <pron> note. The <gloss> remains EXACTLY the single \
bracketed echo.
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
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.inner-monologue", FALLBACK);
    let rules: &str = &IPA_VERSE_RULES;
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", rules)
    } else {
        template.replacen("{}", rules, 1)
    }
});

pub static INNER_MONOLOGUE_ADD_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
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
- Quote verbatim — exact words, exact spelling, exact line breaks.
- {}
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- ONE <gloss> tag per verse line: the bracketed echo only. Do NOT add \
any actable-subtext sentence or any prose after the echo.
- The bracketed echo contains EXACTLY ONE quoted line and ONE source \
citation — never list alternatives or show your deliberation
- Draw the echoes FROM THE PROVIDED LINES, not your own knowledge
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.inner-monologue-add", FALLBACK);
    let rules: &str = &IPA_VERSE_RULES;
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", rules)
    } else {
        template.replacen("{}", rules, 1)
    }
});

pub static INNER_MONOLOGUE_EDIT_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
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
- Quote verbatim — exact words, exact spelling, exact line breaks.
- {}
- Never use / to join verse lines
- Each <verse> tag contains exactly one line of the original
- ONE <gloss> tag per verse line: the bracketed echo only. Do NOT add \
any actable-subtext sentence or any prose after the echo.
- The bracketed echo contains EXACTLY ONE quoted line and ONE source \
citation — never list alternatives or show your deliberation
- Draw the echoes FROM THE PROVIDED LINES, not your own knowledge
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.inner-monologue-edit", FALLBACK);
    let rules: &str = &IPA_VERSE_RULES;
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", rules)
    } else {
        template.replacen("{}", rules, 1)
    }
});

pub static EDIT_GLOSS_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
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
- Never use the = sign. Write paraphrases as prose (write 'X means Y' or 'X, i.e. Y', not 'X = Y').
- {}
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-6 sentences)
- Incorporate the reader's provided lines as evidence or context
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.edit", FALLBACK);
    let rules: &str = &IPA_VERSE_RULES;
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", rules)
    } else {
        template.replacen("{}", rules, 1)
    }
});

pub static READER_GLOSS_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a scholar helping a READER (not an actor) understand a passage from a verse play as it functions within its scene.

Your job: explicate the characters' motives and any Elizabethan vocabulary, allusions, metaphors, idioms, or social/political concepts a modern reader would miss. Be terse. This is NOT acting direction.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS, no period), before every group of <verse> lines
- <verse>one line of quoted text</verse> for each quoted line (one tag per line, verbatim from source, exact words and spelling)
- <gloss>paragraph</gloss> for each prose paragraph

Rules:
- EACH speaker gets their OWN one-sentence motivation lede, placed immediately AFTER that speaker's FIRST quoted <verse> block (the speaker's opening lines come first, then their lede <gloss>). A lede must never come before any verse. A speaker who appears more than once gets a lede only after their first appearance, not on later turns.
- Each motivation lede is exactly one sentence stating what THAT speaker is doing in this moment. Lead it with a PRECISE ACTIVE VERB that names the rhetorical or dramatic move itself (e.g. insinuates, feigns, fishes for, angles for, goads, needles, deflects, flatters, threatens, pleads, taunts, dissembles, parries). Do NOT use weak periphrastic wind-ups — never 'wants to', 'tries to', 'attempts to', 'is trying to', 'seeks to'; the verb must carry the meaning directly (write 'Margaret slyly insinuates…', not 'Margaret wants to flatter…').
- After the lede, each <gloss> is terse (1-3 sentences) explicating further motive shifts and Elizabethan words, allusions, metaphors, idioms, or concepts a reader would miss.
- Do NOT give acting direction: no operative words, no breath, no verse-delivery notes, no Barton/Berry/Hall/Rodenburg/Linklater references.
- NEVER write IPA, phonetic symbols, or slash-wrapped pronunciations anywhere.
- Quote verbatim — exact words, exact spelling, exact line breaks from the source.
- Never use / to join verse lines. Never truncate with ...
- Never use the = sign. Write paraphrases as prose (write 'X means Y' or 'X, i.e. Y', not 'X = Y').
- Each <verse> tag contains exactly one line of the original.
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines, even when the speaker has not changed.
- No markdown, no bullets, no numbered lists, no headers.";
    template_or("gloss.reader-gloss", FALLBACK)
});

pub static READER_GLOSS_QUESTION_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a scholar answering a READER's question about a passage from a verse play, in the terse reader-focused voice (character motive + Elizabethan concepts a reader would miss, NOT acting direction).

The reader has asked a specific question. Answer it directly and concisely, drawing on the passage and the wider work.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> when quoting verse (ALL CAPS, no period)
- <verse>one line of quoted text</verse> for each quoted line (verbatim)
- <gloss>paragraph of answer</gloss> for each paragraph

Rules:
- Answer the reader's question directly; do NOT restate or duplicate the motivation lede.
- When quoting verse, use <speaker>/<verse> tags — never embed verse inside <gloss>.
- Quote verbatim. Never use / to join verse lines.
- Never use the = sign. Write paraphrases as prose (write 'X means Y' or 'X, i.e. Y', not 'X = Y').
- No acting direction, no IPA, no phonetic symbols.
- Each <gloss> is terse (1-3 sentences).
- No markdown, no bullets, no numbered lists, no headers.";
    template_or("gloss.reader-gloss-question", FALLBACK)
});

pub static READER_GLOSS_EDIT_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are revising an existing READER gloss of a passage from a verse play, in the terse reader-focused voice.

The reader has provided additional lines or context. Rewrite the gloss incorporating the new material.

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS)
- <verse>one line of quoted text</verse> for quoted lines (verbatim)
- <gloss>paragraph</gloss> for each paragraph

Rules:
- PRESERVE each speaker's one-sentence motivation lede: every distinct speaker has exactly one, placed immediately AFTER that speaker's first <verse> block (the speaker's opening lines come first, then their lede <gloss>), never before any verse. Rewrite a lede if the new context warrants, but NEVER drop it. Each lede leads with a precise active verb naming the rhetorical move (insinuates, feigns, goads, deflects, flatters…) — never weak wind-ups like 'wants to'/'tries to'.
- After each speaker's lede, the remaining <gloss> blocks are terse (1-3 sentences): character motive and Elizabethan concepts a reader would miss. No acting direction. No IPA.
- Quote verbatim. Never use / to join verse lines.
- Never use the = sign. Write paraphrases as prose (write 'X means Y' or 'X, i.e. Y', not 'X = Y').
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines.
- No markdown, no bullets, no numbered lists, no headers.";
    template_or("gloss.reader-gloss-edit", FALLBACK)
});

pub static FIX_IPA_PROMPT: LazyLock<String> = LazyLock::new(|| {
    if APPEND_IPA {
        "\
Return ONLY the Original-Pronunciation IPA for the given English word, wrapped in \
forward slashes (e.g. /ˈdeɪli/). Use Shakespearean Original Pronunciation (rhotic; \
FACE as the monophthong /ɛː/ or diphthong per the hint; PRICE /əɪ/; MOUTH /əʊ/). \
Honor the user's hint about the desired sound. Output the slash-wrapped IPA and \
nothing else — no prose, no the word, no explanation.".to_string()
    } else {
        // IPA tagging is disabled; the i-key fix path should not synthesize new
        // /IPA/. Return an empty pronunciation so nothing is appended.
        template_or(
            "gloss.fix-ipa",
            "IPA pronunciation tagging is currently disabled. Return ONLY two forward slashes with nothing between them (//) and no other text.",
        )
    }
});

static TEACHER_GENERIC_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a performance-focused teacher helping a reader understand a passage from a literary text.

Given a passage with speaker names and dialogue, provide an actor's explication that:
- Paraphrases the passage in clear, modern English
- Explains archaic vocabulary, allusions, and complex syntax
- Notes rhetorical devices, verse structure, and breath patterns that shape delivery
- {}
- Identifies the speaker's intention, operative words, and emotional arc
- References classical pedagogy where relevant (Barton, Berry, Hall, Rodenburg, Linklater)
- Defines literary terminology on first use (enjambment, caesura, anaphora, antithesis, etc.)

Output format — use these XML tags exactly:
- <speaker>NAME</speaker> for each speaker attribution (ALL CAPS, no period)
- <verse>one line of quoted text</verse> for each quoted line (one tag per line, verbatim from source, preserving exact words and spelling)
- <gloss>paragraph of analysis</gloss> for each analysis paragraph

Rules:
- Quote verbatim — exact words, exact spelling, exact line breaks from the source.
- NEVER write IPA, phonetic symbols, or slash-wrapped pronunciations anywhere in <gloss> prose — do not mention how a word is pronounced using symbols. Explain meaning and delivery in plain words only.
- Never use / to join verse lines
- Never truncate with ...
- Never use the = sign. Write paraphrases as prose (write 'X means Y' or 'X, i.e. Y', not 'X = Y').
- Each <verse> tag contains exactly one line of the original
- Each <gloss> tag contains one flowing prose paragraph (3-4 sentences preferred, never exceed 6)
- For long speeches (over 8 lines), break into 4-8 line chunks with analysis between each chunk
- ALWAYS place a <speaker> tag before EVERY group of <verse> lines, even when the speaker has not changed — every quoted block must be preceded by its speaker name
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.teacher-generic", FALLBACK);
    let rules: &str = &IPA_VERSE_RULES_SPARSE;
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", rules)
    } else {
        template.replacen("{}", rules, 1)
    }
});

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

/// Normalize a work abbrev for gloss storage/lookup by stripping the `-Amb`
/// (Ambrose-edition) suffix, so a base work and its `-Amb` companion share the
/// same gloss rows. See `docs/superpowers/specs/2026-04-29-glossing-design.md`.
pub fn normalize_abbrev(abbrev: &str) -> &str {
    abbrev.strip_suffix("-Amb").unwrap_or(abbrev)
}

pub fn build_context(work: &Work, lines: &[Line]) -> Option<GlossContext> {
    if lines.is_empty() {
        return None;
    }
    let base_abbrev = normalize_abbrev(&work.abbrev);
    let first = lines.first().unwrap();
    let last = lines.last().unwrap();
    let start_citation = crate::db::models::citation(base_abbrev, first.div1, first.div2, first.line_in_div);
    let end_citation = crate::db::models::citation(base_abbrev, last.div1, last.div2, last.line_in_div);

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
    let start_citation = crate::db::models::citation(base_abbrev, first.div1, first.div2, first.line_in_div);
    let end_citation = crate::db::models::citation(base_abbrev, last.div1, last.div2, last.line_in_div);

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
    crate::claude::send_message(&TEACHER_GENERIC_PROMPT, user_message, model).await
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

/// Resolve a free-text echo quote (emitted by the LLM, with no id/citation) to
/// an authoritative `Title act.scene` citation.
///
/// Hardened: the quote is matched against `normalized_text` (the same
/// normalization the DB column is built with — `text_file_map::normalize`), so a
/// quote that differs only in punctuation/case/diacritics still matches. The old
/// `canonical_text =` exact match was punctuation-sensitive (so it usually
/// missed and fell through to a fuzzy `LIKE '%quote%'` that returned an
/// ARBITRARY first row — the wrong-line bug).
///
/// Ambiguity guard: a citation is only returned when the quote resolves to a
/// SINGLE distinct `(work_abbrev, div1, div2)` (excluding the source work and
/// any `-Amb` edition). If the phrase appears in more than one distinct
/// work/scene — a reused line, or a short quote — the citation would be a guess,
/// so we return `None` and the caller flags the gloss `(unverified)`. The fuzzy
/// substring branch is gone entirely.
fn lookup_citation(
    conn: &rusqlite::Connection,
    quote: &str,
    source_work: &str,
) -> Option<String> {
    let base_source = source_work.strip_suffix("-Amb").unwrap_or(source_work);
    let normalized = crate::text_file_map::normalize(quote);
    if normalized.is_empty() {
        return None;
    }

    // Collect the DISTINCT (work, scene) tuples this normalized quote matches,
    // excluding the source work and its -Amb companion. More than one ⇒
    // ambiguous ⇒ no authoritative citation.
    //   ?2 = base abbrev; ?3 = the raw source_work in case it is already the
    //   -Amb form (so both the base and -Amb editions of the source are excluded).
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT work_abbrev, div1, div2 FROM line_mapping \
             WHERE normalized_text = ?1 \
               AND work_abbrev != ?2 AND work_abbrev != ?3 \
               AND work_abbrev NOT LIKE '%-Amb'",
        )
        .ok()?;
    // Propagate any row-deserialization error as None rather than silently
    // dropping a row: a dropped row could collapse an AMBIGUOUS multi-match into
    // an apparent unique match and emit a wrong citation — the exact failure
    // this hardening prevents.
    let matches: Vec<(String, i64, i64)> = stmt
        .query_map(
            rusqlite::params![normalized, base_source, source_work],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    // Unambiguous match only.
    if let [(abbrev, act, scene)] = matches.as_slice() {
        let title = abbrev_to_title(conn, abbrev);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teacher_generic_has_no_unfilled_placeholder() {
        // Assembled from the live DB (active version) or the compiled fallback —
        // either way the IPA placeholder must be filled. Anchor on the prompt's
        // stable opening line, not on version-specific bullet wording (the body
        // is edited across versions; the role line is invariant).
        let p = &*TEACHER_GENERIC_PROMPT;
        assert!(!p.contains("{ipa_rules}"), "ipa_rules token left unfilled");
        assert!(!p.contains("{}"), "positional placeholder left unfilled");
        assert!(p.contains("You are a performance-focused teacher"));
    }

    #[test]
    fn all_templated_gloss_prompts_fill_their_placeholder() {
        // Every templated gloss prompt interpolates an IPA fragment into its
        // {ipa_rules}/{} slot; none may leave the slot unfilled, whether the
        // template came from the DB master or the compiled fallback.
        for (name, p) in [
            ("user-question", &*USER_QUESTION_PROMPT),
            ("inner-monologue", &*INNER_MONOLOGUE_PROMPT),
            ("inner-monologue-add", &*INNER_MONOLOGUE_ADD_PROMPT),
            ("inner-monologue-edit", &*INNER_MONOLOGUE_EDIT_PROMPT),
            ("edit", &*EDIT_GLOSS_PROMPT),
            ("teacher-generic", &*TEACHER_GENERIC_PROMPT),
        ] {
            assert!(!p.contains("{ipa_rules}"), "{name}: ipa_rules token left unfilled");
            assert!(!p.contains("{}"), "{name}: positional placeholder left unfilled");
            assert!(!p.is_empty(), "{name}: assembled prompt is empty");
        }
    }

    #[test]
    fn reader_gloss_prompts_non_empty_and_no_ipa_slot() {
        for (name, p) in [
            ("reader-gloss", &*READER_GLOSS_PROMPT),
            ("reader-gloss-question", &*READER_GLOSS_QUESTION_PROMPT),
            ("reader-gloss-edit", &*READER_GLOSS_EDIT_PROMPT),
        ] {
            assert!(!p.is_empty(), "{name}: assembled prompt is empty");
            assert!(!p.contains("{ipa_rules}"), "{name}: must not contain ipa slot");
            assert!(!p.contains("{}"), "{name}: must not contain positional slot");
        }
    }

    // ---- lookup_citation hardening ----

    /// Build a tiny in-memory lit.db subset: `works(abbrev,title)` +
    /// `line_mapping(work_abbrev,div1,div2,normalized_text)`.
    fn citation_test_db(rows: &[(&str, &str, i64, i64, &str)]) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE works (abbrev TEXT, title TEXT);
             CREATE TABLE line_mapping (work_abbrev TEXT, div1 INTEGER, div2 INTEGER, \
                 canonical_text TEXT, normalized_text TEXT);",
        )
        .unwrap();
        let mut seen = std::collections::HashSet::new();
        for (abbrev, title, div1, div2, text) in rows {
            if seen.insert(*abbrev) {
                conn.execute(
                    "INSERT INTO works (abbrev, title) VALUES (?1, ?2)",
                    rusqlite::params![abbrev, title],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO line_mapping (work_abbrev, div1, div2, canonical_text, normalized_text) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![abbrev, div1, div2, text, crate::text_file_map::normalize(text)],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn lookup_citation_unique_match_returns_title_and_scene() {
        let conn = citation_test_db(&[
            ("Ham", "Hamlet", 3, 1, "To be, or not to be"),
        ]);
        // Quote differs in punctuation/case — normalized_text match must still hit.
        let got = lookup_citation(&conn, "to be or not to be", "Mac");
        assert_eq!(got, Some("Hamlet 3.1".to_string()));
    }

    #[test]
    fn lookup_citation_ambiguous_across_works_returns_none() {
        // Same phrase in two distinct works → the citation is a guess → None.
        let conn = citation_test_db(&[
            ("Ham", "Hamlet", 1, 2, "O, that this too too solid flesh"),
            ("Mac", "Macbeth", 5, 5, "O, that this too too solid flesh"),
        ]);
        let got = lookup_citation(&conn, "O that this too too solid flesh", "Lr");
        assert_eq!(got, None);
    }

    #[test]
    fn lookup_citation_excludes_source_work_and_amb() {
        // Only the source work (and its -Amb) carry the line → nothing to verify
        // against → None (not a self-citation).
        let conn = citation_test_db(&[
            ("Mac", "Macbeth", 1, 1, "When shall we three meet again"),
            ("Mac-Amb", "Macbeth", 1, 1, "When shall we three meet again"),
        ]);
        let got = lookup_citation(&conn, "When shall we three meet again", "Mac");
        assert_eq!(got, None);
    }

    #[test]
    fn lookup_citation_no_match_returns_none() {
        let conn = citation_test_db(&[("Ham", "Hamlet", 3, 1, "To be, or not to be")]);
        assert_eq!(lookup_citation(&conn, "a phrase not in any work", "Mac"), None);
    }

    #[test]
    fn lookup_citation_same_work_multiple_scenes_is_ambiguous() {
        // One work, two different scenes carry the line → which scene is a guess
        // → None. (Distinct (work,div1,div2) tuples, even within a work.)
        let conn = citation_test_db(&[
            ("Ham", "Hamlet", 1, 1, "Who's there"),
            ("Ham", "Hamlet", 4, 2, "Who's there"),
        ]);
        assert_eq!(lookup_citation(&conn, "Who's there", "Mac"), None);
    }
}

#[cfg(test)]
mod normalize_parity_check {
    /// `lookup_citation` matches an echo quote against the DB's `normalized_text`
    /// by running `text_file_map::normalize` on the quote. That only works if the
    /// Rust normalizer reproduces how the DB column was built. Echo quotes are
    /// spoken verse lines (never bracketed stage directions), so guard parity on
    /// representative spoken lines sampled from lit.db `normalized_text`.
    #[test]
    fn rust_normalize_matches_db_samples() {
        let cases = [
            (
                "Enter King Henry, Duke Humphrey of Gloucester,",
                "enter king henry duke humphrey of gloucester",
            ),
            (
                "Salisbury, Warwick, and Cardinal Beaufort, on the one",
                "salisbury warwick and cardinal beaufort on the one",
            ),
        ];
        for (raw, expected_db) in cases {
            assert_eq!(crate::text_file_map::normalize(raw), expected_db, "raw={raw:?}");
        }
    }
}
