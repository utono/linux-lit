---
name: set-default-voice
description: Use when changing linux-lit's default male or female ElevenLabs narration voice, or when asked which voice is the current default for a gender
argument-hint: male <VoiceName> | female <VoiceName>
---

# Set Default Voice

Change linux-lit's default **male** or **female** ElevenLabs narration voice. The
default female voice is also the all-prose narrator. A new voice must already
exist as a **custom (generated) voice** in the user's ElevenLabs account.

There are two source-of-truth locations and BOTH must change or the default
silently won't take effect:

1. The `*_VOICE_ID` const in `src/elevenlabs.rs` (male = `BENEDICK_VOICE_ID`,
   female = `IMOGEN_VOICE_ID`). A fresh `lit.db` re-seeds from these.
2. The live `voice_catalog` rows in `~/utono/litdb/data/lit.db`. Seeding is
   `INSERT OR IGNORE` on `(gender, age_min, age_max, role)`, so existing rows are
   **never overwritten by a rebuild** — `resolve_default_voice` reads the live
   catalog FIRST, so without the DB update verse keeps resolving to the old voice.

## No argument → print current defaults

Run the helper with no args; it prints both current defaults (const value + live
DB rows) and the usage line:

```bash
.claude/skills/set-default-voice/set-default-voice.sh
```

## With argument → set a default

`/set-default-voice female Imogen` or `/set-default-voice male Benedick`.

1. **Resolve the voice via the MCP server (do NOT guess the ID).**
   - `mcp__ElevenLabs__search_voices` with `search: "<VoiceName>"`.
   - The name may match MORE THAN ONE voice (e.g. two "Imogen"s). If so, show
     the user the candidates (id, name, category) and ask which id, or accept an
     id the user gives directly. Confirm the exact id with
     `mcp__ElevenLabs__get_voice`.
   - The voice MUST be `category: "generated"` (a custom Voice-Design voice).
     A `professional`/`premade` voice can be rejected on the free tier with a 402
     and fall back to Alice — warn the user and stop unless they insist.

2. **Apply both changes** with the resolved id:

   ```bash
   .claude/skills/set-default-voice/set-default-voice.sh <male|female> <VOICE_ID> "<VoiceName>"
   ```

   The script edits the const in `src/elevenlabs.rs` and runs the
   `UPDATE voice_catalog` for that gender's rows in `lit.db` (close linux-lit
   first — a running instance can clobber state on exit).

3. **Rebuild and verify:** `cargo build`, then
   `cargo test --bins elevenlabs resolve`. The script does NOT rename the const
   identifier (e.g. `IMOGEN_VOICE_ID`) — if the voice's *character name* changed,
   also fix the const name, its uses, comments, test names, and
   `docs/guides/elevenlabs-v3-custom-voices.md` by hand (the compiler catches
   missed uses).

## Common mistakes

- **Editing only the const.** The live `voice_catalog` rows win for verse until
  updated — always run the DB update too (the script does both).
- **Trusting a name search blindly.** Two voices can share a name; confirm the
  exact id with `get_voice` before writing it.
- **Using a non-generated voice.** Professional/premade voices 402 on the free
  tier and fall back to Alice.
