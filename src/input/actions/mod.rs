//! Verbs invoked from keymap.rs match arms. Each submodule groups verbs by
//! feature area. Phase 2 (F2) adds the Action enum here and re-exports the
//! verbs as the dispatch target.

pub mod bookmarks;
pub mod concordance;
pub mod pickers;
pub mod settings;
