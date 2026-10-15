//! `{{name}}` templates and variable scopes.
//!
//! Names are `[A-Za-z0-9_.-]+`, optionally surrounded by spaces inside the
//! braces. Anything else between `{{` and `}}` is kept as literal text.
//! Values are inserted literally: a value containing `{{x}}` is not expanded
//! again (no recursion).

use super::model::Variable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment<'a> {
    Text(&'a str),
    Var(&'a str),
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn parse(template: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut rest = template;
    let mut text_start = 0usize;
    let mut offset = 0usize;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim();
        let consumed = open + 2 + close + 2;
        if valid_name(name) {
            let abs_open = offset + open;
            if abs_open > text_start {
                out.push(Segment::Text(&template[text_start..abs_open]));
            }
            out.push(Segment::Var(name));
            text_start = offset + consumed;
        }
        // Invalid names stay literal; retry one char later so `{{{x}}}`
        // still finds `{{x}}`.
        let advance = if valid_name(name) { consumed } else { open + 1 };
        offset += advance;
        rest = &rest[advance..];
    }
    if text_start < template.len() {
        out.push(Segment::Text(&template[text_start..]));
    }
    out
}

/// Names referenced by a template, in order.
pub fn names(template: &str) -> Vec<&str> {
    parse(template)
        .into_iter()
        .filter_map(|s| match s {
            Segment::Var(n) => Some(n),
            Segment::Text(_) => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Request,
    Environment,
    Project,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::Request => "request",
            Scope::Environment => "environment",
            Scope::Project => "project",
        }
    }
}

/// Variable lookup with precedence request > active environment > project.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scopes<'a> {
    pub request: &'a [Variable],
    pub environment: &'a [Variable],
    pub project: &'a [Variable],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<'a> {
    Found {
        value: &'a str,
        secret: bool,
        scope: Scope,
    },
    /// Declared as secret but no value is available in this session.
    MissingSecret(Scope),
    Unknown,
}

impl<'a> Scopes<'a> {
    pub fn lookup(&self, name: &str) -> Lookup<'a> {
        for (vars, scope) in [
            (self.request, Scope::Request),
            (self.environment, Scope::Environment),
            (self.project, Scope::Project),
        ] {
            if let Some(v) = vars.iter().find(|v| v.name == name) {
                if v.secret && v.value.is_empty() {
                    return Lookup::MissingSecret(scope);
                }
                return Lookup::Found {
                    value: &v.value,
                    secret: v.secret,
                    scope,
                };
            }
        }
        Lookup::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    /// Secret values that were inserted (to redact them from messages).
    pub secrets: Vec<String>,
}

/// Expands a template. Returns the names that could not be resolved, each
/// with a reason, if any.
pub fn expand(template: &str, scopes: &Scopes<'_>) -> Result<Expanded, Vec<(String, String)>> {
    let mut text = String::with_capacity(template.len());
    let mut secrets = Vec::new();
    let mut errors = Vec::new();
    for seg in parse(template) {
        match seg {
            Segment::Text(t) => text.push_str(t),
            Segment::Var(name) => match scopes.lookup(name) {
                Lookup::Found { value, secret, .. } => {
                    text.push_str(value);
                    if secret && !value.is_empty() {
                        secrets.push(value.to_owned());
                    }
                }
                Lookup::MissingSecret(scope) => errors.push((
                    name.to_owned(),
                    format!(
                        "secret {} variable has no value in this session",
                        scope.label()
                    ),
                )),
                Lookup::Unknown => errors.push((name.to_owned(), "unknown variable".to_owned())),
            },
        }
    }
    if errors.is_empty() {
        Ok(Expanded { text, secrets })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_and_keeps_invalid_braces_literal() {
        assert_eq!(
            parse("{{base_url}}/users/{{ id }}?q={{not valid}}&x={{"),
            vec![
                Segment::Var("base_url"),
                Segment::Text("/users/"),
                Segment::Var("id"),
                Segment::Text("?q={{not valid}}&x={{"),
            ]
        );
        assert_eq!(names("{{a}}{{b.c-d}}"), vec!["a", "b.c-d"]);
        assert_eq!(parse("plain"), vec![Segment::Text("plain")]);
        assert_eq!(
            parse("{{{x}}}"),
            vec![Segment::Text("{"), Segment::Var("x"), Segment::Text("}")]
        );
    }

    #[test]
    fn precedence_request_then_environment_then_project() {
        let request = [Variable::plain("a", "req")];
        let environment = [Variable::plain("a", "env"), Variable::plain("b", "env")];
        let project = [
            Variable::plain("a", "proj"),
            Variable::plain("b", "proj"),
            Variable::plain("c", "proj"),
        ];
        let scopes = Scopes {
            request: &request,
            environment: &environment,
            project: &project,
        };
        let e = expand("{{a}}-{{b}}-{{c}}", &scopes).unwrap();
        assert_eq!(e.text, "req-env-proj");
    }

    #[test]
    fn unknown_and_missing_secret_are_errors_and_values_are_not_recursive() {
        let env = [Variable::secret("token", ""), Variable::plain("x", "{{y}}")];
        let scopes = Scopes {
            environment: &env,
            ..Scopes::default()
        };
        let err = expand("{{token}} {{nope}}", &scopes).unwrap_err();
        assert_eq!(err.len(), 2);
        assert!(err[0].1.contains("secret"));
        assert_eq!(err[1], ("nope".to_owned(), "unknown variable".to_owned()));
        assert_eq!(expand("{{x}}", &scopes).unwrap().text, "{{y}}");

        let env = [Variable::secret("token", "s3cr3t")];
        let scopes = Scopes {
            environment: &env,
            ..Scopes::default()
        };
        let e = expand("Bearer {{token}}", &scopes).unwrap();
        assert_eq!(e.secrets, vec!["s3cr3t".to_owned()]);
    }
}
