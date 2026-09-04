//! Minecraft block model JSON representation and inheritance.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockModel {
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub textures: Option<BTreeMap<String, serde_json::Value>>, // value can be string or {sprite:..., force_translucent}
    #[serde(default)]
    pub elements: Option<Vec<ModelElement>>,
    #[serde(default)]
    pub display: Option<serde_json::Value>,
    #[serde(default)]
    pub ambientocclusion: Option<bool>,
    #[serde(default)]
    pub gui_light: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelElement {
    pub from: [f32; 3],
    pub to: [f32; 3],
    #[serde(default)]
    pub rotation: Option<ElementRotation>,
    #[serde(default)]
    pub shade: Option<bool>,
    pub faces: BTreeMap<String, ModelFace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementRotation {
    pub origin: [f32; 3],
    pub axis: String, // "x","y","z"
    pub angle: f32,   // -45..45
    #[serde(default)]
    pub rescale: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFace {
    pub texture: String, // "#all" or "minecraft:block/stone"
    #[serde(default)]
    pub uv: Option<[f32; 4]>,
    #[serde(default)]
    pub rotation: Option<i32>,
    #[serde(default)]
    pub cullface: Option<String>,
    #[serde(default)]
    pub tintindex: Option<i32>,
}

impl BlockModel {
    pub fn from_value(v: serde_json::Value) -> Result<Self, String> {
        serde_json::from_value(v).map_err(|e| format!("model parse: {e}"))
    }
}

/// Resolve parent chain: child textures override parent, elements from child if present else parent, ambientocclusion etc.
pub fn resolve_model(
    key: &str,
    provider: &mut crate::asset::JarAssetProvider,
) -> Result<ResolvedModel, String> {
    let mut chain: Vec<BlockModel> = Vec::new();
    let mut cur_key = key.to_string();
    for _ in 0..16 {
        let json = provider.load_model(&cur_key)?;
        let model = BlockModel::from_value(json)?;
        let parent = model.parent.clone();
        chain.push(model);
        if let Some(p) = parent {
            // normalize parent like "block/cube" -> "minecraft:block/cube"
            let normalized = if p.contains(':') {
                p
            } else if p.starts_with("block/") {
                format!("minecraft:{p}")
            } else {
                p
            };
            cur_key = normalized;
        } else {
            break;
        }
    }
    chain.reverse(); // root first
                     // Merge
    let mut textures: BTreeMap<String, String> = BTreeMap::new();
    let mut elements: Option<Vec<ModelElement>> = None;
    let mut ambientocclusion: Option<bool> = None;
    for m in &chain {
        if let Some(t) = &m.textures {
            for (k, v) in t {
                let resolved = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Object(obj) => obj
                        .get("sprite")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    _ => "".to_string(),
                };
                // Resolve # references later: keep raw, but child overrides
                textures.insert(k.clone(), resolved);
            }
        }
        if let Some(e) = &m.elements {
            elements = Some(e.clone());
        }
        if m.ambientocclusion.is_some() {
            ambientocclusion = m.ambientocclusion;
        }
    }
    // Resolve # in textures: e.g., "#all" -> "minecraft:block/stone"
    // Do iterative substitution until no #.
    let mut resolved_textures: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in &textures {
        let mut cur = v.clone();
        for _ in 0..8 {
            if let Some(stripped) = cur.strip_prefix('#') {
                if let Some(target) = textures.get(stripped) {
                    cur = target.clone();
                } else if let Some(target) = resolved_textures.get(stripped) {
                    cur = target.clone();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        // Ensure minecraft: prefix
        let final_tex = if cur.starts_with("minecraft:") {
            cur
        } else if cur.contains(':') {
            cur
        } else if cur.is_empty() {
            "".to_string()
        } else {
            // bare like "block/stone" -> "minecraft:block/stone"
            format!("minecraft:{cur}")
        };
        resolved_textures.insert(k.clone(), final_tex);
    }
    Ok(ResolvedModel {
        key: key.to_string(),
        textures: resolved_textures,
        elements: elements.unwrap_or_default(),
        ambientocclusion: ambientocclusion.unwrap_or(true),
    })
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub key: String,
    pub textures: BTreeMap<String, String>,
    pub elements: Vec<ModelElement>,
    pub ambientocclusion: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::JarAssetProvider;
    #[test]
    fn resolve_cube_all() {
        let Some(jar) = crate::asset::default_jar_path() else {
            return;
        };
        let mut prov = JarAssetProvider::from_jar(jar).unwrap();
        let m = resolve_model("minecraft:block/cube_all", &mut prov).unwrap();
        assert!(!m.elements.is_empty());
        assert!(m.textures.contains_key("all") || m.textures.contains_key("particle"));
    }
    #[test]
    fn resolve_stairs() {
        let Some(jar) = crate::asset::default_jar_path() else {
            return;
        };
        let mut prov = JarAssetProvider::from_jar(jar).unwrap();
        let m = resolve_model("minecraft:block/oak_stairs", &mut prov).unwrap();
        assert_eq!(m.elements.len(), 2);
    }
}
