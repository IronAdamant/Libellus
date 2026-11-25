//! Tool definitions shared by the MCP server and the CLI.
//!
//! One table describes every tool: its name, group, parameters, and handler.
//! The JSON schema, the argument checks, and the CLI flags all come from it,
//! so they cannot drift apart. Unknown arguments are errors, never ignored.

use crate::json::Value;

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Str,
    UInt,
    Bool,
    StrList,
    /// One JSON object whose keys are these parameters.
    Object(&'static [Param]),
    /// An array of up to `max` JSON objects whose keys are these parameters.
    /// This is how a tool takes a batch in one call.
    Objects(&'static [Param], usize),
}

#[derive(Debug)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub doc: &'static str,
    /// Allowed values for a `Str` parameter; empty means free text.
    pub choices: &'static [&'static str],
}

pub struct Tool {
    pub name: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    pub params: &'static [Param],
    pub run: fn(&Args) -> Result<Value, String>,
}

/// When `agent` is required and missing, use `LIBELLUS_AGENT`. A present
/// value, including an empty one, is left alone. Tools where `agent` is
/// optional (the all-agent ledger) are not filled.
pub fn fill_agent_default(tool: &Tool, args: Value) -> Value {
    let needs = tool.params.iter().any(|p| p.name == "agent" && p.required);
    if !needs {
        return args;
    }
    let mut pairs = match args {
        Value::Object(pairs) => pairs,
        Value::Null => Vec::new(),
        other => return other,
    };
    let present = pairs
        .iter()
        .any(|(k, v)| k == "agent" && !matches!(v, Value::Null));
    if !present {
        if let Some(name) = std::env::var("LIBELLUS_AGENT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            pairs.retain(|(k, _)| k != "agent");
            pairs.push(("agent".to_string(), Value::str(name)));
        }
    }
    Value::Object(pairs)
}

/// Arguments that passed `Tool::validate`.
pub struct Args {
    pairs: Vec<(String, Value)>,
}

/// Longest single string argument, in bytes.
pub const MAX_STRING: usize = 20_000;
/// Most items in a list argument.
pub const MAX_LIST: usize = 50;

impl Args {
    pub fn str(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(Value::as_str)
    }

    pub fn uint(&self, name: &str) -> Option<u64> {
        self.get(name).and_then(Value::as_u64)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(Value::as_bool)
    }

    pub fn list(&self, name: &str) -> Vec<&str> {
        self.get(name)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// The items of an `Objects` argument, already checked against its fields.
    pub fn objects(&self, name: &str) -> Option<&[Value]> {
        self.get(name).and_then(Value::as_array)
    }

    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// A field of a nested object, with JSON `null` read as absent.
pub fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !matches!(x, Value::Null))
}

pub fn field_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    field(v, key).and_then(Value::as_str)
}

pub fn field_list(v: &Value, key: &str) -> Option<Vec<String>> {
    field(v, key).and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect()
    })
}

fn param_schema(p: &Param) -> Value {
    let mut f = vec![(
        "type",
        Value::str(match p.kind {
            Kind::Str => "string",
            Kind::UInt => "integer",
            Kind::Bool => "boolean",
            Kind::StrList | Kind::Objects(..) => "array",
            Kind::Object(_) => "object",
        }),
    )];
    match p.kind {
        Kind::StrList => {
            f.push(("items", Value::obj(vec![("type", Value::str("string"))])));
        }
        Kind::UInt => {
            let min = if p.name == "limit" { 1 } else { 0 };
            f.push(("minimum", Value::int(min)));
        }
        Kind::Object(fields) => {
            // `type` is already set above; a second one is a duplicate key.
            for (k, v) in object_schema(fields) {
                if k != "type" {
                    f.push((k, v));
                }
            }
        }
        Kind::Objects(fields, max) => {
            f.push(("items", Value::obj(object_schema(fields))));
            f.push(("maxItems", Value::uint(max as u64)));
        }
        Kind::Str | Kind::Bool => {}
    }
    if !p.choices.is_empty() {
        f.push((
            "enum",
            Value::Array(p.choices.iter().map(|c| Value::str(*c)).collect()),
        ));
    }
    // A batch item's fields leave `doc` empty: the parent describes the item
    // once, so the schema does not repeat what the single form already says.
    if !p.doc.is_empty() {
        f.push(("description", Value::str(p.doc)));
    }
    Value::obj(f)
}

fn object_schema(params: &[Param]) -> Vec<(&'static str, Value)> {
    let props: Vec<(String, Value)> = params
        .iter()
        .map(|p| (p.name.to_string(), param_schema(p)))
        .collect();
    let required: Vec<Value> = params
        .iter()
        .filter(|p| p.required)
        .map(|p| Value::str(p.name))
        .collect();
    let mut s = vec![
        ("type", Value::str("object")),
        ("properties", Value::Object(props)),
        ("additionalProperties", Value::Bool(false)),
    ];
    if !required.is_empty() {
        s.push(("required", Value::Array(required)));
    }
    s
}

