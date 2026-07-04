use serde::{de::DeserializeOwned, Serialize};
use std::io::{Read, Write};

pub const MAX_FRAME_LEN: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame payload is too large: {0} bytes")]
    TooLarge(usize),
    #[error("frame is truncated")]
    Truncated,
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn encode_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, FrameError> {
    let payload = serde_json::to_vec(message)?;
    if payload.len() > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(payload.len()));
    }

    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, FrameError> {
    if frame.len() < 4 {
        return Err(FrameError::Truncated);
    }

    let len = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }
    if frame.len() < len + 4 {
        return Err(FrameError::Truncated);
    }

    Ok(serde_json::from_slice(&frame[4..4 + len])?)
}

pub fn read_payload(reader: &mut impl Read) -> Result<Vec<u8>, FrameError> {
    let mut len = [0u8; 4];
    reader.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME_LEN {
        return Err(FrameError::TooLarge(len));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T, FrameError> {
    let payload = read_payload(reader)?;
    Ok(serde_json::from_slice(&payload)?)
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> Result<(), FrameError> {
    let frame = encode_frame(message)?;
    writer.write_all(&frame)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PipeLineEvent, PipeLineMeta, PROTOCOL_VERSION};

    fn sample_event() -> PipeLineEvent {
        PipeLineEvent {
            event_type: "line".to_owned(),
            protocol_version: PROTOCOL_VERSION,
            message_id: 42,
            timestamp_unix_ms: 1_782_806_400_123,
            text: "hello".to_owned(),
            meta: PipeLineMeta {
                process_id: 1234,
                thread_number: 17,
                thread_name: None,
                window_title: Some("Game Window".to_owned()),
                is_current_select: true,
                arch: "x64".to_owned(),
                source: "textractor".to_owned(),
            },
        }
    }

    #[test]
    fn round_trips_length_prefixed_json() {
        let event = sample_event();
        let frame = encode_frame(&event).expect("frame");
        let decoded: PipeLineEvent = decode_frame(&frame).expect("decoded");
        assert_eq!(decoded, event);
    }

    #[test]
    fn rejects_truncated_frames() {
        let event = sample_event();
        let mut frame = encode_frame(&event).expect("frame");
        frame.pop();
        assert!(matches!(
            decode_frame::<PipeLineEvent>(&frame),
            Err(FrameError::Truncated)
        ));
    }

    #[test]
    fn rejects_oversized_frames() {
        let mut frame = ((MAX_FRAME_LEN + 1) as u32).to_le_bytes().to_vec();
        frame.push(b'{');
        assert!(matches!(
            decode_frame::<PipeLineEvent>(&frame),
            Err(FrameError::TooLarge(_))
        ));
    }
}
