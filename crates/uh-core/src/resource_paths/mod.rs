//! Session resource resolution: `ResourceResolver::resolve` produces the
//! ranked skill/prompt paths sessions consume, from the settings
//! arrays and auto-discovery (settings-base directories, `.agents/skills`
//! ancestor scan, bundled skills). It was the package manager's resolve
//! step; the package manager itself (the `packages` setting, npm/git
//! installs, updates) was removed.

mod manager;
pub(crate) mod resolve;
mod source;

pub use manager::{BundledSkillsDir, ResourceResolver, ResourceResolverOptions};
pub use resolve::{
    is_pattern, MetadataSource, PathMetadata, ResolvedPaths, ResolvedResource, ResourceOrigin,
    ResourceType,
};
pub use source::{parse_git_url, GitSource, SourceScope};

use std::path::PathBuf;

/// The TS `CONFIG_DIR_NAME` (project-local settings root).
pub use crate::settings::CONFIG_DIR_NAME;

/// The package directory: `UNHARNESSED_PACKAGE_DIR` wins (matching the TS
/// `getPackageDir` override), then the directory of the executable (the
/// packaged bun-binary layout).
pub(crate) fn package_dir() -> PathBuf {
    if let Ok(env_dir) = std::env::var("UNHARNESSED_PACKAGE_DIR") {
        if !env_dir.is_empty() {
            return expand_tilde(&env_dir);
        }
    }
    uh_types::platform::executable_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The bundled docs directory (TS `getDocsPath`): `<package dir>/docs`.
#[must_use]
pub fn docs_path() -> PathBuf {
    package_dir().join("docs")
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    uh_types::platform::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// The workspace root at compile time (source-checkout layout): uh-core
/// lives at `<root>/crates/uh-core`.
/// Compile-time workspace root (`<root>/crates/uh-core` ancestors), shared by
/// every package-dir resolution that falls back to the source-checkout layout.
pub(crate) fn source_checkout_root() -> Option<&'static std::path::Path> {
    static ROOT: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .map(std::path::Path::to_path_buf)
    })
    .as_deref()
}

/// The directory of built-in skills shipped with the package (TS
/// `getBundledSkillsDir`): `skills/` next to the executable (the packaged
/// layout), falling back to the workspace `skills/` for source checkouts
/// (TS keeps built-in skills at the package root next to `src/`).
pub(crate) fn get_bundled_skills_dir() -> PathBuf {
    let packaged = package_dir().join("skills");
    if packaged.is_dir() {
        return packaged;
    }
    if let Some(root) = source_checkout_root() {
        let source_checkout = root.join("skills");
        if source_checkout.is_dir() {
            return source_checkout;
        }
    }
    packaged
}

#[cfg(test)]
pub(crate) mod test_support {
    /// Process-wide env reads and writes (HOME, `UNHARNESSED_OFFLINE`) serialize
    /// through one lock across the resolution test modules: parallel test
    /// threads in the same binary otherwise race the process env.
    pub(crate) static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
