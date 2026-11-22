//! The `modelInstructions` setting: groups of models, each with a text
//! the agent's system prompt carries when it runs on one of them (the
//! extension's `vscode/src/instructions.ts`). A group shows only the
//! models of connected providers; the others stay in the file and come
//! back with their provider.

use serde_json::{json, Value};

use crate::models::Model;
use crate::providers::Provider;

/// One group of the file: its models (`provider/model-id`), and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub models: Vec<String>,
    pub text: String,
}

/// How a group names a model.
#[must_use]
pub fn key(model: &Model) -> String {
    format!("{}/{}", model.provider, model.id)
}

/// The groups the setting's value holds.
#[must_use]
pub fn saved(value: &Value) -> Vec<Group> {
    (value.as_array().into_iter().flatten())
        .map(|group| Group {
            models: (group.get("models").and_then(Value::as_array))
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            text: (group.get("text").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

/// The models of the connected providers: the only ones a group shows.
/// The session's list keeps the model in use even when its provider is
/// gone, hence the providers.
#[must_use]
pub fn connected<'a>(models: &'a [Model], providers: &[Provider]) -> Vec<&'a Model> {
    let on = |id: &str| {
        providers
            .iter()
            .any(|row| row.connected && row.key.id == id)
    };
    models.iter().filter(|model| on(&model.provider)).collect()
}

/// The setting once the group at `index` (`None`: a new one) is saved
/// with the `checked` models and `text`, or deleted (`checked` `None`).
/// Its models that are not `shown` now keep their place; the newly checked
/// ones follow. A setting left with no group is `null`: it leaves the file.
#[must_use]
pub fn with(
    groups: &[Group],
    index: Option<usize>,
    checked: Option<(&[String], &str)>,
    shown: &[String],
) -> Value {
    let written = |group: &Group| json!({ "models": group.models, "text": group.text });
    let Some((checked, text)) = checked else {
        let rest: Vec<Value> = (groups.iter().enumerate())
            .filter(|(at, _)| Some(*at) != index)
            .map(|(_, group)| written(group))
            .collect();
        return if rest.is_empty() {
            Value::Null
        } else {
            json!(rest)
        };
    };
    let before = index.and_then(|index| groups.get(index));
    let saved: &[String] = before.map_or(&[], |group| &group.models);
    let mut models: Vec<String> = (saved.iter())
        .filter(|model| !shown.contains(model) || checked.contains(model))
        .cloned()
        .collect();
    for model in checked {
        if !models.contains(model) {
            models.push(model.clone());
        }
    }
    let group = Group {
        models,
        text: text.trim().to_string(),
    };
    let mut all: Vec<Value> = groups.iter().map(written).collect();
    match index.filter(|index| *index < all.len()) {
        Some(index) => all[index] = written(&group),
        None => all.push(written(&group)),
    }
    json!(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    // How saving a group could damage the file:
    // - the models of a provider that is not connected now, which the
    //   screen does not show, dropped from the group, so the instructions
    //   are lost for them when the provider comes back;
    // - a model unchecked on the screen kept; one checked twice written twice;
    // - the other groups reordered, or the saved one moved to the end;
    // - a new group written over the first;
    // - the last group deleted leaving `[]` in the file rather than nothing;
    // - the text saved with the blank lines typed around it.
    #[test]
    fn a_saved_group_keeps_the_models_the_screen_does_not_show() {
        let groups = vec![
            Group {
                models: names(&["a/1", "gone/9", "b/2"]),
                text: "Be brief.".to_string(),
            },
            Group {
                models: names(&["c/3"]),
                text: "Use tabs.".to_string(),
            },
        ];
        let shown = names(&["a/1", "b/2", "c/3", "d/4"]);
        let saved_as = with(
            &groups,
            Some(0),
            Some((&names(&["b/2", "d/4", "d/4"]), "\n Be brief!\n")),
            &shown,
        );
        assert_eq!(
            saved_as,
            json!([
                { "models": ["gone/9", "b/2", "d/4"], "text": "Be brief!" },
                { "models": ["c/3"], "text": "Use tabs." },
            ])
        );
        let added = with(&groups, None, Some((&names(&["d/4"]), "New.")), &shown);
        assert_eq!(saved(&added).len(), 3);
        assert_eq!(
            saved(&added)[2],
            Group {
                models: names(&["d/4"]),
                text: "New.".to_string()
            }
        );
        assert_eq!(saved(&added)[..2], groups[..]);
        let one_left = with(&groups, Some(0), None, &shown);
        assert_eq!(
            one_left,
            json!([{ "models": ["c/3"], "text": "Use tabs." }])
        );
        assert_eq!(with(&saved(&one_left), Some(0), None, &shown), Value::Null);
        assert!(saved(&Value::Null).is_empty() && saved(&json!("odd")).is_empty());
    }
}
