use std::collections::HashMap;

use crc::{Crc, CRC_16_KERMIT};
use rand::rngs::StdRng;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;

use super::super::strategy::{FuzzStrategy, MutationResult};

static CRC_KERMIT: Crc<u16> = Crc::<u16>::new(&CRC_16_KERMIT);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChipsMutationMode {
    Random,
    Typed,
}

impl Default for ChipsMutationMode {
    fn default() -> Self {
        Self::Random
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChipsPayloadMode {
    Bitflip,
    Random,
    Boundary,
    Truncate,
    Extend,
}

impl Default for ChipsPayloadMode {
    fn default() -> Self {
        Self::Bitflip
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChipsField {
    Seq,
    Command,
    Payload,
    IsResponse,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TypedPayloadKind {
    Ascii,
    U16Le,
    U32Le,
    Bytes,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ChipsFrameConfig {
    mode: ChipsMutationMode,
    fields: Vec<ChipsField>,
    payload_mode: ChipsPayloadMode,
    flip_prob: f64,
    max_flips: usize,
    payload_flip_prob: f64,
    max_payload_flips: usize,
    max_payload_extend: usize,
    command_payload_types: HashMap<String, TypedPayloadKind>,
}

impl Default for ChipsFrameConfig {
    fn default() -> Self {
        Self {
            mode: ChipsMutationMode::Random,
            fields: vec![ChipsField::Seq, ChipsField::Command, ChipsField::Payload],
            payload_mode: ChipsPayloadMode::Bitflip,
            flip_prob: 0.2,
            max_flips: 2,
            payload_flip_prob: 0.1,
            max_payload_flips: 4,
            max_payload_extend: 16,
            command_payload_types: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct FieldMask {
    seq: bool,
    command: bool,
    payload: bool,
    is_response: bool,
}

impl FieldMask {
    fn from_fields(fields: &[ChipsField]) -> Self {
        let mut mask = Self {
            seq: false,
            command: false,
            payload: false,
            is_response: false,
        };

        for field in fields {
            match field {
                ChipsField::Seq => mask.seq = true,
                ChipsField::Command => mask.command = true,
                ChipsField::Payload => mask.payload = true,
                ChipsField::IsResponse => mask.is_response = true,
            }
        }

        mask
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChipsFrame {
    seq: u8,
    is_response: bool,
    command: u8,
    payload: Vec<u8>,
}

impl ChipsFrame {
    fn encode(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.payload.len() + 4);
        raw.push(self.seq);
        raw.push(((self.is_response as u8) << 7) | (self.command & 0x7F));
        raw.extend_from_slice(&self.payload);
        let crc = crc_kermit(&raw);
        raw.extend_from_slice(&crc.to_le_bytes());

        let escaped = escape_bytes(&raw);
        let mut framed = Vec::with_capacity(escaped.len() + 2);
        framed.push(0x7E);
        framed.extend_from_slice(&escaped);
        framed.push(0x7E);
        framed
    }
}

pub struct ChipsFrameStrategy {
    enabled: bool,
    mode: ChipsMutationMode,
    field_mask: FieldMask,
    payload_mode: ChipsPayloadMode,
    flip_prob: f64,
    max_flips: usize,
    payload_flip_prob: f64,
    max_payload_flips: usize,
    max_payload_extend: usize,
    command_payload_types: HashMap<u8, TypedPayloadKind>,
    pending: Vec<u8>,
}

impl ChipsFrameStrategy {
    pub fn from_config(value: &Value) -> anyhow::Result<Self> {
        let config: ChipsFrameConfig = serde_json::from_value(value.clone())?;
        let field_mask = FieldMask::from_fields(&config.fields);

        let mut command_payload_types: HashMap<u8, TypedPayloadKind> = HashMap::new();
        for (command, kind) in config.command_payload_types {
            let parsed_command = parse_u8_key(&command).ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid CHIPS command key '{}' in command_payload_types (expected 0-255 or 0xNN)",
                    command
                )
            })?;
            command_payload_types.insert(parsed_command, kind);
        }

        Ok(Self {
            enabled: true,
            mode: config.mode,
            field_mask,
            payload_mode: config.payload_mode,
            flip_prob: config.flip_prob,
            max_flips: config.max_flips,
            payload_flip_prob: config.payload_flip_prob,
            max_payload_flips: config.max_payload_flips,
            max_payload_extend: config.max_payload_extend.max(1),
            command_payload_types,
            pending: Vec::new(),
        })
    }

    fn bitflip_limited(
        data: &mut [u8],
        flip_prob: f64,
        max_flips: usize,
        rng: &mut StdRng,
    ) -> usize {
        let mut flips = 0;
        for byte in data {
            if flips >= max_flips {
                break;
            }
            if rng.gen_bool(flip_prob.clamp(0.0, 1.0)) {
                let bit = rng.gen_range(0..8u8);
                *byte ^= 1 << bit;
                flips += 1;
            }
        }
        flips
    }

    fn mutate_payload_random_mode(&self, payload: &mut Vec<u8>, rng: &mut StdRng) -> Option<String> {
        if payload.is_empty() {
            return None;
        }

        match self.payload_mode {
            ChipsPayloadMode::Bitflip => {
                let flips = Self::bitflip_limited(
                    payload,
                    self.payload_flip_prob,
                    self.max_payload_flips,
                    rng,
                );
                if flips > 0 {
                    Some(format!("payload bitflips={}", flips))
                } else {
                    None
                }
            }
            ChipsPayloadMode::Random => {
                for byte in payload.iter_mut() {
                    *byte = rng.r#gen();
                }
                Some(format!("payload randomized len={}", payload.len()))
            }
            ChipsPayloadMode::Boundary => {
                let value = if rng.gen_bool(0.5) { 0x00 } else { 0xFF };
                for byte in payload.iter_mut() {
                    *byte = value;
                }
                Some(format!("payload boundary fill=0x{:02X}", value))
            }
            ChipsPayloadMode::Truncate => {
                if payload.len() <= 1 {
                    return None;
                }
                let new_len = rng.gen_range(0..payload.len());
                let old_len = payload.len();
                payload.truncate(new_len);
                Some(format!("payload truncated {}->{}", old_len, new_len))
            }
            ChipsPayloadMode::Extend => {
                let extra = rng.gen_range(1..=self.max_payload_extend);
                let old_len = payload.len();
                for _ in 0..extra {
                    payload.push(rng.r#gen());
                }
                Some(format!("payload extended {}->{}", old_len, payload.len()))
            }
        }
    }

    fn mutate_payload_typed_mode(
        &self,
        command: u8,
        payload: &mut Vec<u8>,
        rng: &mut StdRng,
    ) -> Option<String> {
        let kind = self.command_payload_types.get(&command)?;

        match kind {
            TypedPayloadKind::Ascii => {
                if payload.is_empty() {
                    return None;
                }
                let idx = rng.gen_range(0..payload.len());
                let replacement = rng.gen_range(0x20u8..=0x7Eu8);
                payload[idx] = replacement;
                Some(format!("payload ascii mutate idx={} val=0x{:02X}", idx, replacement))
            }
            TypedPayloadKind::U16Le => {
                if payload.len() < 2 {
                    return None;
                }
                let value: u16 = rng.r#gen();
                payload[0..2].copy_from_slice(&value.to_le_bytes());
                Some(format!("payload u16le mutate value={}", value))
            }
            TypedPayloadKind::U32Le => {
                if payload.len() < 4 {
                    return None;
                }
                let value: u32 = rng.r#gen();
                payload[0..4].copy_from_slice(&value.to_le_bytes());
                Some(format!("payload u32le mutate value={}", value))
            }
            TypedPayloadKind::Bytes => self.mutate_payload_random_mode(payload, rng),
        }
    }

    fn take_next_frame(&mut self) -> Option<ChipsFrame> {
        loop {
            let Some(start_pos) = self.pending.iter().position(|&b| b == 0x7E) else {
                self.pending.clear();
                return None;
            };

            if start_pos > 0 {
                self.pending.drain(..start_pos);
            }

            let Some(end_pos) = self.pending[1..]
                .iter()
                .position(|&b| b == 0x7E)
                .map(|pos| pos + 1) else {
                return None;
            };

            let candidate = self.pending[..=end_pos].to_vec();
            match decode_chips_frame(&candidate) {
                Ok(frame) => {
                    self.pending.drain(..=end_pos);
                    return Some(frame);
                }
                Err(_) => {
                    self.pending.drain(..1);
                }
            }
        }
    }
}

impl FuzzStrategy for ChipsFrameStrategy {
    fn name(&self) -> &str {
        "chips_frame"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn apply_bytes(&mut self, data: &mut Vec<u8>, rng: &mut StdRng) -> MutationResult {
        self.pending.extend_from_slice(data);

        let Some(mut frame) = self.take_next_frame() else {
            return MutationResult::Buffered("waiting for complete CHIPS frame".to_string());
        };

        let original = frame.clone();
        let mut changes: Vec<String> = Vec::new();

        if self.field_mask.seq {
            let mut seq_buf = [frame.seq];
            let flips = Self::bitflip_limited(&mut seq_buf, self.flip_prob, self.max_flips.max(1), rng);
            if flips > 0 {
                frame.seq = seq_buf[0];
                changes.push(format!("seq bitflips={}", flips));
            }
        }

        if self.field_mask.command {
            let mut cmd_buf = [frame.command & 0x7F];
            let flips = Self::bitflip_limited(&mut cmd_buf, self.flip_prob, self.max_flips.max(1), rng);
            if flips > 0 {
                frame.command = cmd_buf[0] & 0x7F;
                changes.push(format!("command bitflips={}", flips));
            }
        }

        if self.field_mask.is_response && rng.gen_bool(self.flip_prob.clamp(0.0, 1.0)) {
            frame.is_response = !frame.is_response;
            changes.push("is_response toggled".to_string());
        }

        if self.field_mask.payload {
            let payload_change = match self.mode {
                ChipsMutationMode::Random => self.mutate_payload_random_mode(&mut frame.payload, rng),
                ChipsMutationMode::Typed => self
                    .mutate_payload_typed_mode(frame.command, &mut frame.payload, rng)
                    .or_else(|| self.mutate_payload_random_mode(&mut frame.payload, rng)),
            };
            if let Some(change) = payload_change {
                changes.push(change);
            }
        }

        // Always emit a complete re-encoded frame once one is decoded from the internal buffer.
        *data = frame.encode();

        if frame == original {
            return MutationResult::Passed;
        }
        MutationResult::Mutated(changes.join(", "))
    }
}

fn crc_kermit(data: &[u8]) -> u16 {
    CRC_KERMIT.checksum(data)
}

fn escape_bytes(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    for &byte in input {
        match byte {
            0x7E => {
                output.push(0x7D);
                output.push(0x5E);
            }
            0x7D => {
                output.push(0x7D);
                output.push(0x5D);
            }
            _ => output.push(byte),
        }
    }
    output
}

fn unescape_bytes(input: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == 0x7D {
            i += 1;
            if i >= input.len() {
                return None;
            }
            match input[i] {
                0x5E => output.push(0x7E),
                0x5D => output.push(0x7D),
                _ => return None,
            }
        } else {
            output.push(input[i]);
        }
        i += 1;
    }
    Some(output)
}

fn decode_chips_frame(data: &[u8]) -> anyhow::Result<ChipsFrame> {
    if data.len() < 2 {
        anyhow::bail!("CHIPS data too short");
    }
    if data[0] != 0x7E || data[data.len() - 1] != 0x7E {
        anyhow::bail!("CHIPS missing sync bytes");
    }

    let unescaped = unescape_bytes(&data[1..data.len() - 1]).ok_or_else(|| anyhow::anyhow!("CHIPS invalid escaping"))?;
    if unescaped.len() < 4 {
        anyhow::bail!("CHIPS frame too short");
    }

    let content_len = unescaped.len() - 2;
    let content = &unescaped[..content_len];

    let received_crc = u16::from_le_bytes([unescaped[content_len], unescaped[content_len + 1]]);
    let calculated_crc = crc_kermit(content);
    if received_crc != calculated_crc {
        anyhow::bail!("CHIPS crc mismatch");
    }

    let seq = content[0];
    let flags = content[1];
    let is_response = (flags & 0x80) != 0;
    let command = flags & 0x7F;
    let payload = content[2..].to_vec();

    Ok(ChipsFrame {
        seq,
        is_response,
        command,
        payload,
    })
}

fn parse_u8_key(input: &str) -> Option<u8> {
    let trimmed = input.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u8>().ok()
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn chips_frame_mutation_reencodes_valid_crc() {
        let cfg = serde_json::json!({
            "type": "chips_frame",
            "mode": "random",
            "fields": ["seq", "command", "payload"],
            "payload_mode": "random",
            "flip_prob": 1.0,
            "max_flips": 2
        });

        let mut strategy = ChipsFrameStrategy::from_config(&cfg).unwrap();
        let frame = ChipsFrame {
            seq: 1,
            is_response: false,
            command: 0x22,
            payload: b"hello".to_vec(),
        };

        let mut encoded = frame.encode();
        let mut rng = StdRng::seed_from_u64(123);

        let result = strategy.apply_bytes(&mut encoded, &mut rng);
        assert!(matches!(result, MutationResult::Mutated(_)));

        let decoded = decode_chips_frame(&encoded).unwrap();
        assert_ne!(decoded, frame);
        assert!(decoded.command <= 0x7F);
    }

    #[test]
    fn chips_frame_typed_payload_mode_uses_command_type() {
        let cfg = serde_json::json!({
            "type": "chips_frame",
            "mode": "typed",
            "fields": ["payload"],
            "payload_mode": "bitflip",
            "command_payload_types": {
                "0x10": "u16_le"
            }
        });

        let mut strategy = ChipsFrameStrategy::from_config(&cfg).unwrap();
        let frame = ChipsFrame {
            seq: 1,
            is_response: false,
            command: 0x10,
            payload: vec![0xAA, 0xBB],
        };

        let mut encoded = frame.encode();
        let mut rng = StdRng::seed_from_u64(321);

        let result = strategy.apply_bytes(&mut encoded, &mut rng);
        assert!(matches!(result, MutationResult::Mutated(_)));

        let decoded = decode_chips_frame(&encoded).unwrap();
        assert_eq!(decoded.payload.len(), 2);
        assert_ne!(decoded.payload, frame.payload);
    }

    #[test]
    fn chips_frame_buffers_until_complete_packet() {
        let cfg = serde_json::json!({
            "type": "chips_frame",
            "mode": "random",
            "fields": ["seq", "command", "payload"],
            "payload_mode": "random",
            "flip_prob": 0.0,
            "max_flips": 1
        });

        let mut strategy = ChipsFrameStrategy::from_config(&cfg).unwrap();
        let frame = ChipsFrame {
            seq: 3,
            is_response: false,
            command: 0x11,
            payload: b"buffer me".to_vec(),
        };
        let encoded = frame.encode();
        let split = encoded.len() / 2;

        let mut first = encoded[..split].to_vec();
        let mut rng = StdRng::seed_from_u64(7);
        assert!(matches!(
            strategy.apply_bytes(&mut first, &mut rng),
            MutationResult::Buffered(_)
        ));
        assert!(!strategy.pending.is_empty());

        let mut second = encoded[split..].to_vec();
        let result = strategy.apply_bytes(&mut second, &mut rng);
        assert!(matches!(result, MutationResult::Mutated(_) | MutationResult::Passed));
        assert!(strategy.pending.len() < encoded.len());
    }
}
