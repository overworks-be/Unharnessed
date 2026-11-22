//! The models the session can run on, as its `model` option lists them
//! (`crates/uh-daemon/src/acp/config_options.rs`), and how the menu that
//! picks one orders and describes them: the extension's
//! `vscode/webview/models.tsx`.

use serde_json::Value;

use crate::providers::Provider;
use crate::text::clean;

/// One model of the session's `model` option.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// What the option takes to select it.
    pub value: String,
    pub id: String,
    pub name: String,
    /// Its provider's id.
    pub provider: String,
    /// Its context window, in tokens; 0 when unknown.
    pub window: u64,
    /// What a million tokens cost, in and out; absent when unknown.
    pub cost: Option<(f64, f64)>,
    /// What it reads (`text`, `image`); empty when unknown.
    pub input: Vec<String>,
}

/// The models a `model` option lists, its groups flattened. One that
/// comes without its details (the one in use, when the agent only knows
/// it by its session) stays as `known` had it.
#[must_use]
pub fn listed(option: &Value, known: &[Model]) -> Vec<Model> {
    let entries = option.get("options").and_then(Value::as_array);
    let flat = entries.into_iter().flatten().flat_map(|entry| {
        match entry.get("options").and_then(Value::as_array) {
            Some(group) => group.iter().collect::<Vec<_>>(),
            None => vec![entry],
        }
    });
    flat.filter_map(|choice| {
        let value = choice.get("value")?.as_str()?;
        let meta = choice.pointer("/_meta/unharnessed/model");
        if let (None, Some(known)) = (meta, known.iter().find(|model| model.value == value)) {
            return Some(known.clone());
        }
        let said = |key: &str| meta?.get(key)?.as_str().map(clean);
        // Without details, the value is the `["provider", "id"]` pair.
        let pair: Vec<String> = serde_json::from_str(value).unwrap_or_default();
        let name = choice.get("name").and_then(Value::as_str).map(clean);
        let cost = |key: &str| meta?.pointer(&format!("/cost/{key}"))?.as_f64();
        Some(Model {
            value: value.to_string(),
            id: (said("id").or_else(|| pair.get(1).map(|id| clean(id))))
                .unwrap_or_else(|| clean(value)),
            name: (said("name").or(name)).unwrap_or_else(|| clean(value)),
            provider: (said("provider").or_else(|| pair.first().map(|id| clean(id))))
                .unwrap_or_default(),
            window: (meta.and_then(|meta| meta.get("contextWindow")))
                .map_or(0, |window| window.as_u64().unwrap_or(0)),
            cost: cost("input").zip(cost("output")),
            input: (meta.and_then(|meta| meta.get("input")))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        })
    })
    .collect()
}

/// The row of provider `id` that is in use: Anthropic and xAI have one
/// for their subscription and one for their API key.
fn row<'a>(providers: &'a [Provider], id: &str) -> Option<&'a Provider> {
    let of = || providers.iter().filter(|row| row.key.id == id);
    of().find(|row| row.connected).or_else(|| of().next())
}

/// A provider's name, as its row gives it; its id while there is none.
#[must_use]
pub fn maker(providers: &[Provider], id: &str) -> String {
    row(providers, id).map_or_else(|| clean(id), |row| row.name.clone())
}

/// A subscription pays for the provider's models: they have no price per token.
#[must_use]
pub fn subscribed(providers: &[Provider], id: &str) -> bool {
    row(providers, id).is_some_and(|row| row.connected && row.key.subscription)
}

/// The models whose name, id or provider's name holds every word of
/// `query`; those of provider `first` first, then by provider's name, each
/// provider's in the order the agent gave them.
#[must_use]
pub fn matching<'a>(
    models: &'a [Model],
    query: &str,
    providers: &[Provider],
    first: Option<&str>,
) -> Vec<&'a Model> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<(String, &Model)> = models
        .iter()
        .filter_map(|model| {
            let maker = maker(providers, &model.provider);
            let all = format!("{} {} {maker}", model.name, model.id).to_lowercase();
            words
                .iter()
                .all(|word| all.contains(word))
                .then_some((maker.to_lowercase(), model))
        })
        .collect();
    // Stable: a provider's models stay as they came.
    found.sort_by(|(a_name, a), (b_name, b)| {
        let ahead = |model: &Model| Some(model.provider.as_str()) != first;
        (ahead(a), a_name, &a.provider).cmp(&(ahead(b), b_name, &b.provider))
    });
    found.into_iter().map(|(_, model)| model).collect()
}

