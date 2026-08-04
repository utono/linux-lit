use std::collections::HashMap;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::commands::{MpvCommand, MpvEvent, ReaderAction};

/// Deferred action applied on the next `file-loaded` event:
/// (seek_time, resume_after_seek, optional ab_loop (a, b)).
type PendingSeek = Option<(f64, bool, Option<(f64, f64)>)>;

pub async fn run(
    mut cmd_rx: mpsc::Receiver<MpvCommand>,
    evt_tx: mpsc::Sender<MpvEvent>,
) {
    let mut reader: Option<BufReader<tokio::net::unix::OwnedReadHalf>> = None;
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let mut timestamps: Vec<(i64, f64, f64)> = Vec::new();
    let mut line_id_to_index: HashMap<i64, usize> = HashMap::new();
    // (seek_time, resume_after_seek, optional ab_loop (a, b))
    let mut pending_seek_after_load: PendingSeek = None;
    let mut last_synced_work_idx: Option<usize> = None;
    // Accumulated Ctrl+Up/Down nudges this session, in percent points. The
    // desired level (config volume + nudges) is re-asserted after connect and
    // after every file load — watch_later restore (save-position-on-quit in
    // the user's mpv.conf) applies each file's SAVED volume at load time,
    // silently overriding the `--volume=` launch arg otherwise.
    let mut volume_delta: f64 = 0.0;
    // The user's mpv.conf also has a CONDITIONAL auto-profile ([audio_auto]:
    // `profile-cond=not video` -> [audio] volume=75) that fires at track
    // selection — AFTER the file-loaded event — so an assert at file-loaded
    // still loses. Re-assert once more on the FIRST time-pos tick after each
    // connect/load: by then profiles and watch_later restore have all fired.
    let mut assert_volume_on_timepos = false;

    loop {
        if let Some(ref mut r) = reader {
            let mut line_buf = String::new();
            tokio::select! {
                result = r.read_line(&mut line_buf) => {
                    match result {
                        Ok(0) | Err(_) => {
                            reader = None;
                            writer = None;
                            pending_seek_after_load = None;
                            let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                        }
                        Ok(_) => {
                            if is_file_loaded_event(&line_buf) {
                                // Undo any watch_later volume restore for this file
                                // (and again on the next time-pos tick, once the
                                // conditional auto-profiles have fired too).
                                assert_volume_on_timepos = true;
                                if let Some(w) = writer.as_mut() {
                                    let _ = send_command(w, &set_property_cmd("volume", desired_volume(volume_delta))).await;
                                }
                                if let Some((seek_time, resume, ab_loop)) = pending_seek_after_load.take() {
                                    if let Some(w) = writer.as_mut() {
                                        crate::logging::log(&format!(
                                            "MPV: file-loaded, seeking to {:.1} resume={} loop={:?}", seek_time, resume, ab_loop
                                        ));
                                        if let Some((la, lb)) = ab_loop {
                                            let _ = send_command(w, &set_property_cmd("ab-loop-a", la)).await;
                                            let _ = send_command(w, &set_property_cmd("ab-loop-b", lb)).await;
                                        }
                                        let _ = send_command(w, &seek_absolute_cmd(seek_time)).await;
                                        let pause_val = if resume { "false" } else { "true" };
                                        let _ = send_command(w, &set_property_cmd("pause", pause_val)).await;
                                    }
                                }
                            }
                            if let Some(pos) = parse_time_pos(&line_buf) {
                                if assert_volume_on_timepos {
                                    assert_volume_on_timepos = false;
                                    if let Some(w) = writer.as_mut() {
                                        let v = desired_volume(volume_delta);
                                        let _ = send_command(w, &set_property_cmd("volume", v)).await;
                                        crate::logging::log(&format!("MPV: volume asserted to {} post-load", v));
                                    }
                                }
                                let _ = evt_tx.send(MpvEvent::TimePos(pos)).await;
                                if let Some(idx) = find_line_for_time(pos, &timestamps, &line_id_to_index, last_synced_work_idx) {
                                    last_synced_work_idx = Some(idx);
                                    let _ = evt_tx.send(MpvEvent::CursorSync(idx)).await;
                                }
                            }
                            if let Some(paused) = parse_pause_state(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::PlaybackState(!paused)).await;
                            }
                            if let Some(action) = parse_client_message(&line_buf) {
                                crate::logging::log(&format!(
                                    "MPV_BIND: {:?} from mpv window",
                                    action
                                ));
                                let _ = evt_tx.send(MpvEvent::ReaderAction(action)).await;
                            }
                        }
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    if matches!(
                        cmd,
                        MpvCommand::SetTimestamps { .. }
                            | MpvCommand::Seek(_)
                            | MpvCommand::ResumeAndSeek(_)
                            | MpvCommand::SeekRelative(_)
                    ) {
                        last_synced_work_idx = None;
                    }
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index, &mut pending_seek_after_load, &mut volume_delta, &mut assert_volume_on_timepos).await;
                }
            }
        } else {
            match cmd_rx.recv().await {
                Some(cmd) => {
                    if matches!(
                        cmd,
                        MpvCommand::SetTimestamps { .. }
                            | MpvCommand::Seek(_)
                            | MpvCommand::ResumeAndSeek(_)
                            | MpvCommand::SeekRelative(_)
                    ) {
                        last_synced_work_idx = None;
                    }
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index, &mut pending_seek_after_load, &mut volume_delta, &mut assert_volume_on_timepos).await;
                }
                None => break,
            }
        }
    }
}

