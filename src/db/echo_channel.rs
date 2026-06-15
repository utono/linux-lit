//! Which class of cross-work echo an overlay shows.
//!
//! The channel is a pure data filter on `echo_links.echo_work_abbrev`:
//! BCP editions are registered as works `BCP1549` / `BCP1559` / `BCP1662`, so a
//! BCP echo row matches `echo_work_abbrev LIKE 'BCP%'`. There is no schema
//! column — the BCP pipeline guarantees the prefix on every row it writes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EchoChannel {
    /// Book of Common Prayer inner-monologue echoes (echo_work_abbrev LIKE 'BCP%').
    Bcp,
    /// Shakespeare-to-Shakespeare dramatic echoes (everything else).
    Shakespeare,
}

impl EchoChannel {
    /// SQL predicate (no leading AND) selecting this channel's echo_links rows.
    /// `echo_work_abbrev` is NOT NULL, so LIKE is well-defined.
    pub fn sql_predicate(self) -> &'static str {
        match self {
            EchoChannel::Bcp => "echo_work_abbrev LIKE 'BCP%'",
            EchoChannel::Shakespeare => "echo_work_abbrev NOT LIKE 'BCP%'",
        }
    }
}
