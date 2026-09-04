//! Asset loading from Minecraft 26.2 JAR — local, no download.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resolves blockstate/model/texture from local 26.2 JAR.
pub struct JarAssetProvider {
    jar_path: PathBuf,
    // cache parsed JSON to avoid re-reading zip per block
    blockstate_cache: HashMap<String, serde_json::Value>,
    model_cache: HashMap<String, serde_json::Value>,
}

impl JarAssetProvider {
    pub fn from_default_jar() -> Result<Self, String> {
        let jar = default_jar_path().ok_or(
            "26.2 JAR not found at %APPDATA%/.minecraft/versions/26.2/26.2.jar and no fallback",
        )?;
        Self::from_jar(jar)
    }

    pub fn from_jar(path: impl Into<PathBuf>) -> Result<Self, String> {
        let p = path.into();
        if !p.exists() {
            return Err(format!("JAR not found: {}", p.display()));
        }
        Ok(Self {
            jar_path: p,
            blockstate_cache: HashMap::new(),
            model_cache: HashMap::new(),
        })
    }

    pub fn jar_path(&self) -> &Path {
        &self.jar_path
    }

    /// Load raw bytes for a zip entry like `assets/minecraft/blockstates/stone.json`
    pub fn load_bytes(&self, entry: &str) -> Result<Vec<u8>, String> {
        let file = std::fs::File::open(&self.jar_path).map_err(|e| format!("open jar: {e}"))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip open: {e}"))?;
        let mut f = zip
            .by_name(entry)
            .map_err(|_| format!("entry not found: {entry}"))?;
        let mut buf = Vec::new();
        use std::io::Read;
        f.read_to_end(&mut buf)
            .map_err(|e| format!("read {entry}: {e}"))?;
        Ok(buf)
    }

    pub fn load_blockstate(&mut self, block_name: &str) -> Result<serde_json::Value, String> {
        let key = block_name.trim_start_matches("minecraft:");
        if let Some(v) = self.blockstate_cache.get(key) {
            return Ok(v.clone());
        }
        let entry = format!("assets/minecraft/blockstates/{key}.json");
        let bytes = self.load_bytes(&entry)?;
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse {entry}: {e}"))?;
        self.blockstate_cache.insert(key.to_string(), v.clone());
        Ok(v)
    }

    pub fn load_model(&mut self, model_key: &str) -> Result<serde_json::Value, String> {
        // model_key like "minecraft:block/stone" or "block/cube_all"
        let normalized = if let Some(s) = model_key.strip_prefix("minecraft:") {
            s.to_string()
        } else {
            model_key.to_string()
        };
        // normalized now like "block/stone" or "block/cube_all"
        if let Some(v) = self.model_cache.get(&normalized) {
            return Ok(v.clone());
        }
        let entry = format!("assets/minecraft/models/{normalized}.json");
        let bytes = self.load_bytes(&entry)?;
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse {entry}: {e}"))?;
        self.model_cache.insert(normalized, v.clone());
        Ok(v)
    }

    pub fn load_texture_bytes(&self, texture_key: &str) -> Result<Vec<u8>, String> {
        // texture_key like "minecraft:block/stone" -> assets/minecraft/textures/block/stone.png
        let normalized = texture_key
            .strip_prefix("minecraft:")
            .unwrap_or(texture_key);
        // normalized "block/stone"
        let entry = format!("assets/minecraft/textures/{normalized}.png");
        self.load_bytes(&entry)
    }
}

pub fn default_jar_path() -> Option<PathBuf> {
    // %APPDATA%\.minecraft\versions\26.2\26.2.jar
    let appdata = std::env::var("APPDATA").ok()?;
    let p = PathBuf::from(appdata)
        .join(".minecraft")
        .join("versions")
        .join("26.2")
        .join("26.2.jar");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jar_exists_and_blockstate_loadable() {
        let Some(jar) = default_jar_path() else {
            return;
        };
        let mut prov = JarAssetProvider::from_jar(jar).unwrap();
        let v = prov.load_blockstate("minecraft:stone").unwrap();
        assert!(v.get("variants").is_some());
        let m = prov.load_model("minecraft:block/cube_all").unwrap();
        assert!(m.get("parent").is_some());
    }
}