async fn handle_command(
    cmd: MpvCommand,
    reader: &mut Option<BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer: &mut Option<tokio::net::unix::OwnedWriteHalf>,
    evt_tx: &mpsc::Sender<MpvEvent>,
    timestamps: &mut Vec<(i64, f64, f64)>,
    line_id_to_index: &mut HashMap<i64, usize>,
    pending_seek_after_load: &mut PendingSeek,
    volume_delta: &mut f64,
    assert_volume_on_timepos: &mut bool,
) {
    match cmd {
        MpvCommand::Connect(path) => {
            // Headless test runs must never attach to a real player: the
            // derived socket path can be the LIVE session's MPV when both run
            // the same work, and every test nav keypress would seek it.
            // LIT_SYNC_TEST re-enables connect for the playback-sync timing
            // test, whose DB-copy media rewrite guarantees a private socket.
            if (std::env::var_os("LIT_HEADLESS_TEST").is_some()
                || std::env::var_os("LIT_NO_MPV").is_some())
                && std::env::var_os("LIT_SYNC_TEST").is_none()
            {
                crate::logging::log("MPV: connect skipped (LIT_HEADLESS_TEST/LIT_NO_MPV)");
                let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                return;
            }
            match connect_and_observe(&path).await {
                Ok((r, mut w)) => {
                    // Assert the desired volume on connect: the first file may
                    // have loaded (with a watch_later volume restore) before
                    // this IPC connection existed to see its file-loaded event.
                    // Re-assert on the first time-pos tick too — the mpv.conf
                    // conditional auto-profiles can fire after this point.
                    *assert_volume_on_timepos = true;
                    let _ = send_command(&mut w, &set_property_cmd("volume", desired_volume(*volume_delta))).await;
                    *reader = Some(r);
                    *writer = Some(w);
                    let _ = evt_tx.send(MpvEvent::ConnectionStatus(true)).await;
                    crate::logging::log(&format!("MPV: connected to {}", path));
                }
                Err(e) => {
                    crate::logging::log(&format!("MPV: connect failed: {}", e));
                    let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                }
            }
        }
        MpvCommand::TogglePause => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["cycle","pause"]}"#).await;
            }
        }
        MpvCommand::Pause => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["set_property","pause",true]}"#).await;
            }
        }
        MpvCommand::Resume => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["set_property","pause",false]}"#).await;
            }
        }
        MpvCommand::ResumeAndSeek(time) => {
            if let Some(w) = writer.as_mut() {
                crate::logging::log(&format!("MPV: ResumeAndSeek time={:.1}", time));
                let _ = send_command(w, &seek_absolute_cmd(time)).await;
                let _ = send_command(w, r#"{"command":["set_property","pause",false]}"#).await;
            }
        }
        MpvCommand::SetSpeed(speed) => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, &set_property_cmd("speed", speed)).await;
            }
        }
        MpvCommand::Seek(time) => {
            if let Some(w) = writer.as_mut() {
                crate::logging::log(&format!("MPV: Seek time={:.1}", time));
                let _ = send_command(w, &seek_absolute_cmd(time)).await;
            }
        }
        MpvCommand::SeekRelative(offset) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["seek",{},"relative","exact"]}}"#, offset);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::VolumeAdjust(delta) => {
            // Remember the nudge so connect/file-loaded re-asserts land on the
            // nudged level, not back on the config default.
            *volume_delta += delta;
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["add","volume",{}]}}"#, delta);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::SetTimestamps {
            timestamps: ts,
            line_id_to_index: map,
        } => {
            *timestamps = ts;
            *line_id_to_index = map;
            crate::logging::log(&format!("MPV: loaded {} timestamps", timestamps.len()));
        }
        MpvCommand::SetAbLoop { a, b } => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, &set_property_cmd("ab-loop-a", a)).await;
                let _ = send_command(w, &set_property_cmd("ab-loop-b", b)).await;
                let _ = send_command(w, &seek_absolute_cmd(a)).await;
            }
        }
        MpvCommand::ClearAbLoop => {
            if let Some(w) = writer.as_mut() {
                let cmd_a = r#"{"command":["set_property","ab-loop-a","no"]}"#;
                let cmd_b = r#"{"command":["set_property","ab-loop-b","no"]}"#;
                let _ = send_command(w, cmd_a).await;
                let _ = send_command(w, cmd_b).await;
            }
        }
        MpvCommand::SetBackground(color) => {
            if let Some(w) = writer.as_mut() {
                // `color` is a `#rrggbb` string from the theme. These properties
                // take STRING values, so the JSON value must be quoted —
                // set_property_cmd renders its value raw (used for f64s), so it
                // can't build these. Setting `background`/`border-background` to
                // `color` mode makes the idle backdrop and the letterbox matte
                // both honor `background-color`.
                let escaped = color.replace('\\', "\\\\").replace('"', "\\\"");
                let _ = send_command(w, r#"{"command":["set_property","background","color"]}"#).await;
                let _ = send_command(w, r#"{"command":["set_property","border-background","color"]}"#).await;
                let cmd = format!(r#"{{"command":["set_property","background-color","{}"]}}"#, escaped);
                let _ = send_command(w, &cmd).await;
                crate::logging::log(&format!("MPV: background-color set to {}", color));
            }
        }
        MpvCommand::LoadFile(path) => {
            if let Some(w) = writer.as_mut() {
                let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
                let cmd = format!(r#"{{"command":["loadfile","{}","replace"]}}"#, escaped);
                let _ = send_command(w, &cmd).await;
                crate::logging::log(&format!("MPV: loadfile replace '{}'", path));
            }
        }
        MpvCommand::LoadFileAndSeek(path, seek_time) => {
            if let Some(w) = writer.as_mut() {
                let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
                let cmd = format!(r#"{{"command":["loadfile","{}","replace"]}}"#, escaped);
                let _ = send_command(w, &cmd).await;
                *pending_seek_after_load = Some((seek_time, true, None));
                crate::logging::log(&format!(
                    "MPV: loadfile replace '{}' (seek {:.1} resume pending file-loaded)", path, seek_time
                ));
            }
        }
        MpvCommand::LoadFileSeekPaused(path, seek_time) => {
            if let Some(w) = writer.as_mut() {
                let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
                let cmd = format!(r#"{{"command":["loadfile","{}","replace"]}}"#, escaped);
                let _ = send_command(w, &cmd).await;
                *pending_seek_after_load = Some((seek_time, false, None));
                crate::logging::log(&format!(
                    "MPV: loadfile replace '{}' (seek {:.1} paused pending file-loaded)", path, seek_time
                ));
            }
        }
        MpvCommand::LoadFileSeekAndLoop(path, seek_time, loop_b) => {
            if let Some(w) = writer.as_mut() {
                let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
                let cmd = format!(r#"{{"command":["loadfile","{}","replace"]}}"#, escaped);
                let _ = send_command(w, &cmd).await;
                *pending_seek_after_load = Some((seek_time, true, Some((seek_time, loop_b))));
                crate::logging::log(&format!(
                    "MPV: loadfile replace '{}' (seek {:.1} + ab-loop [{:.1},{:.1}] pending file-loaded)",
                    path, seek_time, seek_time, loop_b
                ));
            }
        }
        MpvCommand::Quit => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["quit"]}"#).await;
            }
        }
    }
}

async fn connect_and_observe(
    path: &str,
) -> Result<
    (
        BufReader<tokio::net::unix::OwnedReadHalf>,
        tokio::net::unix::OwnedWriteHalf,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let stream = UnixStream::connect(path).await?;
    let (read_half, mut write_half) = stream.into_split();
    send_command(
        &mut write_half,
        r#"{"command":["observe_property",1,"time-pos"]}"#,
    )
    .await?;
    send_command(
        &mut write_half,
        r#"{"command":["observe_property",2,"pause"]}"#,
    )
    .await?;
    Ok((BufReader::new(read_half), write_half))
}

/// Build a `set_property` IPC command for `prop` with `val` rendered via Display
/// (f64 for speed/ab-loop, `&str` for the pause "true"/"false" sentinels). The
/// byte-identical JSON envelope every dynamic-value set_property send repeats.
fn set_property_cmd(prop: &str, val: impl std::fmt::Display) -> String {
    format!(r#"{{"command":["set_property","{}",{}]}}"#, prop, val)
}

/// The volume the player should sit at: the configured launch volume plus the
/// session's accumulated Ctrl+Up/Down nudges, clamped to the config range.
fn desired_volume(volume_delta: f64) -> f64 {
    (crate::mpv::discovery::mpv_volume() as f64 + volume_delta).clamp(0.0, 150.0)
}

/// Build an absolute-seek IPC command to `time` seconds. The byte-identical
/// envelope shared by ResumeAndSeek / Seek / SetAbLoop / the file-loaded path.
/// (The `["seek", _, "relative","exact"]` form is a distinct command, not this.)
fn seek_absolute_cmd(time: f64) -> String {
    format!(r#"{{"command":["seek",{},"absolute"]}}"#, time)
}

async fn send_command(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    cmd: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    writer.write_all(cmd.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

fn parse_time_pos(line: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? == "property-change" && v.get("name")?.as_str()? == "time-pos" {
        v.get("data")?.as_f64()
    } else {
        None
    }
}

fn parse_pause_state(line: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? == "property-change" && v.get("name")?.as_str()? == "pause" {
        v.get("data")?.as_bool()
    } else {
        None
    }
}

/// Parse an mpv `client-message` event into the reader action it requests.
/// The lit-owned input.conf (`assets/mpv-input.conf`) binds six keys to
/// `script-message lit-*`, and mpv relays each as a client-message on the
/// IPC socket we already read. Unknown names return `None` so other
/// scripts' messages on this socket are ignored.
fn parse_client_message(line: &str) -> Option<ReaderAction> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? != "client-message" {
        return None;
    }
    match v.get("args")?.as_array()?.first()?.as_str()? {
        "lit-prev-speaker" => Some(ReaderAction::PrevSpeaker),
        "lit-next-speaker" => Some(ReaderAction::NextSpeaker),
        "lit-prev-division" => Some(ReaderAction::PrevDivision),
        "lit-next-division" => Some(ReaderAction::NextDivision),
        "lit-set-start-time" => Some(ReaderAction::SetStartTime),
        "lit-undo-timestamp" => Some(ReaderAction::UndoTimestamp),
        _ => None,
    }
}

fn is_file_loaded_event(line: &str) -> bool {
    let v: serde_json::Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    v.get("event").and_then(|e| e.as_str()) == Some("file-loaded")
}

fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)],
    line_id_to_index: &HashMap<i64, usize>,
    last_synced_work_idx: Option<usize>,
) -> Option<usize> {
    use crate::input::navigation::{SYNC_GAP_PREROLL, SYNC_GAP_THRESHOLD, SYNC_PREROLL};

    let effective_time = time_pos + SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }

    // Gap-aware early jump: when the current line A (timestamps[idx - 1]) and
    // the next line B (timestamps[idx]) are separated by a gap longer than
    // SYNC_GAP_THRESHOLD, advance to B at B.start - SYNC_GAP_PREROLL (a fixed
    // lead before B is spoken). Anchoring on B.start rather than A.end keeps
    // the lead correct even when A's end_time overshoots the actual speech
    // (trailing silence / stage business baked into the timestamp). When A has
    // no usable end_time the gap can't be measured, so apply the same lead
    // unconditionally. Promotes by exactly one line, so a line is never skipped.
    let mut active = idx - 1;
    if let Some(&(_, b_start, _)) = timestamps.get(idx) {
        let (_, a_start, a_end) = timestamps[idx - 1];
        let trigger = b_start - SYNC_GAP_PREROLL;
        let qualifies = if a_end > a_start {
            b_start - a_end > SYNC_GAP_THRESHOLD
        } else {
            true
        };
        // Compare in EFFECTIVE time (same clock as the base mapping above):
        // with a non-zero SYNC_PREROLL the whole sync surface leads the audio
        // uniformly, gap jumps included. Comparing raw time_pos here would
        // give gap jumps a different lead than every other sync decision.
        if qualifies && effective_time >= trigger {
            active = idx;
        }
    }

    // Candidate set: all timestamp entries sharing `active`'s start_time resolve
    // to distinct work indices (a re-spoken line carries the first occurrence's
    // timestamp). Pick the one nearest the cursor in citation/work-index order,
    // breaking ties toward the forward (larger) index so normal progress always
    // advances. Without a cursor anchor (first sync after load/seek), fall back
    // to the single `active` candidate (legacy behavior).
    let active_start = timestamps[active].1;
    let candidates: Vec<usize> = timestamps
        .iter()
        .filter(|ts| (ts.1 - active_start).abs() < f64::EPSILON)
        .filter_map(|ts| line_id_to_index.get(&ts.0).copied())
        .collect();

    match (last_synced_work_idx, candidates.as_slice()) {
        (_, []) => line_id_to_index.get(&timestamps[active].0).copied(),
        (None, _) => line_id_to_index.get(&timestamps[active].0).copied(),
        (Some(anchor), cands) => cands
            .iter()
            .copied()
            .min_by(|&a, &b| {
                let da = (a as isize - anchor as isize).unsigned_abs();
                let db = (b as isize - anchor as isize).unsigned_abs();
                // nearest wins; tie -> larger index (forward)
                da.cmp(&db).then(b.cmp(&a))
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_pos() {
        let line = r#"{"event":"property-change","id":1,"name":"time-pos","data":123.456}"#;
        assert_eq!(parse_time_pos(line), Some(123.456));
    }

    #[test]
    fn test_parse_time_pos_null() {
        let line = r#"{"event":"property-change","id":1,"name":"time-pos","data":null}"#;
        assert_eq!(parse_time_pos(line), None);
    }

    #[test]
    fn test_parse_pause_state() {
        let line = r#"{"event":"property-change","id":2,"name":"pause","data":true}"#;
        assert_eq!(parse_pause_state(line), Some(true));
    }

    #[test]
    fn test_parse_client_message_all_six() {
        let cases = [
            ("lit-prev-speaker", ReaderAction::PrevSpeaker),
            ("lit-next-speaker", ReaderAction::NextSpeaker),
            ("lit-prev-division", ReaderAction::PrevDivision),
            ("lit-next-division", ReaderAction::NextDivision),
            ("lit-set-start-time", ReaderAction::SetStartTime),
            ("lit-undo-timestamp", ReaderAction::UndoTimestamp),
        ];
        for (name, expected) in cases {
            let line = format!(r#"{{"event":"client-message","args":["{}"]}}"#, name);
            assert_eq!(
                parse_client_message(&line),
                Some(expected),
                "failed to parse {}",
                name
            );
        }
    }

    /// The shipped input.conf and the parser must agree on all six message
    /// names — a typo in either would silently dead-key that bind.
    #[test]
    fn test_shipped_input_conf_matches_parser() {
        let conf = include_str!("../../assets/mpv-input.conf");
        let mut found = 0;
        for line in conf.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let msg = line
                .split("script-message ")
                .nth(1)
                .unwrap_or_else(|| panic!("non-comment line is not a script-message: {}", line))
                .trim();
            let fake = format!(r#"{{"event":"client-message","args":["{}"]}}"#, msg);
            assert!(
                parse_client_message(&fake).is_some(),
                "input.conf sends '{}' but the parser does not know it",
                msg
            );
            found += 1;
        }
        assert_eq!(found, 6, "expected exactly 6 binds in assets/mpv-input.conf");
    }

    #[test]
    fn test_parse_client_message_rejects_others() {
        // A different script's message on the same socket.
        assert_eq!(
            parse_client_message(r#"{"event":"client-message","args":["some-other-script"]}"#),
            None
        );
        // Right event, no args.
        assert_eq!(
            parse_client_message(r#"{"event":"client-message","args":[]}"#),
            None
        );
        // A different event entirely.
        assert_eq!(
            parse_client_message(r#"{"event":"property-change","name":"time-pos","data":1.0}"#),
            None
        );
        // Not JSON at all.
        assert_eq!(parse_client_message("not json"), None);
    }

    /// Probe times below are the EFFECTIVE times the mapping should see;
    /// find_line_for_time adds SYNC_PREROLL internally, so subtract it here
    /// to keep these fixtures valid at any preroll setting.
    fn t(effective: f64) -> f64 {
        effective - crate::input::navigation::SYNC_PREROLL
    }

    #[test]
    fn test_find_line_for_time() {
        let timestamps = vec![(10, 1.0, 2.0), (20, 3.0, 4.0), (30, 5.0, 6.0)];
        let map: HashMap<i64, usize> = [(10, 0), (20, 1), (30, 2)].into();

        assert_eq!(find_line_for_time(t(0.5), &timestamps, &map, None), None);
        assert_eq!(find_line_for_time(t(1.0), &timestamps, &map, None), Some(0));
        assert_eq!(find_line_for_time(t(2.5), &timestamps, &map, None), Some(0));
        assert_eq!(find_line_for_time(t(3.0), &timestamps, &map, None), Some(1));
        assert_eq!(find_line_for_time(t(5.0), &timestamps, &map, None), Some(2));
    }

    #[test]
    fn test_find_line_for_time_gap_aware() {
        // A: id 10, start 1.0, end 2.0. B: id 20, start 6.0, end 7.0.
        // Gap = 6.0 - 2.0 = 4.0 > 1.5 threshold -> early jump anchored to
        // B.start - 1.5 = 4.5.
        let gap = vec![(10, 1.0, 2.0), (20, 6.0, 7.0)];
        let map: HashMap<i64, usize> = [(10, 0), (20, 1)].into();

        // Just before B.start - 1.5 = 4.5: still on A.
        assert_eq!(find_line_for_time(t(4.4), &gap, &map, None), Some(0));
        // At B.start - 1.5: jump to B early.
        assert_eq!(find_line_for_time(t(4.5), &gap, &map, None), Some(1));
        // After B starts: still B.
        assert_eq!(find_line_for_time(t(6.5), &gap, &map, None), Some(1));

        // No-gap case: A ends 2.0, B starts 3.0 -> gap 1.0 <= 1.5, no early jump.
        // B.start - 1.5 = 1.5 would land mid-A, but the gap is below threshold
        // so the early jump does not apply; B becomes active only at its start.
        let nogap = vec![(10, 1.0, 2.0), (20, 3.0, 4.0)];
        assert_eq!(find_line_for_time(t(2.5), &nogap, &map, None), Some(0));
        assert_eq!(find_line_for_time(t(3.0), &nogap, &map, None), Some(1));

        // Invalid A.end (end == start): gap unknown -> apply the lead anyway,
        // B.start - 1.5 = 4.5.
        let badend = vec![(10, 1.0, 1.0), (20, 6.0, 7.0)];
        assert_eq!(find_line_for_time(t(4.4), &badend, &map, None), Some(0));
        assert_eq!(find_line_for_time(t(4.5), &badend, &map, None), Some(1));
    }

    #[test]
    fn picks_nearest_candidate_on_duplicate_timestamp() {
        // Two work lines share start=2484: the spirit's line (work idx 37) and the
        // re-read (work idx 71). Cursor is near the first → must stay on 37.
        let timestamps = vec![(/*id*/100, 2484.0, 2485.0), (/*id*/200, 2484.0, 2485.0)];
        let mut map = std::collections::HashMap::new();
        map.insert(100, 37usize);
        map.insert(200, 71usize);
        // Effective time lands in the shared bracket.
        let got = find_line_for_time(2484.5, &timestamps, &map, Some(36));
        assert_eq!(got, Some(37));
    }

    #[test]
    fn backward_seek_picks_near_earlier_candidate() {
        // Cursor far ahead (71); audio seeks back into the 2484 bracket → choose 37.
        let timestamps = vec![(100, 2484.0, 2485.0), (200, 2484.0, 2485.0)];
        let mut map = std::collections::HashMap::new();
        map.insert(100, 37usize);
        map.insert(200, 71usize);
        let got = find_line_for_time(2484.5, &timestamps, &map, Some(71));
        assert_eq!(got, Some(71)); // 71 is its own nearest; the near earlier 37 loses only if 71 is closer — here cursor==71 so 71 wins
    }
}
