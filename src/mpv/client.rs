use std::collections::HashMap;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::commands::{MpvCommand, MpvEvent};

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
                                if let Some((seek_time, resume, ab_loop)) = pending_seek_after_load.take() {
                                    if let Some(w) = writer.as_mut() {
                                        crate::logging::log(&format!(
                                            "MPV: file-loaded, seeking to {:.1} resume={} loop={:?}", seek_time, resume, ab_loop
                                        ));
                                        if let Some((la, lb)) = ab_loop {
                                            let _ = send_command(w, &format!(r#"{{"command":["set_property","ab-loop-a",{}]}}"#, la)).await;
                                            let _ = send_command(w, &format!(r#"{{"command":["set_property","ab-loop-b",{}]}}"#, lb)).await;
                                        }
                                        let cmd = format!(r#"{{"command":["seek",{},"absolute"]}}"#, seek_time);
                                        let _ = send_command(w, &cmd).await;
                                        let pause_val = if resume { "false" } else { "true" };
                                        let _ = send_command(w, &format!(r#"{{"command":["set_property","pause",{}]}}"#, pause_val)).await;
                                    }
                                }
                            }
                            if let Some(pos) = parse_time_pos(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::TimePos(pos)).await;
                                if let Some(idx) = find_line_for_time(pos, &timestamps, &line_id_to_index) {
                                    let _ = evt_tx.send(MpvEvent::CursorSync(idx)).await;
                                }
                            }
                            if let Some(paused) = parse_pause_state(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::PlaybackState(!paused)).await;
                            }
                        }
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index, &mut pending_seek_after_load).await;
                }
            }
        } else {
            match cmd_rx.recv().await {
                Some(cmd) => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index, &mut pending_seek_after_load).await;
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
) {
    match cmd {
        MpvCommand::Connect(path) => {
            match connect_and_observe(&path).await {
                Ok((r, w)) => {
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
        MpvCommand::ResumeAndSeek(time) => {
            if let Some(w) = writer.as_mut() {
                crate::logging::log(&format!("MPV: ResumeAndSeek time={:.1}", time));
                let cmd = format!(r#"{{"command":["seek",{},"absolute"]}}"#, time);
                let _ = send_command(w, &cmd).await;
                let _ = send_command(w, r#"{"command":["set_property","pause",false]}"#).await;
            }
        }
        MpvCommand::SetSpeed(speed) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["set_property","speed",{}]}}"#, speed);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::Seek(time) => {
            if let Some(w) = writer.as_mut() {
                crate::logging::log(&format!("MPV: Seek time={:.1}", time));
                let cmd = format!(r#"{{"command":["seek",{},"absolute"]}}"#, time);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::SeekRelative(offset) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["seek",{},"relative","exact"]}}"#, offset);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::VolumeAdjust(delta) => {
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
                let cmd_a = format!(r#"{{"command":["set_property","ab-loop-a",{}]}}"#, a);
                let cmd_b = format!(r#"{{"command":["set_property","ab-loop-b",{}]}}"#, b);
                let seek = format!(r#"{{"command":["seek",{},"absolute"]}}"#, a);
                let _ = send_command(w, &cmd_a).await;
                let _ = send_command(w, &cmd_b).await;
                let _ = send_command(w, &seek).await;
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
) -> Option<usize> {
    use crate::input::navigation::{
        SYNC_GAP_POST_END, SYNC_GAP_PREROLL, SYNC_GAP_THRESHOLD, SYNC_PREROLL,
    };

    let effective_time = time_pos + SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }

    // Gap-aware early jump: when the current line A (timestamps[idx - 1]) and
    // the next line B (timestamps[idx]) are separated by a gap longer than
    // SYNC_GAP_THRESHOLD, advance to B early. With a valid A.end (end > start)
    // the jump anchors to A.end + SYNC_GAP_POST_END (jump shortly after A
    // finishes, then rest on B through the silence). Without a usable A.end
    // the gap is unknown, so fall back to B.start - SYNC_GAP_PREROLL and apply
    // the early jump unconditionally. Promotes by exactly one line, so a line
    // is never skipped.
    let mut active = idx - 1;
    if let Some(&(_, b_start, _)) = timestamps.get(idx) {
        let (_, a_start, a_end) = timestamps[idx - 1];
        if a_end > a_start {
            let gap = b_start - a_end;
            if gap > SYNC_GAP_THRESHOLD && time_pos >= a_end + SYNC_GAP_POST_END {
                active = idx;
            }
        } else if time_pos >= b_start - SYNC_GAP_PREROLL {
            active = idx;
        }
    }

    let (line_id, _, _) = timestamps[active];
    line_id_to_index.get(&line_id).copied()
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
    fn test_find_line_for_time() {
        let timestamps = vec![(10, 1.0, 2.0), (20, 3.0, 4.0), (30, 5.0, 6.0)];
        let map: HashMap<i64, usize> = [(10, 0), (20, 1), (30, 2)].into();

        assert_eq!(find_line_for_time(0.5, &timestamps, &map), None);
        assert_eq!(find_line_for_time(1.0, &timestamps, &map), Some(0));
        assert_eq!(find_line_for_time(2.5, &timestamps, &map), Some(0));
        assert_eq!(find_line_for_time(3.0, &timestamps, &map), Some(1));
        assert_eq!(find_line_for_time(5.0, &timestamps, &map), Some(2));
    }

    #[test]
    fn test_find_line_for_time_gap_aware() {
        // A: id 10, start 1.0, end 2.0. B: id 20, start 6.0, end 7.0.
        // Gap = 6.0 - 2.0 = 4.0 > 1.5 threshold -> early jump anchored to
        // A.end + 0.2 = 2.2.
        let gap = vec![(10, 1.0, 2.0), (20, 6.0, 7.0)];
        let map: HashMap<i64, usize> = [(10, 0), (20, 1)].into();

        // Just before A.end + 0.2 = 2.2: still on A.
        assert_eq!(find_line_for_time(2.1, &gap, &map), Some(0));
        // At A.end + 0.2: jump to B early.
        assert_eq!(find_line_for_time(2.2, &gap, &map), Some(1));
        // Through the silence and after B starts: still B.
        assert_eq!(find_line_for_time(4.5, &gap, &map), Some(1));
        assert_eq!(find_line_for_time(6.5, &gap, &map), Some(1));

        // No-gap case: A ends 2.0, B starts 3.0 -> gap 1.0 <= 1.5, no early jump.
        let nogap = vec![(10, 1.0, 2.0), (20, 3.0, 4.0)];
        assert_eq!(find_line_for_time(2.5, &nogap, &map), Some(0));
        assert_eq!(find_line_for_time(3.0, &nogap, &map), Some(1));

        // Invalid A.end (end == start): gap unknown -> fall back to
        // B.start - 1.5 = 4.5.
        let badend = vec![(10, 1.0, 1.0), (20, 6.0, 7.0)];
        assert_eq!(find_line_for_time(4.4, &badend, &map), Some(0));
        assert_eq!(find_line_for_time(4.5, &badend, &map), Some(1));
    }
}
