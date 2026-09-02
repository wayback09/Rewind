use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbackMetadata {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub version_string: Option<String>,
    #[serde(default)]
    pub data_version: Option<i32>,
    #[serde(default)]
    pub protocol_version: Option<i32>,
    #[serde(default)]
    pub total_ticks: Option<i32>,
    #[serde(default)]
    pub markers: Option<BTreeMap<String, Marker>>,
    #[serde(default)]
    pub customNamespacesForRegistries: Option<serde_json::Value>,
    pub chunks: BTreeMap<String, ChunkMeta>,
    // Optional fields we tolerate but don't require
    #[serde(default)]
    pub world_name: Option<String>,
    #[serde(default)]
    pub bobby_world_name: Option<String>,
    #[serde(default)]
    pub distantHorizonPaths: Option<serde_json::Value>,
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub duration: i32,
    #[serde(default)]
    pub forcePlaySnapshot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub colour: i32,
    pub description: String,
    #[serde(default)]
    pub position: Option<serde_json::Value>,
}

impl FlashbackMetadata {
    pub fn total_duration(&self) -> i32 {
        self.chunks.values().map(|c| c.duration).sum()
    }

    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if let Some(total) = self.total_ticks {
            let sum = self.total_duration();
            if sum != total {
                issues.push(format!(
                    "total_ticks {} != sum of chunk durations {}",
                    total, sum
                ));
            }
        }
        // Validate chunk names are c<N>.flashback
        for key in self.chunks.keys() {
            if !key.starts_with('c') || !key.ends_with(".flashback") {
                issues.push(format!("unexpected chunk key: {}", key));
            }
        }
        issues
    }
}

pub fn parse_metadata(json_bytes: &[u8]) -> Result<FlashbackMetadata, String> {
    serde_json::from_slice(json_bytes).map_err(|e| format!("metadata.json parse failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let json = br#"{"uuid":"e9b37da6-ed54-40b0-905a-ba55d52e84c2","name":"test recording","version_string":"26.2","data_version":4903,"protocol_version":776,"total_ticks":916,"customNamespacesForRegistries":{},"chunks":{"c0.flashback":{"duration":916,"forcePlaySnapshot":false}}}"#;
        let m = parse_metadata(json).unwrap();
        assert_eq!(m.total_ticks, Some(916));
        assert_eq!(m.chunks.len(), 1);
        assert!(m.validate().is_empty());
    }

    #[test]
    fn total_duration_mismatch() {
        let json = br#"{"uuid":"x","name":"y","chunks":{"c0.flashback":{"duration":10,"forcePlaySnapshot":false}},"total_ticks":5}"#;
        let m = parse_metadata(json).unwrap();
        assert!(!m.validate().is_empty());
    }
}
