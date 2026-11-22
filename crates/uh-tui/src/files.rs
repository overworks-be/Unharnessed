//! The session's folder as the `@` menu lists it: its files, less the ones
//! git ignores (the extension lists them with git). Outside a repository
//! the hidden ones are left out too.

use std::path::Path;

use ignore::WalkBuilder;

/// The files the `@` menu gets at most.
const LIMIT: usize = 20_000;

/// The files under `folder`, relative to it and `/`-separated, in order.
#[must_use]
pub fn list(folder: &Path) -> Vec<String> {
    let repository = folder.ancestors().any(|above| above.join(".git").exists());
    let walk = WalkBuilder::new(folder)
        .hidden(!repository)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    let mut files: Vec<String> = walk
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| !kind.is_dir()))
        .filter_map(|entry| {
            let path = entry.path().strip_prefix(folder).ok()?;
            let parts: Option<Vec<&str>> = path.iter().map(|part| part.to_str()).collect();
            Some(parts?.join("/"))
        })
        // A name that would drive the terminal, or break the message's line, is not offered.
        .filter(|path| !path.is_empty() && !path.contains(char::is_control))
        .take(LIMIT)
        .collect();
    files.sort();
    files
}
