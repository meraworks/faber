//! Parameter expressions. Angles are in degrees.

use std::collections::{BTreeMap, BTreeSet};

/// Constants. Names are case-sensitive. A parameter cannot use these names.
const CONSTANTS: &[(&str, f64)] = &[("pi", std::f64::consts::PI)];

/// Functions and their argument counts. Used in error texts.
const FUNCTIONS: &[(&str, usize)] = &[
    ("sin", 1),
    ("cos", 1),
    ("tan", 1),
    ("asin", 1),
    ("acos", 1),
    ("atan", 1),
    ("atan2", 2),
    ("sqrt", 1),
    ("abs", 1),
    ("round", 1),
    ("min", 2),
    ("max", 2),
    ("if", 3),
];

fn function_list() -> String {
    FUNCTIONS
        .iter()
        .map(|(name, count)| format!("{name}({count})"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn resolve(expressions: &BTreeMap<String, String>) -> Result<BTreeMap<String, f64>, String> {
    fn visit(
        name: &str,
        expressions: &BTreeMap<String, String>,
        values: &mut BTreeMap<String, f64>,
        active: &mut BTreeSet<String>,
    ) -> Result<f64, String> {
        if let Some(value) = values.get(name) {
            return Ok(*value);
        }
        if active.len() >= 128 {
            return Err("Parameter dependency chain is too deep".into());
        }
        if !active.insert(name.to_owned()) {
            return Err(format!("Parameter cycle: {name}"));
        }
        let expression = expressions
            .get(name)
            .ok_or_else(|| format!("Unknown parameter: {name}"))?;
        let value = evaluate(expression, |dependency| {
            visit(dependency, expressions, values, active)
        })
        .map_err(|error| format!("Parameter {name}: {error}"))?;
        active.remove(name);
        values.insert(name.to_owned(), value);
        Ok(value)
    }
    let mut values = BTreeMap::new();
    let mut active = BTreeSet::new();
    for name in expressions.keys() {
        if name.is_empty()
            || !name
                .bytes()
                .enumerate()
                .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
        {
            return Err(format!("Invalid parameter name: {name}"));
        }
        if CONSTANTS.iter().any(|(constant, _)| *constant == name) {
            return Err(format!(
                "Parameter name is reserved: {name}. Use another name."
            ));
        }
        visit(name, expressions, &mut values, &mut active)?;
    }
    Ok(values)
}

pub fn evaluate(
    source: &str,
    lookup: impl FnMut(&str) -> Result<f64, String>,
) -> Result<f64, String> {
    if source.len() > 8192 {
        return Err("Expression is too long".into());
    }
    let mut parser = Parser {
        source: source.as_bytes(),
        position: 0,
        lookup,
        depth: 0,
        enabled: true,
    };
    let result = parser.expression(0)?;
    parser.space();
    if parser.position != parser.source.len() {
        return Err("Unexpected text after expression".into());
    }
    finite(result)
}

/// Refuses arguments outside the domain of a function.
fn check_domain(name: &str, arguments: &[f64]) -> Result<(), String> {
    match (name, arguments) {
        ("asin" | "acos", [a]) if !(-1. ..=1.).contains(a) => {
            Err(format!("Function {name}: argument must be from -1 to 1"))
        }
        _ => Ok(()),
    }
}

/// Adds a hint when an unknown name is close to a constant.
fn unknown_name_hint(error: String, name: &str) -> String {
    match CONSTANTS
        .iter()
        .find(|(constant, _)| constant.eq_ignore_ascii_case(name))
    {
        Some((constant, _)) => format!(
            "{error}. Did you mean `{constant}`? Constants: {}",
            CONSTANTS
                .iter()
                .map(|(constant, _)| *constant)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => error,
    }
}

fn finite(value: f64) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Expression result is not finite".into())
    }
}

struct Parser<'a, F> {
    source: &'a [u8],
    position: usize,
    lookup: F,
    depth: usize,
    enabled: bool,
}

impl<F: FnMut(&str) -> Result<f64, String>> Parser<'_, F> {
    fn space(&mut self) {
        while self
            .source
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.space();
        if self.source.get(self.position) == Some(&byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expression(&mut self, minimum: u8) -> Result<f64, String> {
        if self.depth >= 128 {
            return Err("Expression is too deep".into());
        }
        self.depth += 1;
        let result = self.inner(minimum);
        self.depth -= 1;
        result
    }

    fn inner(&mut self, minimum: u8) -> Result<f64, String> {
        let mut compared = false;
        let mut left = if self.take(b'-') {
            -self.expression(3)?
        } else if self.take(b'+') {
            self.expression(3)?
        } else {
            self.atom()?
        };
        loop {
            self.space();
            let Some(&operator) = self.source.get(self.position) else {
                break;
            };
            let (binding, right_binding) = match operator {
                b'+' | b'-' => (1, 2),
                b'*' | b'/' => (2, 3),
                b'^' => (4, 4),
                b'<' | b'>' | b'=' | b'!' => (0, 1),
                _ => break,
            };
            if binding < minimum {
                break;
            }
            self.position += 1;
            let comparison = binding == 0;
            let equal = comparison && self.source.get(self.position) == Some(&b'=');
            if comparison {
                if compared {
                    return Err("Use brackets around chained comparisons".into());
                }
                compared = true;
                if equal {
                    self.position += 1;
                }
                if matches!(operator, b'=' | b'!') && !equal {
                    return Err("Expected == or !=".into());
                }
            }
            let right = self.expression(right_binding)?;
            if !self.enabled {
                left = 0.;
                continue;
            }
            left = finite(match operator {
                b'+' => left + right,
                b'-' => left - right,
                b'*' => left * right,
                b'/' => left / right,
                b'<' => f64::from(if equal { left <= right } else { left < right }),
                b'>' => f64::from(if equal { left >= right } else { left > right }),
                b'=' => f64::from(left == right),
                b'!' => f64::from(left != right),
                _ => left.powf(right),
            })?;
        }
        finite(left)
    }

    fn atom(&mut self) -> Result<f64, String> {
        if self.take(b'(') {
            let value = self.expression(0)?;
            if !self.take(b')') {
                return Err("Expected closing bracket".into());
            }
            return Ok(value);
        }
        self.space();
        let start = self.position;
        if self
            .source
            .get(start)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            while self
                .source
                .get(self.position)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            {
                self.position += 1;
            }
            if self
                .source
                .get(self.position)
                .is_some_and(|c| *c == b'e' || *c == b'E')
            {
                self.position += 1;
                if self
                    .source
                    .get(self.position)
                    .is_some_and(|c| *c == b'+' || *c == b'-')
                {
                    self.position += 1;
                }
                while self
                    .source
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
            }
            return finite(self.text(start)?.parse().map_err(|_| "Invalid number")?);
        }
        if !self
            .source
            .get(start)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            return Err("Expected number, name or bracket".into());
        }
        self.position += 1;
        while self
            .source
            .get(self.position)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.position += 1;
        }
        let name = self.text(start)?.to_owned();
        if !self.take(b'(') {
            if let Some((_, value)) = CONSTANTS.iter().find(|(constant, _)| *constant == name) {
                return Ok(*value);
            }
            return if self.enabled {
                finite((self.lookup)(&name).map_err(|error| unknown_name_hint(error, &name))?)
            } else {
                Ok(0.)
            };
        }
        if name == "if" {
            let enabled = self.enabled;
            let condition = self.expression(0)?;
            if !self.take(b',') {
                return Err("Expected three arguments for if".into());
            }
            self.enabled = enabled && condition != 0.;
            let yes = self.expression(0)?;
            if !self.take(b',') {
                return Err("Expected three arguments for if".into());
            }
            self.enabled = enabled && condition == 0.;
            let no = self.expression(0)?;
            self.enabled = enabled;
            if !self.take(b')') {
                return Err("Expected closing bracket after if".into());
            }
            return Ok(if condition != 0. { yes } else { no });
        }
        let mut arguments = Vec::new();
        if !self.take(b')') {
            loop {
                arguments.push(self.expression(0)?);
                if self.take(b')') {
                    break;
                }
                if arguments.len() >= 3 || !self.take(b',') {
                    return Err("Invalid function arguments".into());
                }
            }
        }
        match FUNCTIONS.iter().find(|(function, _)| *function == name) {
            None => {
                return Err(format!(
                    "Unknown function `{name}`. Functions: {}",
                    function_list()
                ));
            }
            Some((_, count)) if *count != arguments.len() => {
                return Err(format!(
                    "Wrong argument count for `{name}`. Functions: {}",
                    function_list()
                ));
            }
            Some(_) => {}
        }
        if !self.enabled {
            return Ok(0.);
        }
        check_domain(&name, &arguments)?;
        let result = match (name.as_str(), arguments.as_slice()) {
            ("sin", [a]) => a.to_radians().sin(),
            ("cos", [a]) => a.to_radians().cos(),
            ("tan", [a]) => a.to_radians().tan(),
            ("asin", [a]) => a.asin().to_degrees(),
            ("acos", [a]) => a.acos().to_degrees(),
            ("atan", [a]) => a.atan().to_degrees(),
            ("atan2", [y, x]) if *y == 0. && *x == 0. => 0.,
            ("atan2", [y, x]) => y.atan2(*x).to_degrees(),
            ("sqrt", [a]) => a.sqrt(),
            ("abs", [a]) => a.abs(),
            ("round", [a]) => a.round(),
            ("min", [a, b]) => a.min(*b),
            ("max", [a, b]) => a.max(*b),
            _ => {
                return Err(format!(
                    "Unknown function `{name}`. Functions: {}",
                    function_list()
                ));
            }
        };
        finite(result)
    }

    fn text(&self, start: usize) -> Result<&str, String> {
        std::str::from_utf8(&self.source[start..self.position])
            .map_err(|_| "Invalid expression text".into())
    }
}
