//! Blockstate variant/multipart resolution.

use crate::asset::JarAssetProvider;
use replay_model::CanonicalBlockState;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BlockModelRef {
    pub key: String, // e.g., "minecraft:block/stone"
    pub x: i32,
    pub y: i32,
    pub uvlock: bool,
    pub weight: u32,
}

/// Deterministic weighted selection: sort keys and pick based on pos hash.
fn pick_weighted(variants: &[BlockModelRef], pos: Option<(i32, i32, i32)>) -> &BlockModelRef {
    if variants.len() == 1 {
        return &variants[0];
    }
    let hash = if let Some((x, y, z)) = pos {
        // Simple hash of pos for determinism (like vanilla per-position)
        let h = (x as i64).wrapping_mul(3129871) ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
        (h.unsigned_abs() % 1000) as usize
    } else {
        0
    };
    let total_weight: u32 = variants.iter().map(|v| v.weight).sum();
    if total_weight == 0 {
        return &variants[0];
    }
    let mut r = (hash as u32) % total_weight;
    for v in variants {
        if r < v.weight {
            return v;
        }
        r -= v.weight;
    }
    &variants[0]
}

/// Resolve a CanonicalBlockState to one or more BlockModelRef via blockstate JSON.
/// `pos` is optional world pos for weighted random determinism.
pub fn resolve_blockstate(
    state: &CanonicalBlockState,
    provider: &mut JarAssetProvider,
    pos: Option<(i32, i32, i32)>,
) -> Result<Vec<BlockModelRef>, String> {
    let json = provider.load_blockstate(&state.name)?;
    // Filter properties to those relevant: only those present in blockstate JSON keys.
    // For variant: keys are comma-joined "prop=value" sorted alphabetically.
    // For multipart: when conditions only check subset.
    // Simplest: try full key first, if miss and state has waterlogged, retry without waterlogged.
    // More robust: for variant, build key from sorted properties, but if miss, try stripping waterlogged.
    if let Some(variants) = json.get("variants") {
        return resolve_variants(state, variants, pos);
    }
    if let Some(multipart) = json.get("multipart") {
        return resolve_multipart(state, multipart, pos);
    }
    Err(format!(
        "blockstate {} has no variants/multipart",
        state.name
    ))
}

fn resolve_variants(
    state: &CanonicalBlockState,
    variants: &Value,
    pos: Option<(i32, i32, i32)>,
) -> Result<Vec<BlockModelRef>, String> {
    let map = variants.as_object().ok_or("variants not object")?;
    // Build key from state properties sorted (BTreeMap already sorted)
    let key = build_variant_key(&state.properties);
    // Try exact, then without waterlogged, then empty
    let mut candidates = Vec::new();
    candidates.push(key.clone());
    if state.properties.contains_key("waterlogged") {
        let mut stripped = state.properties.clone();
        stripped.remove("waterlogged");
        candidates.push(build_variant_key(&stripped));
    }
    // For blocks like stone with "" key, properties empty => key ""
    let mut found: Option<&Value> = None;
    for k in &candidates {
        if let Some(v) = map.get(k) {
            found = Some(v);
            break;
        }
    }
    // Fallback: if state has empty properties, try "" key
    if found.is_none() && map.contains_key("") {
        found = map.get("");
    }
    let v = found.ok_or_else(|| {
        format!(
            "variant not found for {} key '{}' candidates {:?}",
            state, key, candidates
        )
    })?;
    let refs = parse_model_value(v)?;
    // If multiple weighted, pick deterministically one
    if refs.len() > 1 {
        let chosen = pick_weighted(&refs, pos);
        return Ok(vec![chosen.clone()]);
    }
    Ok(refs)
}

