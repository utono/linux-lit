use std::collections::HashMap;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::commands::{MpvCommand, MpvEvent};

pub async fn run(
    mut cmd_rx: mpsc::Receiver<MpvCommand>,
    evt_tx: mpsc::Sender<MpvEvent>,
) {
    let mut reader: Option<BufReader<tokio::net::unix::OwnedReadHalf>> = None;
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let mut timestamps: Vec<(i64, f64, f64)> = Vec::new();
    let mut line_id_to_index: HashMap<i64, usize> = HashMap::new();

    loop {
        if let Some(ref mut r) = reader {
            let mut line_buf = String::new();
            tokio::select! {
                result = r.read_line(&mut line_buf) => {
                    match result {
                        Ok(0) | Err(_) => {
                            reader = None;
                            writer = None;
                            let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                        }
                        Ok(_) => {
                            if let Some(pos) = parse_time_pos(&line_buf) {
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
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index).await;
                }
            }
        } else {
            match cmd_rx.recv().await {
                Some(cmd) => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index).await;
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
        MpvCommand::Disconnect => {
            *reader = None;
            *writer = None;
            let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
        }
        MpvCommand::TogglePause => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["cycle","pause"]}"#).await;
            }
        }
        MpvCommand::Seek(time) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["set_property","time-pos",{}]}}"#, time);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::LoadFile(path) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(
                    r#"{{"command":["loadfile","{}"]}}"#,
                    path.replace('"', r#"\""#)
                );
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

fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)],
    line_id_to_index: &HashMap<i64, usize>,
) -> Option<usize> {
    let effective_time = time_pos + crate::input::navigation::SYNC_PREROLL;
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }
    let (line_id, _, _) = timestamps[idx - 1];
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
        assert_eq!(find_line_for_time(2.8, &timestamps, &map), Some(1));
        assert_eq!(find_line_for_time(5.0, &timestamps, &map), Some(2));
    }
}
