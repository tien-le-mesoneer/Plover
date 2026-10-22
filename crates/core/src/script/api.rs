//! Rust side of the script API: the JSON exchanged with `api.js`.

use crate::vars::{VarMap, VarStack};

/// Variable layers as `api.js` sends them to `__brsInterpolate`.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Layers {
    runtime: VarMap,
    request: VarMap,
    folders: Vec<VarMap>,
    environment: VarMap,
    collection: VarMap,
}

/// `bru.interpolate`: Phase 1 interpolation over the script's current variables.
pub(crate) fn interpolate_with_layers(text: &str, layers_json: &str) -> String {
    let layers: Layers = serde_json::from_str(layers_json).unwrap_or_default();
    VarStack::new(
        layers.runtime,
        layers.request,
        layers.folders,
        layers.environment,
        layers.collection,
    )
    .interpolate(text)
}