fn resolve_multipart(
    state: &CanonicalBlockState,
    multipart: &Value,
    pos: Option<(i32, i32, i32)>,
) -> Result<Vec<BlockModelRef>, String> {
    let arr = multipart.as_array().ok_or("multipart not array")?;
    let mut out = Vec::new();
    for entry in arr {
        let when = entry.get("when");
        let apply = entry.get("apply").ok_or("multipart entry missing apply")?;
        let matches = if let Some(w) = when {
            matches_when(state, w)
        } else {
            true
        };
        if matches {
            let mut refs = parse_model_value(apply)?;
            // For multipart, if apply is array weighted, pick one deterministically
            if refs.len() > 1 {
                let chosen = pick_weighted(&refs, pos);
                out.push(chosen.clone());
            } else {
                out.extend(refs);
            }
        }
    }
    Ok(out)
}

fn matches_when(state: &CanonicalBlockState, when: &Value) -> bool {
    if let Some(obj) = when.as_object() {
        for (k, v) in obj {
            if k == "OR" {
                if let Some(arr) = v.as_array() {
                    // OR: any sub-condition matches
                    let mut any = false;
                    for sub in arr {
                        if matches_when(state, sub) {
                            any = true;
                            break;
                        }
                    }
                    if !any {
                        return false;
                    }
                }
            } else {
                // Normal property check: value may contain "|" for OR
                let state_val = match state.properties.get(k) {
                    Some(s) => s,
                    None => return false,
                };
                if let Some(s) = v.as_str() {
                    // Check if s contains "|"
                    if s.contains('|') {
                        let parts: Vec<&str> = s.split('|').collect();
                        if !parts.contains(&state_val.as_str()) {
                            return false;
                        }
                    } else if s != state_val {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
        true
    } else {
        false
    }
}

fn build_variant_key(props: &BTreeMap<String, String>) -> String {
    if props.is_empty() {
        return "".to_string();
    }
    props
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_model_value(v: &Value) -> Result<Vec<BlockModelRef>, String> {
    if let Some(arr) = v.as_array() {
        let mut out = Vec::new();
        for item in arr {
            out.extend(parse_single_model(item)?);
        }
        Ok(out)
    } else {
        parse_single_model(v)
    }
}

fn parse_single_model(v: &Value) -> Result<Vec<BlockModelRef>, String> {
    let obj = v.as_object().ok_or("model not object")?;
    let model = obj
        .get("model")
        .and_then(|x| x.as_str())
        .ok_or("model missing model field")?
        .to_string();
    let x = obj.get("x").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
    let y = obj.get("y").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
    let uvlock = obj.get("uvlock").and_then(|x| x.as_bool()).unwrap_or(false);
    let weight = obj.get("weight").and_then(|x| x.as_i64()).unwrap_or(1) as u32;
    Ok(vec![BlockModelRef {
        key: model,
        x,
        y,
        uvlock,
        weight,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use replay_model::CanonicalBlockState;
    use std::collections::BTreeMap;
    #[test]
    fn variant_stairs() {
        let Some(jar) = crate::asset::default_jar_path() else {
            return;
        };
        let mut prov = crate::asset::JarAssetProvider::from_jar(jar).unwrap();
        let mut props = BTreeMap::new();
        props.insert("facing".into(), "north".into());
        props.insert("half".into(), "bottom".into());
        props.insert("shape".into(), "straight".into());
        props.insert("waterlogged".into(), "true".into());
        let state = CanonicalBlockState {
            name: "minecraft:oak_stairs".into(),
            properties: props,
        };
        let refs = resolve_blockstate(&state, &mut prov, Some((0, 0, 0))).unwrap();
        assert_eq!(refs.len(), 1);
        assert!(refs[0].key.contains("oak_stairs"));
    }
    #[test]
    fn multipart_fence() {
        let Some(jar) = crate::asset::default_jar_path() else {
            return;
        };
        let mut prov = crate::asset::JarAssetProvider::from_jar(jar).unwrap();
        let mut props = BTreeMap::new();
        props.insert("east".into(), "true".into());
        props.insert("north".into(), "false".into());
        props.insert("south".into(), "false".into());
        props.insert("west".into(), "false".into());
        props.insert("waterlogged".into(), "false".into());
        let state = CanonicalBlockState {
            name: "minecraft:oak_fence".into(),
            properties: props,
        };
        let refs = resolve_blockstate(&state, &mut prov, None).unwrap();
        // post + east side = 2 models
        assert!(refs.len() >= 2);
    }
}
