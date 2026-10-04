//! Recipes: reusable command templates.
//!
//! A template uses `{name}` for parameters and `{{` / `}}` for literal braces:
//!
//! ```
//! use protocol::{Recipe, RecipeParam};
//! use std::collections::BTreeMap;
//!
//! let recipe = Recipe {
//!     id: "qwen-vllm".into(),
//!     name: "Qwen vLLM Server".into(),
//!     category: Some("SERVERS".into()),
//!     project: None,
//!     machine_id: None,
//!     command: "vllm serve {model} --port {port}".into(),
//!     params: vec![
//!         RecipeParam::with_default("model", "Qwen/Qwen3-8B"),
//!         RecipeParam::with_default("port", "8000"),
//!     ],
//! };
//! let mut values = BTreeMap::new();
//! values.insert("port".to_string(), "9000".to_string());
//! assert_eq!(recipe.render(&values).unwrap(), "vllm serve Qwen/Qwen3-8B --port 9000");
//! ```
//!
//! **Safety:** placeholders must be standalone, unquoted shell words. Parameter
//! values are then shell-quoted, so a value cannot inject shell syntax.

use crate::{MachineId, ProtocolError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One parameter a recipe accepts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RecipeParam {
    /// Name used in the template, e.g. `port` for `{port}`.
    pub name: String,
    /// Value used when none is given. `None` means the value is required.
    pub default: Option<String>,
    /// Help text shown in the UI.
    pub description: Option<String>,
}

impl RecipeParam {
    /// A required parameter with no default.
    pub fn required(name: impl Into<String>) -> Self {
        RecipeParam {
            name: name.into(),
            default: None,
            description: None,
        }
    }

    /// A parameter with a default value.
    pub fn with_default(name: impl Into<String>, default: impl Into<String>) -> Self {
        RecipeParam {
            name: name.into(),
            default: Some(default.into()),
            description: None,
        }
    }
}

/// A reusable command template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Recipe {
    /// Stable slug, e.g. `"qwen-vllm"`.
    pub id: String,
    /// Display name, e.g. `"Qwen vLLM Server"`.
    pub name: String,
    /// Optional catalog grouping shown in the UI.
    #[serde(default)]
    pub category: Option<String>,
    /// Optional working directory or project label for the Recipe.
    #[serde(default)]
    pub project: Option<String>,
    /// Machine this recipe is meant for; `None` means any machine.
    pub machine_id: Option<MachineId>,
    /// The template, e.g. `"vllm serve {model} --port {port}"`.
    pub command: String,
    /// Declared parameters.
    pub params: Vec<RecipeParam>,
}

/// A piece of a parsed template.
#[derive(Debug, PartialEq)]
enum Part<'a> {
    Text(String),
    Param(&'a str),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ShellQuote {
    #[default]
    Unquoted,
    Single,
    Double,
    Backtick,
}

