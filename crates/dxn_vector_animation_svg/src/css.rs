//! The CSS an SVG's `<style>` sheets and `style` attributes hold: rules of
//! selectors (compounds of types, ids, classes, attributes and structural
//! pseudo-classes, joined by combinators), declarations, and `@keyframes`.

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

/// A sheet's selectors, `(rule, selector)` each, by what an element must
/// have for one to match it (its first id, else its last class, else its
/// tag), so that an element tries only those it may match.
#[derive(Debug, Default)]
pub(crate) struct Index {
    ids: HashMap<String, Vec<(usize, usize)>>,
    classes: HashMap<String, Vec<(usize, usize)>>,
    tags: HashMap<String, Vec<(usize, usize)>>,
    rest: Vec<(usize, usize)>,
}

impl Index {
    /// The selectors `node` may match, in the sheet's order.
    pub(crate) fn candidates(&self, node: roxmltree::Node<'_, '_>) -> Vec<(usize, usize)> {
        let mut out = self.rest.clone();
        let mut add = |list: Option<&Vec<(usize, usize)>>| out.extend(list.into_iter().flatten());
        if let Some(id) = node.attribute("id") {
            add(self.ids.get(id));
        }
        if let Some(classes) = node.attribute("class") {
            for (i, class) in classes.split_ascii_whitespace().enumerate() {
                // A class the element names twice adds its selectors once.
                if !classes.split_ascii_whitespace().take(i).any(|c| c == class) {
                    add(self.classes.get(class));
                }
            }
        }
        add(self.tags.get(node.tag_name().name()));
        out.sort_unstable();
        out
    }
}

impl Stylesheet {
    /// Its selectors indexed.
    pub(crate) fn index(&self) -> Index {
        let mut index = Index::default();
        for (r, rule) in self.rules.iter().enumerate() {
            for (s, sel) in rule.selectors.iter().enumerate() {
                let subject = &sel.subject;
                let list = if let Some(id) = subject.ids.first() {
                    index.ids.entry(id.clone()).or_default()
                } else if let Some(class) = subject.classes.last() {
                    index.classes.entry(class.clone()).or_default()
                } else if let Some(tag) = &subject.tag {
                    index.tags.entry(tag.clone()).or_default()
                } else {
                    &mut index.rest
                };
                list.push((r, s));
            }
        }
        index
    }
}

/// A selector: compounds joined by combinators, matched right to left.
#[derive(Debug, Default)]
pub(crate) struct Selector {
    /// What the element itself matches.
    subject: Compound,
    /// What its ancestors or earlier siblings match, nearest first, each
    /// with how it joins the compound after it.
    context: Vec<(Combinator, Compound)>,
}

/// How two compounds of a selector relate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    /// `a b`: an ancestor.
    Descendant,
    /// `a > b`: the parent.
    Child,
    /// `a + b`: the element just before.
    Adjacent,
    /// `a ~ b`: an element before.
    Sibling,
}

/// A compound selector: `tag` or `*`, `#id`, `.class`, `[attribute]` and
/// pseudo-classes together.
#[derive(Debug, Default, Clone)]
struct Compound {
    tag: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
    attributes: Vec<Attribute>,
    pseudo: Vec<Pseudo>,
}

/// `[name]`, or `[name op value]`.
#[derive(Debug, Clone)]
struct Attribute {
    name: String,
    test: Option<(String, String)>,
}

/// The pseudo-classes it matches.
#[derive(Debug, Clone)]
enum Pseudo {
    /// `:nth-child(an+b)` (`:first-child` is `0n+1`), from the end for
    /// `:nth-last-child` and `:last-child`; among the elements of its own
    /// tag for the `-of-type` ones.
    Nth {
        a: i64,
        b: i64,
        from_end: bool,
        of_type: bool,
    },
    /// `:only-child`, `:only-of-type`.
    Only { of_type: bool },
    /// `:empty`: no element children and no text.
    Empty,
    /// `:root`.
    Root,
    /// `:any-link`, `:link`: a link with an address.
    Link,
    /// `:is()` (and `:where()`, which weighs nothing): any of the
    /// compounds.
    Is { any: Vec<Compound>, weighs: bool },
    /// `:not()`: none of the compounds.
    Not(Vec<Compound>),
    /// What never matches here: the states of interaction (`:hover`, named
    /// so it can be reported) and pseudo-elements, which draw nothing in
    /// SVG.
    Never(Option<String>),
}

impl Selector {
    /// CSS specificity: (ids, classes, tags).
    pub(crate) fn specificity(&self) -> (u32, u32, u32) {
        std::iter::once(&self.subject)
            .chain(self.context.iter().map(|(_, c)| c))
            .map(Compound::specificity)
            .fold((0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2))
    }

