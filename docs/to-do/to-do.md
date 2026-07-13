# linux-lit to-do

Running list of reader bugs and feature requests. Mark completed items with a
leading `[X]`; never delete them.

- Overlay `/` regex search shows no positional counter: `n`/`N` step matches
  within the entry and the current match is tinted, but the footer "match N of
  M" tracks the `f`-term set, not the active `/` regex, so there's no `[2/5]`
  feedback. Deliberate per the overlay-search design (per-entry match tally was
  a non-goal); revisit if positional feedback is wanted.