impl Recipe {
    /// Check the template: braces are balanced and every `{name}` is declared.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let parts = parse_template(&self.command)?;
        let mut quote = ShellQuote::Unquoted;
        for (index, part) in parts.iter().enumerate() {
            match part {
                Part::Text(text) => quote = quote_after(text, quote),
                Part::Param(name) => {
                    if !self.params.iter().any(|p| p.name == *name) {
                        return Err(ProtocolError::UndeclaredPlaceholder(name.to_string()));
                    }
                    let left_safe = index == 0
                        || matches!(&parts[index - 1], Part::Text(text) if text.chars().last().is_some_and(char::is_whitespace));
                    let right_safe = index + 1 == parts.len()
                        || matches!(&parts[index + 1], Part::Text(text) if text.chars().next().is_some_and(char::is_whitespace));
                    if quote != ShellQuote::Unquoted || !left_safe || !right_safe {
                        return Err(ProtocolError::InvalidRecipePlaceholderContext(
                            name.to_string(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Build the final command line.
    ///
    /// `values` overrides defaults. Every given value must be a declared parameter,
    /// and every parameter used in the template must end up with a value.
    pub fn render(&self, values: &BTreeMap<String, String>) -> Result<String, ProtocolError> {
        self.validate()?;
        if let Some(unknown) = values
            .keys()
            .find(|k| !self.params.iter().any(|p| &p.name == *k))
        {
            return Err(ProtocolError::UnknownParam(unknown.clone()));
        }

        let mut out = String::with_capacity(self.command.len());
        for part in parse_template(&self.command)? {
            match part {
                Part::Text(text) => out.push_str(&text),
                Part::Param(name) => {
                    let value = values
                        .get(name)
                        .or_else(|| self.default_for(name))
                        .ok_or_else(|| ProtocolError::MissingParam(name.to_string()))?;
                    if value.chars().any(char::is_control) {
                        return Err(ProtocolError::InvalidRecipeParamValue(name.to_string()));
                    }
                    out.push_str(&shell_quote(value));
                }
            }
        }
        Ok(out)
    }

    fn default_for(&self, name: &str) -> Option<&String> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.default.as_ref())
    }
}

/// Shell quote state after literal template text. Parameter values are safe to
/// interpolate only in the unquoted state; shell quotes inside a value cannot
/// protect it when the template has already opened quotes or backticks.
fn quote_after(text: &str, mut quote: ShellQuote) -> ShellQuote {
    let mut escaped = false;
    for character in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            ShellQuote::Unquoted => match character {
                '\\' => escaped = true,
                '\'' => quote = ShellQuote::Single,
                '"' => quote = ShellQuote::Double,
                '`' => quote = ShellQuote::Backtick,
                _ => {}
            },
            ShellQuote::Single => {
                if character == '\'' {
                    quote = ShellQuote::Unquoted;
                }
            }
            ShellQuote::Double => match character {
                '\\' => escaped = true,
                '"' => quote = ShellQuote::Unquoted,
                _ => {}
            },
            ShellQuote::Backtick => match character {
                '\\' => escaped = true,
                '`' => quote = ShellQuote::Unquoted,
                _ => {}
            },
        }
    }
    quote
}

/// Split a template into literal text and `{param}` parts.
fn parse_template(template: &str) -> Result<Vec<Part<'_>>, ProtocolError> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = template.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '{' if matches!(chars.peek(), Some((_, '{'))) => {
                chars.next();
                text.push('{');
            }
            '}' if matches!(chars.peek(), Some((_, '}'))) => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let rest = &template[i + 1..];
                let end = rest.find('}').ok_or(ProtocolError::UnclosedBrace(i))?;
                let name = rest[..end].trim();
                if name.is_empty() || name.contains('{') {
                    return Err(ProtocolError::UnclosedBrace(i));
                }
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                parts.push(Part::Param(name));
                // Skip past the closing brace.
                for _ in 0..=rest[..end].chars().count() {
                    chars.next();
                }
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(parts)
}

/// Quote a value for POSIX shells, only when needed.
///
/// Plain values like `Qwen/Qwen3-8B` or `8000` are left alone. Anything with
/// spaces or shell characters is wrapped in single quotes.
pub fn shell_quote(value: &str) -> String {
    let is_plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c));
    if is_plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(command: &str, params: Vec<RecipeParam>) -> Recipe {
        Recipe {
            id: "test".into(),
            name: "Test".into(),
            category: None,
            project: None,
            machine_id: None,
            command: command.into(),
            params,
        }
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn vllm() -> Recipe {
        recipe(
            "vllm serve {model} --port {port} --max-model-len {context}",
            vec![
                RecipeParam::with_default("model", "Qwen/Qwen3-8B"),
                RecipeParam::with_default("port", "8000"),
                RecipeParam::with_default("context", "32768"),
            ],
        )
    }

    #[test]
    fn render_uses_defaults_when_no_values_given() {
        assert_eq!(
            vllm().render(&BTreeMap::new()).unwrap(),
            "vllm serve Qwen/Qwen3-8B --port 8000 --max-model-len 32768"
        );
    }

    #[test]
    fn render_prefers_given_values_over_defaults() {
        let out = vllm().render(&values(&[("port", "9000")])).unwrap();
        assert_eq!(
            out,
            "vllm serve Qwen/Qwen3-8B --port 9000 --max-model-len 32768"
        );
    }

    #[test]
    fn render_fails_on_missing_required_param() {
        let r = recipe(
            "python train.py --config {config}",
            vec![RecipeParam::required("config")],
        );
        assert_eq!(
            r.render(&BTreeMap::new()),
            Err(ProtocolError::MissingParam("config".into()))
        );
    }

    #[test]
    fn render_fails_on_unknown_param() {
        assert_eq!(
            vllm().render(&values(&[("prot", "9000")])),
            Err(ProtocolError::UnknownParam("prot".into()))
        );
    }

    #[test]
    fn render_rejects_terminal_control_characters_in_values() {
        assert_eq!(
            vllm().render(&values(&[("model", "safe\nsecond-command")])),
            Err(ProtocolError::InvalidRecipeParamValue("model".into()))
        );
    }

    #[test]
    fn validate_fails_on_undeclared_placeholder() {
        let r = recipe("run {missing}", vec![]);
        assert_eq!(
            r.validate(),
            Err(ProtocolError::UndeclaredPlaceholder("missing".into()))
        );
    }

    #[test]
    fn validate_fails_on_unclosed_brace() {
        let r = recipe("run {oops", vec![]);
        assert_eq!(r.validate(), Err(ProtocolError::UnclosedBrace(4)));
    }

    #[test]
    fn validate_fails_on_empty_placeholder() {
        assert!(recipe("run {}", vec![]).validate().is_err());
    }

    #[test]
    fn placeholder_inside_quotes_cannot_turn_data_into_shell_substitution() {
        for template in [
            "printf '%s\\n' \"{value}\"",
            "printf \"prefix {value} suffix\\n\"",
            "printf 'prefix {value} suffix\\n'",
            "printf `echo {value}`",
        ] {
            let r = recipe(template, vec![RecipeParam::required("value")]);
            assert_eq!(
                r.render(&values(&[("value", "$(printf AUDIT_MARKER)")])),
                Err(ProtocolError::InvalidRecipePlaceholderContext(
                    "value".into()
                )),
                "{template}"
            );
        }
    }

    #[test]
    fn placeholder_adjacent_to_assignment_or_path_is_rejected() {
        for template in [
            "PORT={value} server",
            "cat /tmp/{value}",
            "echo {value}.txt",
        ] {
            let r = recipe(template, vec![RecipeParam::required("value")]);
            assert!(matches!(
                r.validate(),
                Err(ProtocolError::InvalidRecipePlaceholderContext(_))
            ));
        }
    }

    #[test]
    fn double_braces_render_as_literal_braces() {
        let r = recipe(
            "echo {{x}} {name}",
            vec![RecipeParam::with_default("name", "hi")],
        );
        assert_eq!(r.render(&BTreeMap::new()).unwrap(), "echo {x} hi");
    }

    #[test]
    fn template_without_params_renders_unchanged() {
        let r = recipe("nvidia-smi", vec![]);
        assert_eq!(r.render(&BTreeMap::new()).unwrap(), "nvidia-smi");
    }

    #[test]
    fn same_param_can_appear_twice() {
        let r = recipe("echo {x} {x}", vec![RecipeParam::with_default("x", "a")]);
        assert_eq!(r.render(&BTreeMap::new()).unwrap(), "echo a a");
    }

    #[test]
    fn unicode_text_around_params_is_preserved() {
        let r = recipe("echo ✓ {x} ✓", vec![RecipeParam::with_default("x", "ok")]);
        assert_eq!(r.render(&BTreeMap::new()).unwrap(), "echo ✓ ok ✓");
    }

    #[test]
    fn values_with_spaces_are_quoted() {
        let r = recipe("echo {msg}", vec![RecipeParam::required("msg")]);
        let out = r.render(&values(&[("msg", "hello world")])).unwrap();
        assert_eq!(out, "echo 'hello world'");
    }

    #[test]
    fn injection_attempt_is_quoted_not_executed() {
        let r = recipe("echo {msg}", vec![RecipeParam::required("msg")]);
        let out = r.render(&values(&[("msg", "x; rm -rf ~")])).unwrap();
        assert_eq!(out, "echo 'x; rm -rf ~'");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn shell_quote_quotes_empty_string() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn shell_quote_leaves_plain_values_alone() {
        for v in [
            "8000",
            "Qwen/Qwen3-8B",
            "./ckpt_07.pt",
            "--flag=on",
            "user@host:22",
        ] {
            assert_eq!(shell_quote(v), v);
        }
    }
}