/// `1M`, `400K`, `128K`: a count of tokens.
#[must_use]
pub fn tokens(count: u64) -> String {
    match count {
        1_000_000.. => format!("{}M", trimmed(count as f64 / 1e6, 1)),
        1_000.. => format!("{}K", (count as f64 / 1e3).round()),
        _ => count.to_string(),
    }
}

/// `$4`, `$1.25`, `$0.075`: a price.
#[must_use]
pub fn dollars(amount: f64) -> String {
    format!("${}", trimmed(amount, if amount < 1.0 { 3 } else { 2 }))
}

/// `value` to `digits` decimals, without the zeros that end it.
fn trimmed(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

/// What is said of a model beside its name, each in its column: its
/// context window, then what a million tokens cost, in and out, unless a
/// subscription pays (`priced`) or the price is unknown.
#[must_use]
pub fn specs(model: &Model, priced: bool) -> [Option<String>; 3] {
    let window = (model.window > 0).then(|| tokens(model.window));
    let cost = (model.cost).filter(|(input, output)| priced && (*input > 0.0 || *output > 0.0));
    let input = cost.map(|(input, _)| format!("{}/M in", dollars(input)));
    let output = cost.map(|(_, output)| format!("{}/M out", dollars(output)));
    [window, input, output]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::providers::Key;

    fn model(provider: &str, id: &str, name: &str) -> Model {
        Model {
            value: json!([provider, id]).to_string(),
            id: id.to_string(),
            name: name.to_string(),
            provider: provider.to_string(),
            window: 0,
            cost: None,
            input: Vec::new(),
        }
    }

    fn provider(id: &str, name: &str, subscription: bool, connected: bool) -> Provider {
        Provider {
            key: Key {
                id: id.to_string(),
                subscription,
            },
            name: name.to_string(),
            browser: subscription,
            connected,
            removable: connected,
            note: None,
            warning: None,
            local: None,
        }
    }

    fn names(found: &[&Model]) -> Vec<String> {
        found.iter().map(|model| model.name.clone()).collect()
    }

    // How the list could show the wrong models, or in the wrong order: a
    // word found in the provider's name only is not searched; one word of
    // two is enough; the case of what is typed matters; spaces alone hide
    // everything; the provider in use is not first; the others go by their
    // id where their name says otherwise; a provider's models are sorted
    // too, and lose the agent's order; a provider without a row is lost.
    #[test]
    fn the_models_that_match_come_by_provider_the_one_in_use_first() {
        let providers = [
            provider("zeta", "Alpha Labs", false, true),
            provider("openai", "OpenAI", false, true),
            provider("anthropic", "Anthropic", true, false),
            provider("anthropic", "Anthropic", false, true),
        ];
        let models = [
            model("openai", "gpt-5", "GPT-5"),
            model("zeta", "z-2", "Zed Two"),
            model("anthropic", "claude-b", "Claude B"),
            model("openai", "gpt-4", "GPT-4"),
            model("anthropic", "claude-a", "Claude A"),
            model("local", "llama", "Llama"),
        ];
        let all = matching(&models, "  ", &providers, Some("openai"));
        assert_eq!(
            names(&all),
            ["GPT-5", "GPT-4", "Zed Two", "Claude B", "Claude A", "Llama"]
        );
        let none_first = matching(&models, "", &providers, None);
        assert_eq!(
            names(&none_first),
            ["Zed Two", "Claude B", "Claude A", "Llama", "GPT-5", "GPT-4"]
        );
        assert_eq!(
            names(&matching(&models, "ALPHA", &providers, None)),
            ["Zed Two"]
        );
        assert_eq!(
            names(&matching(&models, "gpt 4", &providers, None)),
            ["GPT-4"]
        );
        assert_eq!(
            names(&matching(&models, "claude-a", &providers, None)),
            ["Claude A"]
        );
        assert!(matching(&models, "gpt claude", &providers, None).is_empty());
    }

    // How a model could be read wrong from the option: one in a group is
    // skipped; one without details has no id, no provider and no name; its
    // name keeps the option's when the details have one; a price of which
    // half is missing is taken for free; a control character of a name
    // reaches the screen; one whose details came before and no longer do
    // loses them, and takes the option's name (`One (a)`).
    #[test]
    fn models_are_read_from_the_option_with_or_without_their_details() {
        let option = json!({ "id": "model", "options": [
            { "value": "[\"a\",\"one\"]", "name": "One (a)", "_meta": { "unharnessed": { "model": {
                "id": "one", "name": "One", "provider": "a", "contextWindow": 200_000,
                "input": ["text"], "cost": { "input": 3, "output": 15.5 } } } } },
            { "group": "Others", "options": [
                { "value": "[\"b\",\"two\"]", "name": "Two\u{1b}[31m" },
                { "value": "plain", "name": "Plain", "_meta": { "unharnessed": { "model": {
                    "id": "plain", "name": "Plain", "provider": "c", "cost": { "input": 1 } } } } },
            ] },
        ] });
        let models = listed(&option, &[]);
        assert_eq!(models.len(), 3);
        assert_eq!(
            (models[0].name.as_str(), models[0].window, models[0].cost),
            ("One", 200_000, Some((3.0, 15.5)))
        );
        assert_eq!(
            (models[1].id.as_str(), models[1].provider.as_str()),
            ("two", "b")
        );
        assert!(!models[1].name.contains('\u{1b}') && models[1].name.starts_with("Two"));
        assert_eq!((models[2].window, models[2].cost), (0, None));

        let later = json!({ "id": "model", "options": [
            { "value": "[\"a\",\"one\"]", "name": "One (a)" },
            { "value": "[\"d\",\"new\"]", "name": "New (d)" },
        ] });
        let again = listed(&later, &models);
        assert_eq!(again[0], models[0]);
        assert_eq!(
            (again[1].name.as_str(), again[1].provider.as_str()),
            ("New (d)", "d")
        );
    }

    // How a count or a price could read wrong: a window of 1 048 576 as
    // `1.05M` or `1.0M`; 400 000 as `0.4M`; 128 000 as `128.0K`; a price
    // under a dollar cut to two decimals (`$0.07`); one with zeros kept
    // (`$4.00`, `$1.50`); the float's noise shown (`$0.30000000000000004`);
    // a whole price losing its own zeros (`$3` for 30).
    #[test]
    fn counts_and_prices_are_short() {
        let said: Vec<String> = [
            1_000_000, 1_048_576, 1_500_000, 400_000, 128_000, 8_192, 999,
        ]
        .into_iter()
        .map(tokens)
        .collect();
        assert_eq!(said, ["1M", "1M", "1.5M", "400K", "128K", "8K", "999"]);
        let said: Vec<String> = [4.0, 1.25, 1.5, 0.075, 0.33, 30.0, 0.1 + 0.2, 100.0]
            .into_iter()
            .map(dollars)
            .collect();
        assert_eq!(
            said,
            ["$4", "$1.25", "$1.5", "$0.075", "$0.33", "$30", "$0.3", "$100"]
        );
    }

    // How what is said of a model could mislead: a price shown where a
    // subscription pays; `$0/M` for a model whose price is unknown; a
    // window of 0 shown as `0`; a price without its window lost with it.
    #[test]
    fn a_model_says_its_window_and_its_price_when_it_has_one() {
        let said = |model: &Model, priced| specs(model, priced).map(Option::unwrap_or_default);
        let mut one = model("a", "one", "One");
        assert_eq!(said(&one, true), ["", "", ""]);
        one.window = 200_000;
        one.cost = Some((0.0, 0.0));
        assert_eq!(said(&one, true), ["200K", "", ""]);
        one.cost = Some((3.0, 15.0));
        assert_eq!(said(&one, true), ["200K", "$3/M in", "$15/M out"]);
        assert_eq!(said(&one, false), ["200K", "", ""]);
        one.window = 0;
        assert_eq!(said(&one, true), ["", "$3/M in", "$15/M out"]);
    }
}