    pub(crate) fn matches(&self, node: roxmltree::Node<'_, '_>) -> bool {
        self.subject.matches(node) && matches_context(&self.context, node)
    }
}

/// Whether `node`'s ancestors and earlier siblings match `context`
/// (nearest first), trying every candidate a combinator allows.
fn matches_context(context: &[(Combinator, Compound)], node: roxmltree::Node<'_, '_>) -> bool {
    let Some(((combinator, compound), rest)) = context.split_first() else {
        return true;
    };
    let mut candidates: Box<dyn Iterator<Item = roxmltree::Node<'_, '_>>> = match combinator {
        Combinator::Child => Box::new(node.parent_element().into_iter()),
        Combinator::Descendant => Box::new(node.ancestors().skip(1).filter(|n| n.is_element())),
        Combinator::Adjacent => Box::new(node.prev_sibling_element().into_iter()),
        Combinator::Sibling => Box::new(std::iter::successors(node.prev_sibling_element(), |n| {
            n.prev_sibling_element()
        })),
    };
    candidates.any(|n| compound.matches(n) && matches_context(rest, n))
}

impl Compound {
    fn specificity(&self) -> (u32, u32, u32) {
        let pseudo: (u32, u32, u32) = self
            .pseudo
            .iter()
            .map(|p| match p {
                Pseudo::Not(list)
                | Pseudo::Is {
                    any: list,
                    weighs: true,
                } => list
                    .iter()
                    .map(Compound::specificity)
                    .max()
                    .unwrap_or((0, 0, 0)),
                Pseudo::Is { weighs: false, .. } => (0, 0, 0),
                Pseudo::Never(None) => (0, 0, 1),
                _ => (0, 1, 0),
            })
            .fold((0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
        (
            self.ids.len() as u32 + pseudo.0,
            (self.classes.len() + self.attributes.len()) as u32 + pseudo.1,
            u32::from(self.tag.is_some()) + pseudo.2,
        )
    }

    fn matches(&self, node: roxmltree::Node<'_, '_>) -> bool {
        if !node.is_element() {
            return false;
        }
        if self
            .tag
            .as_deref()
            .is_some_and(|t| t != node.tag_name().name())
        {
            return false;
        }
        if self
            .ids
            .iter()
            .any(|id| node.attribute("id") != Some(id.as_str()))
        {
            return false;
        }
        if !self.classes.is_empty() {
            let classes = node.attribute("class").unwrap_or_default();
            if !self
                .classes
                .iter()
                .all(|c| classes.split_ascii_whitespace().any(|n| n == c))
            {
                return false;
            }
        }
        self.attributes.iter().all(|a| a.matches(node))
            && self.pseudo.iter().all(|p| p.matches(node))
    }
}

impl Attribute {
    fn matches(&self, node: roxmltree::Node<'_, '_>) -> bool {
        let Some(value) = node
            .attributes()
            .find(|a| a.name() == self.name)
            .map(|a| a.value())
        else {
            return false;
        };
        let Some((op, want)) = &self.test else {
            return true;
        };
        let want = want.as_str();
        match op.as_str() {
            "=" => value == want,
            "~=" => value.split_ascii_whitespace().any(|w| w == want),
            "|=" => value == want || value.starts_with(&format!("{want}-")),
            "^=" => !want.is_empty() && value.starts_with(want),
            "$=" => !want.is_empty() && value.ends_with(want),
            "*=" => !want.is_empty() && value.contains(want),
            _ => false,
        }
    }
}

impl Pseudo {
    fn matches(&self, node: roxmltree::Node<'_, '_>) -> bool {
        // The siblings before (or after) `node` and itself, of its tag
        // only when `of_type`.
        let count = |next: bool, of_type: bool| {
            std::iter::successors(Some(node), move |n| {
                if next {
                    n.next_sibling_element()
                } else {
                    n.prev_sibling_element()
                }
            })
            .filter(|n| !of_type || n.tag_name() == node.tag_name())
            .count() as i64
        };
        match self {
            Pseudo::Nth {
                a,
                b,
                from_end,
                of_type,
            } => {
                let index = count(*from_end, *of_type);
                // index = a·n + b for some n ≥ 0.
                match a {
                    0 => index == *b,
                    a => (index - b) % a == 0 && (index - b) / a >= 0,
                }
            }
            Pseudo::Only { of_type } => count(true, *of_type) == 1 && count(false, *of_type) == 1,
            Pseudo::Empty => !node
                .children()
                .any(|c| c.is_element() || c.text().is_some_and(|t| !t.is_empty())),
            Pseudo::Root => node.parent_element().is_none(),
            Pseudo::Link => {
                matches!(node.tag_name().name(), "a" | "area")
                    && node
                        .attributes()
                        .any(|a| a.name() == "href" && !a.value().is_empty())
            }
            Pseudo::Is { any, .. } => any.iter().any(|c| c.matches(node)),
            Pseudo::Not(list) => !list.iter().any(|c| c.matches(node)),
            Pseudo::Never(_) => false,
        }
    }
}

impl Selector {
    /// The state of interaction it needs to match (`hover`), if any.
    pub(crate) fn interaction(&self) -> Option<&str> {
        std::iter::once(&self.subject)
            .chain(self.context.iter().map(|(_, c)| c))
            .flat_map(|c| &c.pseudo)
            .find_map(|p| match p {
                Pseudo::Never(Some(name)) => Some(name.as_str()),
                _ => None,
            })
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
fn strip_comments(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("/*") {
        return std::borrow::Cow::Borrowed(text);
    }
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
    std::borrow::Cow::Owned(out)
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
            if let Some(i) = value
                .as_bytes()
                .windows(10)
                .rposition(|w| w.eq_ignore_ascii_case(b"!important"))
            {
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
            for sel in split_top(prelude, ',') {
                match parse_selector(sel.trim()) {
                    Some(s) => {
                        if let Some(state) = s.interaction() {
                            warn(format!(
                                "CSS `:{state}` rules apply on interaction, which the scene does not have: it is drawn at rest"
                            ));
                        }
                        selectors.push(s)
                    }
                    None => warn(format!(
                        "CSS selector `{}` is not supported: its rule is left out",
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

/// A selector, or `None` for what this reader does not match
/// (pseudo-elements, other pseudo-classes, namespaces).
fn parse_selector(text: &str) -> Option<Selector> {
    let mut compounds: Vec<(Option<Combinator>, Compound)> = Vec::new();
    let mut pending: Option<Combinator> = None;
    let mut rest = text.trim();
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        let spaced = trimmed.len() < rest.len();
        rest = trimmed;
        if let Some(c) = rest.chars().next()
            && let Some(combinator) = match c {
                '>' => Some(Combinator::Child),
                '+' => Some(Combinator::Adjacent),
                '~' => Some(Combinator::Sibling),
                _ => None,
            }
        {
            if compounds.is_empty() || pending.is_some_and(|p| p != Combinator::Descendant) {
                return None;
            }
            pending = Some(combinator);
            rest = &rest[1..];
            continue;
        }
        if spaced && !compounds.is_empty() && pending.is_none() {
            pending = Some(Combinator::Descendant);
        }
        let (compound, after) = parse_compound(rest)?;
        if !compounds.is_empty() && pending.is_none() {
            return None;
        }
        compounds.push((pending.take(), compound));
        rest = after;
    }
    if pending.is_some() {
        return None;
    }
    // Each compound came with the combinator joining it to the one before.
    let (join, subject) = compounds.pop()?;
    let mut context = Vec::with_capacity(compounds.len());
    let mut next_join = join;
    while let Some((join, compound)) = compounds.pop() {
        context.push((next_join?, compound));
        next_join = join;
    }
    Some(Selector { subject, context })
}

/// A compound at the start of `text`, and what follows it.
fn parse_compound(text: &str) -> Option<(Compound, &str)> {
    let mut c = Compound::default();
    let mut rest = text;
    let ident_len = |s: &str| {
        s.find(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '\\'))
            .unwrap_or(s.len())
    };
    if let Some(r) = rest.strip_prefix('*') {
        rest = r;
    } else {
        let n = ident_len(rest);
        if n > 0 {
            c.tag = Some(rest[..n].to_owned());
            rest = &rest[n..];
        }
    }
    let mut any = c.tag.is_some() || rest.len() < text.len();
    loop {
        match rest.chars().next() {
            Some('#') => {
                let n = ident_len(&rest[1..]);
                if n == 0 {
                    return None;
                }
                c.ids.push(rest[1..1 + n].to_owned());
                rest = &rest[1 + n..];
            }
            Some('.') => {
                let n = ident_len(&rest[1..]);
                if n == 0 {
                    return None;
                }
                c.classes.push(rest[1..1 + n].to_owned());
                rest = &rest[1 + n..];
            }
            Some('[') => {
                let close = rest.find(']')?;
                c.attributes.push(parse_attribute(&rest[1..close])?);
                rest = &rest[close + 1..];
            }
            Some(':') => {
                if let Some(after) = rest.strip_prefix("::") {
                    // A pseudo-element: SVG's elements have none to draw.
                    let n = ident_len(after);
                    rest = &after[n..];
                    if rest.starts_with('(') {
                        rest = &rest[matching_paren(rest)? + 1..];
                    }
                    c.pseudo.push(Pseudo::Never(None));
                    any = true;
                    continue;
                }
                let n = ident_len(&rest[1..]);
                let name = rest[1..1 + n].to_ascii_lowercase();
                rest = &rest[1 + n..];
                let mut args = None;
                if rest.starts_with('(') {
                    let close = matching_paren(rest)?;
                    args = Some(rest[1..close].trim());
                    rest = &rest[close + 1..];
                }
                let nth = |a: i64, b: i64, from_end: bool, of_type: bool| Pseudo::Nth {
                    a,
                    b,
                    from_end,
                    of_type,
                };
                c.pseudo.push(match (name.as_str(), args) {
                    ("first-child", None) => nth(0, 1, false, false),
                    ("last-child", None) => nth(0, 1, true, false),
                    ("first-of-type", None) => nth(0, 1, false, true),
                    ("last-of-type", None) => nth(0, 1, true, true),
                    ("only-child", None) => Pseudo::Only { of_type: false },
                    ("only-of-type", None) => Pseudo::Only { of_type: true },
                    ("nth-child", Some(a)) => {
                        let (a, b) = parse_nth(a)?;
                        nth(a, b, false, false)
                    }
                    ("nth-last-child", Some(a)) => {
                        let (a, b) = parse_nth(a)?;
                        nth(a, b, true, false)
                    }
                    ("nth-of-type", Some(a)) => {
                        let (a, b) = parse_nth(a)?;
                        nth(a, b, false, true)
                    }
                    ("nth-last-of-type", Some(a)) => {
                        let (a, b) = parse_nth(a)?;
                        nth(a, b, true, true)
                    }
                    ("empty", None) => Pseudo::Empty,
                    ("root", None) => Pseudo::Root,
                    ("any-link" | "link", None) => Pseudo::Link,
                    ("not", Some(inner)) => Pseudo::Not(compound_list(inner)?),
                    ("is" | "matches" | "-webkit-any", Some(inner)) => Pseudo::Is {
                        any: compound_list(inner)?,
                        weighs: true,
                    },
                    ("where", Some(inner)) => Pseudo::Is {
                        any: compound_list(inner)?,
                        weighs: false,
                    },
                    // Pseudo-elements written with one colon.
                    ("before" | "after" | "first-line" | "first-letter", None) => {
                        Pseudo::Never(None)
                    }
                    // States of interaction, and of what SVG does not have.
                    (
                        "hover" | "active" | "focus" | "focus-visible" | "focus-within" | "visited"
                        | "target" | "checked" | "indeterminate" | "disabled" | "enabled"
                        | "placeholder-shown" | "default" | "valid" | "invalid" | "required"
                        | "optional" | "read-only" | "read-write",
                        None,
                    ) => Pseudo::Never(Some(name.clone())),
                    _ => return None,
                });
            }
            _ => break,
        }
        any = true;
    }
    any.then_some((c, rest))
}

/// A comma-separated list of compound selectors (as `:is()` and `:not()`
/// take them).
fn compound_list(text: &str) -> Option<Vec<Compound>> {
    split_top(text, ',')
        .into_iter()
        .map(|part| {
            let (compound, after) = parse_compound(part.trim())?;
            after.trim().is_empty().then_some(compound)
        })
        .collect()
}

/// The index of the parenthesis closing the one `text` starts with.
fn matching_paren(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `name`, or `name op value` (the value quoted or not).
fn parse_attribute(text: &str) -> Option<Attribute> {
    let text = text.trim();
    let ops = ["~=", "|=", "^=", "$=", "*=", "="];
    for op in ops {
        if let Some((name, value)) = text.split_once(op) {
            let value = value.trim();
            let value = value
                .strip_suffix(" i")
                .or_else(|| value.strip_suffix(" s"))
                .unwrap_or(value)
                .trim();
            return Some(Attribute {
                name: name.trim().to_owned(),
                test: Some((op.to_owned(), unquote(value).to_owned())),
            });
        }
    }
    (!text.is_empty()).then(|| Attribute {
        name: text.to_owned(),
        test: None,
    })
}

/// `an+b` of `:nth-child()`: `odd`, `even`, `3`, `2n`, `-n+3`, `2n+1`.
fn parse_nth(text: &str) -> Option<(i64, i64)> {
    let t: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match t.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    match t.split_once('n') {
        None => Some((0, t.parse().ok()?)),
        Some((a, b)) => {
            let a = match a {
                "" | "+" => 1,
                "-" => -1,
                a => a.parse().ok()?,
            };
            let b = if b.is_empty() { 0 } else { b.parse().ok()? };
            Some((a, b))
        }
    }
}
