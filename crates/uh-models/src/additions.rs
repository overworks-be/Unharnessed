//! Model additions: the models a provider released that the catalog repo
//! does not list yet.
//!
//! The catalog repo admits a model when a maintainer adds its id to a
//! whitelist, so a new model reaches the picker late.
//! `scripts/sync-model-additions.py` (run on a schedule by
//! `.github/workflows/model-additions.yml`) finds those models on
//! models.dev and, for the `ChatGPT` subscription, in the Codex CLI's model
//! list, and writes them to `catalog/models.additions.v1.json` in this
//! repository, in the `catalog.v1` shape; this module reads that file. Its
//! entries go through the strict catalog schema and transport pinning like
//! any catalog entry.
//!
//! Merge: an addition is appended when the base has no entry with its
//! `(provider, id)`; a model the catalog lists is always the catalog's.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::cache::{CatalogCache, RefreshOptions, PUBLIC_SCOPE};
use crate::fetch::CatalogFetcher;
use crate::pinning::{parse_provider_model_catalog, PinnedTemplates};
use crate::Model;

/// The model additions file (written by `scripts/sync-model-additions.py`).
pub const MODEL_ADDITIONS_URL: &str =
    "https://raw.githubusercontent.com/unharnessed-agent/Unharnessed/main/catalog/models.additions.v1.json";

const CACHE_FILE: &str = "model-additions-cache.json";

/// Parse the additions file like the remote catalog (strict schema,
/// skip-invalid, transport pinning). Unlike the catalog, an empty `models`
/// list is valid: the catalog repo caught up and nothing is left to add.
///
/// # Errors
///
/// Fails on an unsupported `schemaVersion`, a missing `models` array, or a
/// non-empty list with no entry this client supports.
pub fn parse_model_additions(
    payload: &Value,
    templates: &PinnedTemplates,
) -> Result<Vec<Model>, String> {
    if payload.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        return Err("Unsupported model additions schema version".into());
    }
    match payload.get("models").and_then(Value::as_array) {
        None => Err("Invalid model additions list".into()),
        Some(models) if models.is_empty() => Ok(Vec::new()),
        Some(_) => parse_provider_model_catalog(payload, templates),
    }
}

/// Append the additions `base` has no `(provider, id)` entry for.
#[must_use]
pub fn merge_model_additions(mut base: Vec<Model>, additions: &[Model]) -> Vec<Model> {
    let missing: Vec<Model> = {
        let listed: HashSet<(&str, &str)> = base
            .iter()
            .map(|model| (model.provider.as_str(), model.id.as_str()))
            .collect();
        additions
            .iter()
            .filter(|model| !listed.contains(&(model.provider.as_str(), model.id.as_str())))
            .cloned()
            .collect()
    };
    base.extend(missing);
    base
}

/// The last-good additions file (unauthenticated, public scope).
pub struct ModelAdditions {
    cache: CatalogCache<Vec<Model>>,
}

impl ModelAdditions {
    /// Additions fetched from `url`, persisted beside `models_dir`
    /// (`model-additions-cache.json`).
    #[must_use]
    pub fn new(models_dir: Option<PathBuf>, url: &str, templates: PinnedTemplates) -> Self {
        let parse: crate::cache::CatalogParse<Vec<Model>> =
            Arc::new(move |payload, _scope| parse_model_additions(payload, &templates));
        Self {
            cache: CatalogCache::new(
                url,
                models_dir.map(|dir| dir.join(CACHE_FILE)),
                Arc::new(CatalogFetcher::new()),
                parse,
            ),
        }
    }

    /// The last-good additions, if the file was ever fetched.
    pub fn get(&self) -> Option<Vec<Model>> {
        self.cache.get(PUBLIC_SCOPE)
    }

    /// Fetch the file (hourly-gated unless `force`); failures keep last-good.
    pub async fn refresh(&self, force: bool) -> Option<Vec<Model>> {
        self.cache
            .refresh(
                PUBLIC_SCOPE,
                RefreshOptions {
                    force,
                    ..RefreshOptions::default()
                },
            )
            .await
    }
}
