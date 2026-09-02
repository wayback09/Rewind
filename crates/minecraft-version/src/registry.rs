use crate::{CanonicalBlockState, MinecraftVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySource {
    pub description: String,
    pub path: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("registry file not found: {message} (searched: {searched:?})")]
    NotFound {
        message: String,
        searched: Vec<String>,
    },
    #[error("registry file unreadable at {path}: {details}")]
    Unreadable { path: String, details: String },
    #[error("registry JSON invalid at {path}: {details}")]
    InvalidJson { path: String, details: String },
    #[error("registry validation failed: {0}")]
    Validation(String),
}

/// File-backed registry for 26.2 — holds `Vec<CanonicalBlockState>` indexed by global ID.
pub struct FileRegistry {
    version: MinecraftVersion,
    states: Vec<CanonicalBlockState>,
    source: RegistrySource,
}

impl FileRegistry {
    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn source(&self) -> &RegistrySource {
        &self.source
    }
}

impl crate::BlockStateRegistry for FileRegistry {
    fn get(&self, id: u32) -> Option<&CanonicalBlockState> {
        self.states.get(id as usize)
    }
    fn len(&self) -> usize {
        self.states.len()
    }
    fn version(&self) -> &MinecraftVersion {
        &self.version
    }
}

#[derive(Debug, Deserialize)]
struct RawRegistryFile {
    version: String,
    data_version: i32,
    protocol_version: i32,
    registry_source: Option<String>,
    num_block_states: Option<usize>,
    block_states: RawBlockStates,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawBlockStates {
    Map(BTreeMap<String, RawEntry>),
    Array(Vec<Option<RawEntry>>),
}

#[derive(Debug, Clone, Deserialize)]
struct RawEntry {
    name: String,
    properties: BTreeMap<String, String>,
}

impl RawEntry {
    fn into_canonical(self) -> CanonicalBlockState {
        CanonicalBlockState {
            name: self.name,
            properties: self.properties,
        }
    }
}

/// Try to locate the 26.2 registry file.
/// Search order:
/// 1. `$FLASHBACK_REGISTRY_PATH` env var
/// 2. `crates/minecraft-version/data/26.2-blocks-array.json` (relative to this crate's manifest, also embedded)
/// 3. `crates/minecraft-version/data/26.2-blocks.json` (object form)
/// 4. `data/registries/26.2-4903-blocks.json` relative to workspace root
/// 5. `%APPDATA%\.minecraft` derived report (if `mc-reports-26.2` exists)
pub fn locate_registry_file() -> Result<PathBuf, RegistryError> {
    let mut searched: Vec<String> = Vec::new();

    // 1. Env var
    if let Ok(p) = std::env::var("FLASHBACK_REGISTRY_PATH") {
        let pb = PathBuf::from(&p);
        searched.push(format!("$FLASHBACK_REGISTRY_PATH={}", p));
        if pb.exists() {
            return Ok(pb);
        }
    } else {
        searched.push("$FLASHBACK_REGISTRY_PATH (not set)".to_string());
    }

    // 2. Crate data dir — array version (preferred, compact)
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p1 = manifest_dir.join("data").join("26.2-blocks-array.json");
    searched.push(p1.display().to_string());
    if p1.exists() {
        return Ok(p1);
    }
    let p2 = manifest_dir.join("data").join("26.2-blocks.json");
    searched.push(p2.display().to_string());
    if p2.exists() {
        return Ok(p2);
    }

    // 3. Workspace root data/registries
    // Try to find workspace root by walking up from manifest_dir
    let mut cur = manifest_dir.as_path();
    for _ in 0..5 {
        let cand = cur
            .join("data")
            .join("registries")
            .join("26.2-4903-blocks.json");
        searched.push(cand.display().to_string());
        if cand.exists() {
            return Ok(cand);
        }
        let cand2 = cur
            .join("crates")
            .join("minecraft-version")
            .join("data")
            .join("26.2-blocks-array.json");
        searched.push(cand2.display().to_string());
        if cand2.exists() {
            return Ok(cand2);
        }
        if let Some(parent) = cur.parent() {
            cur = parent;
        } else {
            break;
        }
    }

    // 4. Temp report location (where `net.minecraft.data.Main --reports` writes)
    let tmp_report = PathBuf::from(
        std::env::var("TEMP")
            .unwrap_or_else(|_| "C:\\Users\\temit\\AppData\\Local\\Temp".to_string()),
    )
    .join("mc-reports-26.2")
    .join("reports")
    .join("blocks.json");
    searched.push(tmp_report.display().to_string());
    if tmp_report.exists() {
        return Ok(tmp_report);
    }

    // 5. Also check APPDATA/.minecraft/versions/26.2 derived? The raw jar is not a registry; we need the generated report.
    // If not found, produce diagnostic.
    Err(RegistryError::NotFound {
        message: "Minecraft 26.2 BlockState registry not found. Run: java -cp <26.2.jar>;<libraries> net.minecraft.data.Main --reports --output <dir> and then transform reports/blocks.json into the expected format.".to_string(),
        searched,
    })
}

pub fn load_26_2_registry() -> Result<FileRegistry, RegistryError> {
    let path = locate_registry_file()?;
    load_registry_from_path(&path)
}

pub fn load_registry_from_path(path: &Path) -> Result<FileRegistry, RegistryError> {
    let data = std::fs::read_to_string(path).map_err(|e| RegistryError::Unreadable {
        path: path.display().to_string(),
        details: e.to_string(),
    })?;

    let raw: RawRegistryFile =
        serde_json::from_str(&data).map_err(|e| RegistryError::InvalidJson {
            path: path.display().to_string(),
            details: e.to_string(),
        })?;

    // Validate version
    if raw.data_version != 4903 || raw.protocol_version != 776 || raw.version != "26.2" {
        return Err(RegistryError::Validation(format!(
            "registry version mismatch: expected 26.2/4903/776 got {}/{}/{} at {}",
            raw.version,
            raw.data_version,
            raw.protocol_version,
            path.display()
        )));
    }

    let version = MinecraftVersion {
        version: raw.version.clone(),
        data_version: raw.data_version,
        protocol_version: raw.protocol_version,
    };

    let states: Vec<CanonicalBlockState> = match raw.block_states {
        RawBlockStates::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for (idx, entry) in arr.into_iter().enumerate() {
                let e = entry.ok_or_else(|| {
                    RegistryError::Validation(format!("gap at id {} in array registry", idx))
                })?;
                out.push(e.into_canonical());
            }
            out
        }
        RawBlockStates::Map(map) => {
            // Map is string id -> entry; need to sort by numeric id and fill Vec
            let max_id = map
                .keys()
                .filter_map(|k| k.parse::<usize>().ok())
                .max()
                .unwrap_or(0);
            let mut vec: Vec<Option<CanonicalBlockState>> = vec![None; max_id + 1];
            for (k, v) in map {
                let id: usize = k
                    .parse()
                    .map_err(|_| RegistryError::Validation(format!("invalid numeric key {}", k)))?;
                vec[id] = Some(v.into_canonical());
            }
            let mut out = Vec::with_capacity(vec.len());
            for (idx, opt) in vec.into_iter().enumerate() {
                let e = opt.ok_or_else(|| {
                    RegistryError::Validation(format!("gap at id {} in map registry", idx))
                })?;
                out.push(e);
            }
            out
        }
    };

