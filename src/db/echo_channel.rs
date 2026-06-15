//! Which class of cross-work echo an overlay shows.
//!
//! A Book of Common Prayer pairing exists in both reading directions:
//!   - Shakespeare -> BCP: turn is a Shakespeare work, echo is a `BCP*` work.
//!   - BCP -> Shakespeare: turn is a `BCP*` work, echo is a Shakespeare work.
//! So a pair belongs to the BCP channel when EITHER side is a BCP work. The BCP
//! editions are registered as works `BCP1549` / `BCP1559` / `BCP1662`. The
//! channel is a pure data filter (no schema column); the BCP pipeline guarantees
//! a `BCP*` work on exactly one side of every BCP pair.
//!
//! The predicate references two aliases — `t` (echo_turns, for the turn's
//! `work_abbrev`) and `l` (echo_links, for `echo_work_abbrev`). Every query that
//! uses it must expose both aliases (load_echo_links JOINs echo_turns to do so).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EchoChannel {
    /// BCP pairings, either direction: t.work_abbrev or l.echo_work_abbrev is BCP.
    Bcp,
    /// Shakespeare-to-Shakespeare: neither side is BCP.
    Shakespeare,
}

impl EchoChannel {
    /// SQL predicate (no leading AND) selecting this channel's pairs. References
    /// `t.work_abbrev` (the turn) and `l.echo_work_abbrev` (the link) — both
    /// columns are NOT NULL, so LIKE is well-defined. Callers must alias
    /// echo_turns AS t and echo_links AS l.
    pub fn sql_predicate(self) -> &'static str {
        match self {
            EchoChannel::Bcp => {
                "(t.work_abbrev LIKE 'BCP%' OR l.echo_work_abbrev LIKE 'BCP%')"
            }
            EchoChannel::Shakespeare => {
                "(t.work_abbrev NOT LIKE 'BCP%' AND l.echo_work_abbrev NOT LIKE 'BCP%')"
            }
        }
    }
}