/// Check one nested object against `params`. `at` names it in errors
/// (`notes[2]`).
fn check_object(params: &[Param], v: &Value, at: &str) -> Result<(), String> {
    let Value::Object(pairs) = v else {
        return Err(format!("{at} must be an object, got {}", v.type_name()));
    };
    let unknown: Vec<&str> = pairs
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !params.iter().any(|p| p.name == *k))
        .collect();
    if !unknown.is_empty() {
        let valid: Vec<&str> = params.iter().map(|p| p.name).collect();
        return Err(format!(
            "{at}: unknown key(s) {}. Valid: {}",
            unknown.join(", "),
            valid.join(", ")
        ));
    }
    for p in params {
        match pairs.iter().find(|(k, _)| k == p.name).map(|(_, v)| v) {
            None | Some(Value::Null) if p.required => {
                return Err(format!("{at}: missing {}", p.name));
            }
            None | Some(Value::Null) => {}
            Some(v) => check_value(p, v).map_err(|e| format!("{at}.{e}"))?,
        }
    }
    Ok(())
}

impl Tool {
    pub fn schema(&self) -> Value {
        Value::obj(object_schema(self.params))
    }

    /// Check `args` against the parameter table. Unknown fields, wrong types,
    /// missing required fields, and values outside `choices` are all errors.
    pub fn validate(&self, args: &Value) -> Result<Args, String> {
        let pairs = match args {
            Value::Object(p) => p.clone(),
            Value::Null => Vec::new(),
            other => {
                return Err(format!(
                    "arguments must be an object, got {}",
                    other.type_name()
                ))
            }
        };
        let unknown: Vec<&str> = pairs
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !self.params.iter().any(|p| p.name == *k))
            .collect();
        if !unknown.is_empty() {
            let valid: Vec<&str> = self.params.iter().map(|p| p.name).collect();
            return Err(format!(
                "unknown argument(s) for {}: {}. Valid: {}",
                self.name,
                unknown.join(", "),
                valid.join(", ")
            ));
        }
        for p in self.params {
            let v = pairs.iter().find(|(k, _)| k == p.name).map(|(_, v)| v);
            match v {
                None | Some(Value::Null) if p.required => {
                    if p.name == "agent" {
                        return Err(
                            "missing required argument: agent. Pass agent, or set LIBELLUS_AGENT."
                                .into(),
                        );
                    }
                    return Err(format!("missing required argument: {}", p.name));
                }
                None | Some(Value::Null) => {}
                Some(v) => check_value(p, v)?,
            }
        }
        let pairs = pairs
            .into_iter()
            .filter(|(_, v)| *v != Value::Null)
            .collect();
        Ok(Args { pairs })
    }
}

fn check_value(p: &Param, v: &Value) -> Result<(), String> {
    let bad = |want: &str| Err(format!("{} must be {want}, got {}", p.name, v.type_name()));
    match (&p.kind, v) {
        (Kind::Str, Value::String(s)) => {
            if s.len() > MAX_STRING {
                return Err(format!("{} is longer than {MAX_STRING} bytes", p.name));
            }
            if !p.choices.is_empty() && !p.choices.contains(&s.as_str()) {
                return Err(format!(
                    "{} must be one of: {}",
                    p.name,
                    p.choices.join(", ")
                ));
            }
            Ok(())
        }
        (Kind::Str, _) => bad("a string"),
        (Kind::UInt, v) if v.as_u64().is_some() => {
            let n = v.as_u64().unwrap();
            if p.name == "limit" && n < 1 {
                return Err(format!(
                    "limit is {n}. It must be a whole number from 1 up to 18446744073709551615."
                ));
            }
            Ok(())
        }
        (Kind::UInt, _) => {
            let least = if p.name == "limit" { 1 } else { 0 };
            Err(format!(
                "{} must be a whole number from {least} up to 18446744073709551615",
                p.name
            ))
        }
        (Kind::Bool, Value::Bool(_)) => Ok(()),
        (Kind::Bool, _) => bad("a boolean"),
        (Kind::StrList, Value::Array(items)) => {
            if items.len() > MAX_LIST {
                return Err(format!("{} has more than {MAX_LIST} items", p.name));
            }
            for item in items {
                match item {
                    Value::String(s) if s.len() <= MAX_STRING => {}
                    _ => return Err(format!("{} must be an array of strings", p.name)),
                }
            }
            Ok(())
        }
        (Kind::StrList, _) => bad("an array of strings"),
        (Kind::Object(fields), v) => check_object(fields, v, p.name),
        (Kind::Objects(fields, max), Value::Array(items)) => {
            if items.len() > *max {
                return Err(format!(
                    "{} has {} items; at most {max} fit in one call",
                    p.name,
                    items.len()
                ));
            }
            // Every bad item is named, not just the first, so one retry fixes them all.
            let bad: Vec<String> = items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    check_object(fields, item, &format!("{}[{i}]", p.name)).err()
                })
                .collect();
            if bad.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "nothing written: {} of {} {} are invalid. {}",
                    bad.len(),
                    items.len(),
                    p.name,
                    bad.join("; ")
                ))
            }
        }
        (Kind::Objects(..), _) => bad("an array of objects"),
    }
}