    if states.is_empty() {
        return Err(RegistryError::Validation("registry empty".to_string()));
    }
    // Basic sanity: id 0 must be air
    if states[0].name != "minecraft:air" {
        return Err(RegistryError::Validation(format!(
            "registry sanity: id 0 expected minecraft:air got {}",
            states[0].name
        )));
    }
    if states.len() < 1000 {
        return Err(RegistryError::Validation(format!(
            "registry too small: {} entries, expected ~32366 for 26.2",
            states.len()
        )));
    }

    let source = RegistrySource {
        description: raw
            .registry_source
            .unwrap_or_else(|| format!("file:{}", path.display())),
        path: Some(path.display().to_string()),
    };

    Ok(FileRegistry {
        version,
        states,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockStateRegistry;

    #[test]
    fn load_26_2() {
        let reg = load_26_2_registry()
            .expect("registry must be present for M1 (generated from local 26.2 jar)");
        assert_eq!(reg.version().data_version, 4903);
        assert_eq!(reg.version().protocol_version, 776);
        assert_eq!(reg.version().version, "26.2");
        assert!(reg.len() >= 32366, "len {}", reg.len());
        // Spot check
        assert_eq!(reg.get(0).unwrap().name, "minecraft:air");
        assert_eq!(reg.get(1).unwrap().name, "minecraft:stone");
        // id 85 should be bedrock per our dump
        assert_eq!(reg.get(85).unwrap().name, "minecraft:bedrock");
        // oak_stairs id 3907 should be oak_stairs with properties
        let o = reg.get(3907).unwrap();
        assert_eq!(o.name, "minecraft:oak_stairs");
        assert_eq!(
            o.properties.get("facing").map(|s| s.as_str()),
            Some("north")
        );
    }

    #[test]
    fn canonical_display() {
        let mut props = std::collections::BTreeMap::new();
        props.insert("facing".to_string(), "north".to_string());
        props.insert("half".to_string(), "bottom".to_string());
        let s = crate::CanonicalBlockState {
            name: "minecraft:oak_stairs".to_string(),
            properties: props,
        };
        assert_eq!(
            s.to_string(),
            "minecraft:oak_stairs[facing=north,half=bottom]"
        );
    }
}
