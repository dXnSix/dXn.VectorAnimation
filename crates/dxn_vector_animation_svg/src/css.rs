//! The CSS an SVG's `<style>` sheets and `style` attributes hold: rules of
//! simple selectors, declarations, and `@keyframes`.

use std::collections::HashMap;

/// A parsed style sheet.
#[derive(Debug, Default)]
pub(crate) struct Stylesheet {
    pub(crate) rules: Vec<Rule>,
    /// `@keyframes` by name (a later definition replaces an earlier one).
    pub(crate) keyframes: HashMap<String, Vec<KeyframeBlock>>,
}

/// A style rule.
#[derive(Debug)]
pub(crate) struct Rule {
    pub(crate) selectors: Vec<Selector>,
    pub(crate) declarations: Vec<Declaration>,
    /// Its position among every rule of every sheet.
    pub(crate) order: usize,
}

/// A compound selector: `tag`, `#id`, `.class` and `*` together.
#[derive(Debug, Default)]
pub(crate) struct Selector {
    pub(crate) tag: Option<String>,
    pub(crate) id: Option<String>,
    pub(crate) classes: Vec<String>,
}

impl Selector {
    /// CSS specificity: (ids, classes, tags).
    pub(crate) fn specificity(&self) -> (u32, u32, u32) {
        (
            u32::from(self.id.is_some()),
            self.classes.len() as u32,
            u32::from(self.tag.is_some()),
        )
    }

    pub(crate) fn matches(&self, node: roxmltree::Node<'_, '_>) -> bool {
        if self
            .tag
            .as_deref()
            .is_some_and(|t| t != node.tag_name().name())
        {
            return false;
        }
        if let Some(id) = &self.id
            && node.attribute("id") != Some(id.as_str())
        {
            return false;
        }
        if !self.classes.is_empty() {
            let classes: Vec<&str> = node
                .attribute("class")
                .map(|c| c.split_ascii_whitespace().collect())
                .unwrap_or_default();
            if !self.classes.iter().all(|c| classes.contains(&c.as_str())) {
                return false;
            }
        }
        true
    }
}

/// One `name: value` pair.
#[derive(Debug, Clone)]
pub(crate) struct Declaration {
    /// Lower case.
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) important: bool,
}

/// One block of a `@keyframes` rule.
#[derive(Debug, Clone)]
pub(crate) struct KeyframeBlock {
    /// Its selectors as fractions (`from` 0, `50%` 0.5, `to` 1).
    pub(crate) offsets: Vec<f64>,
    pub(crate) declarations: Vec<Declaration>,
}

/// `text` without its comments.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Splits `text` at `sep` where it is outside strings, parentheses and
/// braces.
pub(crate) fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (_, '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            (None, c) if c == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// The declarations of a block (`fill: red; opacity: .5`).
pub(crate) fn parse_declarations(text: &str) -> Vec<Declaration> {
    let text = strip_comments(text);
    split_top(&text, ';')
        .into_iter()
        .filter_map(|part| {
            let (name, value) = part.split_once(':')?;
            let name = name.trim().to_ascii_lowercase();
            let mut value = value.trim();
            let mut important = false;
            if let Some(i) = value.to_ascii_lowercase().rfind("!important") {
                value = value[..i].trim_end();
                important = true;
            }
            (!name.is_empty() && !value.is_empty()).then(|| Declaration {
                name,
                value: value.to_owned(),
                important,
            })
        })
        .collect()
}

/// Reads the block that starts after the `{` at `open` in `text`: its
/// content and the index after its `}`.
fn block(text: &str, open: usize) -> (&str, usize) {
    let (mut depth, mut quote) = (1i32, None);
    for (i, c) in text[open + 1..].char_indices() {
        let at = open + 1 + i;
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth -= 1;
                if depth == 0 {
                    return (&text[open + 1..at], at + 1);
                }
            }
            _ => {}
        }
    }
    (&text[open + 1..], text.len())
}

impl Stylesheet {
    /// Adds the rules of sheet `text`. What it cannot use is reported to
    /// `warn`.
    pub(crate) fn add(&mut self, text: &str, warn: &mut dyn FnMut(String)) {
        let text = strip_comments(text);
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            let Some(open) = rest.find(['{', ';']) else {
                break;
            };
            let prelude = rest[..open].trim();
            if rest[open..].starts_with(';') {
                // `@import`, `@charset` and stray semicolons.
                if prelude.starts_with("@import") {
                    warn("its style sheet imports another, which is left out".to_owned());
                }
                at += open + 1;
                continue;
            }
            let (content, end) = block(rest, open);
            at += end;
            if let Some(at_rule) = prelude.strip_prefix('@') {
                let (name, rest) = at_rule
                    .split_once(char::is_whitespace)
                    .unwrap_or((at_rule, ""));
                let name = name.to_ascii_lowercase();
                if name == "keyframes" || name == "-webkit-keyframes" {
                    let blocks = parse_keyframes(content);
                    self.keyframes
                        .insert(unquote(rest.trim()).to_owned(), blocks);
                } else {
                    warn(format!("its style sheet's @{name} rules are left out"));
                }
                continue;
            }
            let mut selectors = Vec::new();
            for sel in prelude.split(',') {
                match parse_selector(sel.trim()) {
                    Some(s) => selectors.push(s),
                    None => warn(format!(
                        "CSS selector `{}` is not supported (simple selectors only): its rule is left out",
                        sel.trim()
                    )),
                }
            }
            if !selectors.is_empty() {
                let order = self.rules.len();
                self.rules.push(Rule {
                    selectors,
                    declarations: parse_declarations(content),
                    order,
                });
            }
        }
    }
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(s)
}

fn parse_keyframes(content: &str) -> Vec<KeyframeBlock> {
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < content.len() {
        let rest = &content[at..];
        let Some(open) = rest.find('{') else { break };
        let prelude = &rest[..open];
        let (body, end) = block(rest, open);
        at += end;
        let offsets: Vec<f64> = prelude
            .split(',')
            .filter_map(|s| match s.trim().to_ascii_lowercase().as_str() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                p => p
                    .strip_suffix('%')
                    .and_then(|n| n.trim().parse::<f64>().ok())
                    .filter(|n| (0.0..=100.0).contains(n))
                    .map(|n| n / 100.0),
            })
            .collect();
        if !offsets.is_empty() {
            blocks.push(KeyframeBlock {
                offsets,
                declarations: parse_declarations(body),
            });
        }
    }
    blocks
}

/// A compound selector, or `None` for what this reader does not match
/// (combinators, attributes, pseudo-classes).
fn parse_selector(text: &str) -> Option<Selector> {
    if text.is_empty() || text.contains(|c: char| c.is_whitespace() || "[]:>+~".contains(c)) {
        return None;
    }
    let mut sel = Selector::default();
    let mut rest = text;
    let ident_end = |s: &str| s.find(['#', '.', '*']).unwrap_or(s.len());
    if let Some(r) = rest.strip_prefix('*') {
        rest = r;
    } else if !rest.starts_with(['#', '.']) {
        let end = ident_end(rest);
        sel.tag = Some(rest[..end].to_owned());
        rest = &rest[end..];
    }
    while !rest.is_empty() {
        let (kind, r) = rest.split_at(1);
        let end = ident_end(r);
        let name = &r[..end];
        if name.is_empty() {
            return None;
        }
        match kind {
            "#" => sel.id = Some(name.to_owned()),
            "." => sel.classes.push(name.to_owned()),
            _ => return None,
        }
        rest = &r[end..];
    }
    Some(sel)
}
