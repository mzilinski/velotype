//! Queries over the notes of a vault, in a subset of Obsidian Dataview's
//! query language, written in a ```dataview (or ```query) code block:
//!
//! ```text
//! TABLE status, due AS "Due" FROM #project AND "work" WHERE !done SORT due DESC LIMIT 10
//! LIST FROM [[Home]]
//! ```
//!
//! Sources: `#tag`, `"folder"`, `[[note]]` (notes linking to it),
//! `outgoing([[note]])`, combined with `and`, `or`, `-`/`!` and parentheses.
//! Fields: front matter keys and `file.name`, `file.path`, `file.folder`,
//! `file.link`, `file.size`, `file.ctime`, `file.mtime`, `file.cday`,
//! `file.mday`, `file.tags`, `file.inlinks`, `file.outlinks`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use gpui::*;

use super::Editor;
use super::link_index::{LinkIndex, link_index};
use crate::components::PropertyValue;
use crate::i18n::I18nManager;

/// Rows shown at most.
pub(crate) const MAX_ROWS: usize = 500;

/// Languages of the code blocks run as queries.
pub(crate) fn is_query_language(language: &str) -> bool {
    matches!(
        language.trim().to_ascii_lowercase().as_str(),
        "dataview" | "query"
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Value>),
}

impl Value {
    fn from_text(text: &str) -> Self {
        let trimmed = text.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "" | "null" | "~" => return Value::Null,
            "true" => return Value::Bool(true),
            "false" => return Value::Bool(false),
            _ => {}
        }
        match trimmed.parse::<f64>() {
            Ok(number) if number.is_finite() => Value::Number(number),
            _ => Value::Text(trimmed.to_string()),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(value) => *value != 0.0,
            Value::Text(text) => !text.is_empty(),
            Value::List(items) => !items.is_empty(),
        }
    }

    /// The value as shown in a table cell.
    pub(crate) fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => {
                if value.fract() == 0.0 && value.abs() < 1e15 {
                    format!("{}", *value as i64)
                } else {
                    format!("{value}")
                }
            }
            Value::Text(text) => text.clone(),
            Value::List(items) => items
                .iter()
                .map(Value::display)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    fn compare(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            // Missing values sort last.
            (Value::Null, _) => Ordering::Greater,
            (_, Value::Null) => Ordering::Less,
            (Value::Number(left), Value::Number(right)) => {
                left.partial_cmp(right).unwrap_or(Ordering::Equal)
            }
            (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
            _ => {
                let (left, right) = (self.display(), other.display());
                left.to_lowercase()
                    .cmp(&right.to_lowercase())
                    .then(left.cmp(&right))
            }
        }
    }

    fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Null, _) | (_, Value::Null) => false,
            (Value::Text(left), Value::Text(right)) => left == right,
            _ => self.compare(other) == Ordering::Equal,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Number(f64),
    Tag(String),
    Link(String),
    Symbol(&'static str),
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let chars = source.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    let is_word = |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '.' | '-');
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && chars[end] != ch {
                end += 1;
            }
            if end >= chars.len() {
                return Err("unterminated string".into());
            }
            tokens.push(Token::Text(chars[start..end].iter().collect()));
            index = end + 1;
            continue;
        }
        if ch == '[' && chars.get(index + 1) == Some(&'[') {
            let start = index + 2;
            let mut end = start;
            while end + 1 < chars.len() && !(chars[end] == ']' && chars[end + 1] == ']') {
                end += 1;
            }
            if end + 1 >= chars.len() {
                return Err("unterminated [[link]]".into());
            }
            let inner = chars[start..end].iter().collect::<String>();
            let target = inner
                .split('|')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            tokens.push(Token::Link(target));
            index = end + 2;
            continue;
        }
        if ch == '#' {
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && (is_word(chars[end]) || chars[end] == '/') {
                end += 1;
            }
            tokens.push(Token::Tag(chars[start..end].iter().collect()));
            index = end;
            continue;
        }
        if ch.is_ascii_digit() {
            let start = index;
            let mut end = index;
            while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.') {
                end += 1;
            }
            // `2026-01-05` and similar stay text.
            if end < chars.len() && is_word(chars[end]) {
                while end < chars.len() && is_word(chars[end]) {
                    end += 1;
                }
                tokens.push(Token::Word(chars[start..end].iter().collect()));
            } else {
                let text = chars[start..end].iter().collect::<String>();
                tokens.push(Token::Number(
                    text.parse().map_err(|_| format!("bad number {text}"))?,
                ));
            }
            index = end;
            continue;
        }
        if is_word(ch) && ch != '-' && ch != '.' {
            let start = index;
            let mut end = index;
            while end < chars.len() && is_word(chars[end]) {
                end += 1;
            }
            tokens.push(Token::Word(chars[start..end].iter().collect()));
            index = end;
            continue;
        }
        let two = chars[index..chars.len().min(index + 2)]
            .iter()
            .collect::<String>();
        let symbol = ["!=", "<=", ">=", "&&", "||"]
            .into_iter()
            .find(|symbol| two == *symbol)
            .or_else(|| {
                ["=", "<", ">", "!", "-", "(", ")", ","]
                    .into_iter()
                    .find(|symbol| symbol.starts_with(ch))
            })
            .ok_or_else(|| format!("unexpected '{ch}'"))?;
        tokens.push(Token::Symbol(symbol));
        index += symbol.len();
    }
    Ok(tokens)
}

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Tag(String),
    Folder(String),
    LinksTo(String),
    LinkedFrom(String),
    Not(Box<Source>),
    And(Box<Source>, Box<Source>),
    Or(Box<Source>, Box<Source>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    Field(String),
    Literal(Value),
    Call(String, Vec<Expr>),
    Compare(Comparison, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Column {
    expr: Expr,
    pub(crate) header: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    List(Option<Column>),
    Table(Vec<Column>),
}

/// A parsed query.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Query {
    mode: Mode,
    without_id: bool,
    from: Option<Source>,
    filter: Option<Expr>,
    sort: Vec<(Expr, bool)>,
    limit: Option<usize>,
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

const CLAUSES: [&str; 4] = ["from", "where", "sort", "limit"];

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += 1;
        token
    }

    fn keyword(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(found)) if found.eq_ignore_ascii_case(word))
    }

    fn eat_keyword(&mut self, word: &str) -> bool {
        let found = self.keyword(word);
        if found {
            self.position += 1;
        }
        found
    }

    fn eat_symbol(&mut self, symbol: &str) -> bool {
        let found = matches!(self.peek(), Some(Token::Symbol(found)) if *found == symbol);
        if found {
            self.position += 1;
        }
        found
    }

    fn at_clause(&self) -> bool {
        CLAUSES.iter().any(|clause| self.keyword(clause))
    }

    fn query(&mut self) -> Result<Query, String> {
        if self.eat_keyword("table") {
            let without_id = self.without_id();
            let mut columns = Vec::new();
            while self.peek().is_some() && !self.at_clause() {
                columns.push(self.column()?);
                if !self.eat_symbol(",") {
                    break;
                }
            }
            return self.clauses(Mode::Table(columns), without_id);
        }
        if self.eat_keyword("list") {
            let without_id = self.without_id();
            let column = if self.peek().is_some() && !self.at_clause() {
                Some(self.column()?)
            } else {
                None
            };
            return self.clauses(Mode::List(column), without_id);
        }
        Err("a query starts with TABLE or LIST".into())
    }

    fn without_id(&mut self) -> bool {
        if self.keyword("without")
            && matches!(self.tokens.get(self.position + 1), Some(Token::Word(word)) if word.eq_ignore_ascii_case("id"))
        {
            self.position += 2;
            return true;
        }
        false
    }

    fn column(&mut self) -> Result<Column, String> {
        let start = self.position;
        let expr = self.expr()?;
        let header = if self.eat_keyword("as") {
            match self.next() {
                Some(Token::Text(text) | Token::Word(text)) => text,
                _ => return Err("AS needs a name".into()),
            }
        } else {
            describe(&self.tokens[start..self.position])
        };
        Ok(Column { expr, header })
    }

    fn clauses(&mut self, mode: Mode, without_id: bool) -> Result<Query, String> {
        let mut query = Query {
            mode,
            without_id,
            from: None,
            filter: None,
            sort: Vec::new(),
            limit: None,
        };
        while let Some(token) = self.next() {
            let Token::Word(word) = token else {
                return Err(format!("unexpected {}", describe(&[token])));
            };
            match word.to_ascii_lowercase().as_str() {
                "from" => query.from = Some(self.source()?),
                "where" => {
                    let filter = self.expr()?;
                    query.filter = Some(match query.filter.take() {
                        Some(previous) => Expr::And(Box::new(previous), Box::new(filter)),
                        None => filter,
                    });
                }
                "sort" => loop {
                    let expr = self.expr()?;
                    let descending = if self.eat_keyword("desc") || self.eat_keyword("descending") {
                        true
                    } else {
                        if !self.eat_keyword("asc") {
                            self.eat_keyword("ascending");
                        }
                        false
                    };
                    query.sort.push((expr, descending));
                    if !self.eat_symbol(",") {
                        break;
                    }
                },
                "limit" => match self.next() {
                    Some(Token::Number(number)) if number >= 0.0 => {
                        query.limit = Some(number as usize);
                    }
                    _ => return Err("LIMIT needs a number".into()),
                },
                other => return Err(format!("unknown clause {other}")),
            }
        }
        Ok(query)
    }

    fn source(&mut self) -> Result<Source, String> {
        let mut left = self.source_and()?;
        while self.eat_keyword("or") || self.eat_symbol("||") {
            left = Source::Or(Box::new(left), Box::new(self.source_and()?));
        }
        Ok(left)
    }

    fn source_and(&mut self) -> Result<Source, String> {
        let mut left = self.source_term()?;
        while self.eat_keyword("and") || self.eat_symbol("&&") {
            left = Source::And(Box::new(left), Box::new(self.source_term()?));
        }
        Ok(left)
    }

    fn source_term(&mut self) -> Result<Source, String> {
        if self.eat_symbol("-") || self.eat_symbol("!") {
            return Ok(Source::Not(Box::new(self.source_term()?)));
        }
        if self.eat_symbol("(") {
            let source = self.source()?;
            if !self.eat_symbol(")") {
                return Err("missing )".into());
            }
            return Ok(source);
        }
        match self.next() {
            Some(Token::Tag(tag)) => Ok(Source::Tag(tag)),
            Some(Token::Text(folder)) => Ok(Source::Folder(folder)),
            Some(Token::Link(note)) => Ok(Source::LinksTo(note)),
            Some(Token::Word(word)) if word.eq_ignore_ascii_case("outgoing") => {
                let ok = self.eat_symbol("(");
                let Some(Token::Link(note)) = self.next() else {
                    return Err("outgoing() needs a [[link]]".into());
                };
                if !ok || !self.eat_symbol(")") {
                    return Err("outgoing() needs a [[link]]".into());
                }
                Ok(Source::LinkedFrom(note))
            }
            Some(token) => Err(format!("unexpected {} in FROM", describe(&[token]))),
            None => Err("FROM needs a source".into()),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut left = self.and_expr()?;
        while self.eat_keyword("or") || self.eat_symbol("||") {
            left = Expr::Or(Box::new(left), Box::new(self.and_expr()?));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.comparison()?;
        while self.eat_keyword("and") || self.eat_symbol("&&") {
            left = Expr::And(Box::new(left), Box::new(self.comparison()?));
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let left = self.unary()?;
        let op = match self.peek() {
            Some(Token::Symbol("=")) => Comparison::Equal,
            Some(Token::Symbol("!=")) => Comparison::NotEqual,
            Some(Token::Symbol("<")) => Comparison::Less,
            Some(Token::Symbol("<=")) => Comparison::LessOrEqual,
            Some(Token::Symbol(">")) => Comparison::Greater,
            Some(Token::Symbol(">=")) => Comparison::GreaterOrEqual,
            _ => return Ok(left),
        };
        self.position += 1;
        let right = self.unary()?;
        Ok(Expr::Compare(op, Box::new(left), Box::new(right)))
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_symbol("!") || self.eat_keyword("not") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat_symbol("(") {
            let expr = self.expr()?;
            if !self.eat_symbol(")") {
                return Err("missing )".into());
            }
            return Ok(expr);
        }
        if self.eat_symbol("-") {
            return match self.next() {
                Some(Token::Number(number)) => Ok(Expr::Literal(Value::Number(-number))),
                _ => Err("expected a number after -".into()),
            };
        }
        match self.next() {
            Some(Token::Number(number)) => Ok(Expr::Literal(Value::Number(number))),
            Some(Token::Text(text)) => Ok(Expr::Literal(Value::Text(text))),
            Some(Token::Tag(tag)) => Ok(Expr::Literal(Value::Text(format!("#{tag}")))),
            Some(Token::Link(note)) => Ok(Expr::Literal(Value::Text(note))),
            Some(Token::Word(word)) => {
                if self.eat_symbol("(") {
                    let mut args = Vec::new();
                    if !self.eat_symbol(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat_symbol(")") {
                                break;
                            }
                            if !self.eat_symbol(",") {
                                return Err(format!("missing ) after {word}("));
                            }
                        }
                    }
                    return Ok(Expr::Call(word.to_ascii_lowercase(), args));
                }
                match word.to_ascii_lowercase().as_str() {
                    "true" => Ok(Expr::Literal(Value::Bool(true))),
                    "false" => Ok(Expr::Literal(Value::Bool(false))),
                    "null" => Ok(Expr::Literal(Value::Null)),
                    _ if word.chars().next().is_some_and(|ch| ch.is_ascii_digit()) => {
                        Ok(Expr::Literal(Value::Text(word)))
                    }
                    _ => Ok(Expr::Field(word)),
                }
            }
            Some(token) => Err(format!("unexpected {}", describe(&[token]))),
            None => Err("expression expected".into()),
        }
    }
}

/// The tokens as written, for column headers and errors.
fn describe(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        let text = match token {
            Token::Word(word) => word.clone(),
            Token::Text(text) => format!("\"{text}\""),
            Token::Number(number) => Value::Number(*number).display(),
            Token::Tag(tag) => format!("#{tag}"),
            Token::Link(note) => format!("[[{note}]]"),
            Token::Symbol(symbol) => symbol.to_string(),
        };
        let joins =
            matches!(token, Token::Symbol("(" | ")" | ",")) || out.ends_with('(') || out.is_empty();
        if !joins {
            out.push(' ');
        }
        out.push_str(&text);
    }
    out
}

/// Parses a query; the error names what is wrong.
pub(crate) fn parse_query(source: &str) -> Result<Query, String> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Err("empty query".into());
    }
    Parser {
        tokens,
        position: 0,
    }
    .query()
}

/// What a query block shows when it is not being edited.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum QueryView {
    /// The vault is being read.
    Loading,
    Error(String),
    Ready(Arc<QueryResult>),
}

/// One note of the result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueryRow {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) cells: Vec<String>,
}

/// What a query block shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueryResult {
    pub(crate) table: bool,
    /// Whether the first column is the note itself.
    pub(crate) show_id: bool,
    pub(crate) headers: Vec<String>,
    pub(crate) rows: Vec<QueryRow>,
    /// Matches beyond [`MAX_ROWS`] that were left out.
    pub(crate) truncated: usize,
}

struct Document<'a> {
    index: &'a LinkIndex,
    path: &'a Path,
    metadata: Option<std::fs::Metadata>,
}

impl Document<'_> {
    fn field(&self, name: &str) -> Value {
        let lower = name.to_lowercase();
        if let Some(file_field) = lower.strip_prefix("file.") {
            return self.file_field(file_field);
        }
        let key = lower.strip_prefix("this.").unwrap_or(&lower);
        self.index
            .properties(self.path)
            .iter()
            .find(|property| property.key.to_lowercase() == key)
            .map(|property| match &property.value {
                PropertyValue::Empty => Value::Null,
                PropertyValue::Text(text) => Value::from_text(text),
                PropertyValue::List(items) => {
                    Value::List(items.iter().map(|item| Value::from_text(item)).collect())
                }
                PropertyValue::Nested(text) => Value::Text(text.clone()),
            })
            .unwrap_or(Value::Null)
    }

    fn file_field(&self, name: &str) -> Value {
        let targets = self.index.targets();
        let relative = self
            .path
            .strip_prefix(targets.root())
            .unwrap_or(self.path)
            .to_string_lossy()
            .replace('\\', "/");
        let time = |modified: bool, with_time: bool| {
            let metadata = self.metadata.as_ref()?;
            let time = if modified {
                metadata.modified().ok()?
            } else {
                metadata.created().ok()?
            };
            let time = DateTime::<Local>::from(time);
            Some(Value::Text(if with_time {
                time.format("%Y-%m-%d %H:%M").to_string()
            } else {
                time.format("%Y-%m-%d").to_string()
            }))
        };
        let names = |paths: &[PathBuf]| {
            Value::List(
                paths
                    .iter()
                    .map(|path| Value::Text(targets.link_name(path)))
                    .collect(),
            )
        };
        match name {
            "name" => Value::Text(
                self.path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            "link" => Value::Text(targets.link_name(self.path)),
            "path" => Value::Text(relative),
            "folder" => Value::Text(
                relative
                    .rsplit_once('/')
                    .map(|(folder, _)| folder.to_string())
                    .unwrap_or_default(),
            ),
            "ext" => Value::Text(
                self.path
                    .extension()
                    .map(|ext| ext.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            "size" => self
                .metadata
                .as_ref()
                .map(|metadata| Value::Number(metadata.len() as f64))
                .unwrap_or(Value::Null),
            "ctime" => time(false, true).unwrap_or(Value::Null),
            "cday" => time(false, false).unwrap_or(Value::Null),
            "mtime" => time(true, true).unwrap_or(Value::Null),
            "mday" => time(true, false).unwrap_or(Value::Null),
            "tags" | "etags" => Value::List(
                self.index
                    .tag_names_of(self.path)
                    .into_iter()
                    .map(|tag| Value::Text(format!("#{tag}")))
                    .collect(),
            ),
            "inlinks" => {
                let mut sources = self
                    .index
                    .backlinks(self.path)
                    .iter()
                    .map(|link| link.source.clone())
                    .collect::<Vec<_>>();
                sources.dedup();
                names(&sources)
            }
            "outlinks" => names(self.index.outgoing(self.path)),
            _ => Value::Null,
        }
    }

    fn eval(&self, expr: &Expr) -> Value {
        match expr {
            Expr::Field(name) => self.field(name),
            Expr::Literal(value) => value.clone(),
            Expr::Not(inner) => Value::Bool(!self.eval(inner).truthy()),
            Expr::And(left, right) => {
                Value::Bool(self.eval(left).truthy() && self.eval(right).truthy())
            }
            Expr::Or(left, right) => {
                Value::Bool(self.eval(left).truthy() || self.eval(right).truthy())
            }
            Expr::Compare(op, left, right) => {
                let (left, right) = (self.eval(left), self.eval(right));
                Value::Bool(match op {
                    Comparison::Equal => left.equals(&right),
                    Comparison::NotEqual => !left.equals(&right),
                    _ if left == Value::Null || right == Value::Null => false,
                    Comparison::Less => left.compare(&right) == Ordering::Less,
                    Comparison::LessOrEqual => left.compare(&right) != Ordering::Greater,
                    Comparison::Greater => left.compare(&right) == Ordering::Greater,
                    Comparison::GreaterOrEqual => left.compare(&right) != Ordering::Less,
                })
            }
            // `date(today)`: the argument is a word, not a field.
            Expr::Call(name, args)
                if name == "date" && matches!(args.as_slice(), [Expr::Field(_)]) =>
            {
                let [Expr::Field(word)] = args.as_slice() else {
                    unreachable!()
                };
                call(name, &[Value::Text(word.clone())])
            }
            Expr::Call(name, args) => {
                let args = args.iter().map(|arg| self.eval(arg)).collect::<Vec<_>>();
                call(name, &args)
            }
        }
    }
}

fn call(name: &str, args: &[Value]) -> Value {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Value::Null);
    let text = |index: usize| arg(index).display();
    match name {
        "contains" | "icontains" => {
            let fold = |value: String| {
                if name == "icontains" {
                    value.to_lowercase()
                } else {
                    value
                }
            };
            let needle = fold(text(1));
            Value::Bool(match arg(0) {
                Value::List(items) => items.iter().any(|item| {
                    let item = fold(item.display());
                    // Tags match with or without `#`, and nested below.
                    item == needle
                        || item.trim_start_matches('#') == needle.trim_start_matches('#')
                        || (name == "contains" && item.starts_with(&format!("{needle}/")))
                }),
                Value::Null => false,
                value => fold(value.display()).contains(&needle),
            })
        }
        "startswith" => Value::Bool(text(0).starts_with(&text(1))),
        "endswith" => Value::Bool(text(0).ends_with(&text(1))),
        "lower" => Value::Text(text(0).to_lowercase()),
        "upper" => Value::Text(text(0).to_uppercase()),
        "length" => match arg(0) {
            Value::List(items) => Value::Number(items.len() as f64),
            Value::Null => Value::Number(0.0),
            value => Value::Number(value.display().chars().count() as f64),
        },
        "default" => match arg(0) {
            Value::Null => arg(1),
            value => value,
        },
        "date" => match text(0).to_lowercase().as_str() {
            "today" => Value::Text(Local::now().format("%Y-%m-%d").to_string()),
            "now" => Value::Text(Local::now().format("%Y-%m-%d %H:%M").to_string()),
            "tomorrow" => Value::Text(
                (Local::now() + chrono::Duration::days(1))
                    .format("%Y-%m-%d")
                    .to_string(),
            ),
            "yesterday" => Value::Text(
                (Local::now() - chrono::Duration::days(1))
                    .format("%Y-%m-%d")
                    .to_string(),
            ),
            _ => arg(0),
        },
        _ => Value::Null,
    }
}

impl Source {
    fn matches(&self, index: &LinkIndex, path: &Path) -> bool {
        match self {
            Source::Tag(tag) => {
                let key = super::tags::tag_key(tag);
                index
                    .tag_names_of(path)
                    .iter()
                    .any(|name| super::tags::tag_within(&super::tags::tag_key(name), &key))
            }
            Source::Folder(folder) => {
                let folder = folder.trim_matches('/');
                let relative = path
                    .strip_prefix(index.root())
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let relative = relative.strip_suffix(".md").unwrap_or(&relative);
                folder.is_empty()
                    || relative == folder
                    || relative.starts_with(&format!("{folder}/"))
            }
            Source::LinksTo(note) => resolve_note(index, note)
                .is_some_and(|target| index.outgoing(path).contains(&target)),
            Source::LinkedFrom(note) => resolve_note(index, note)
                .is_some_and(|source| index.outgoing(&source).iter().any(|target| target == path)),
            Source::Not(inner) => !inner.matches(index, path),
            Source::And(left, right) => left.matches(index, path) && right.matches(index, path),
            Source::Or(left, right) => left.matches(index, path) || right.matches(index, path),
        }
    }
}

fn resolve_note(index: &LinkIndex, note: &str) -> Option<PathBuf> {
    let root = index.root().to_path_buf();
    index.targets().resolve_wiki(&root.join("_"), note)
}

/// Runs `query` over the notes of `index`.
pub(crate) fn run_query(query: &Query, index: &LinkIndex) -> QueryResult {
    let mut files = index.targets().files().cloned().collect::<Vec<_>>();
    // Notes without a sort key come by name.
    files.sort_by_cached_key(|path| {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (name, path.clone())
    });
    let documents = files
        .iter()
        .map(|path| Document {
            index,
            path,
            metadata: std::fs::metadata(path).ok(),
        })
        .filter(|document| {
            query
                .from
                .as_ref()
                .is_none_or(|source| source.matches(index, document.path))
        })
        .filter(|document| {
            query
                .filter
                .as_ref()
                .is_none_or(|filter| document.eval(filter).truthy())
        })
        .collect::<Vec<_>>();

    let mut keyed = documents
        .into_iter()
        .map(|document| {
            let keys = query
                .sort
                .iter()
                .map(|(expr, _)| document.eval(expr))
                .collect::<Vec<_>>();
            (keys, document)
        })
        .collect::<Vec<_>>();
    if !query.sort.is_empty() {
        keyed.sort_by(|(left, _), (right, _)| {
            for ((left, right), (_, descending)) in left.iter().zip(right).zip(&query.sort) {
                let order = left.compare(right);
                if order != Ordering::Equal {
                    return if *descending && *left != Value::Null && *right != Value::Null {
                        order.reverse()
                    } else {
                        order
                    };
                }
            }
            Ordering::Equal
        });
    }

    let limit = query.limit.unwrap_or(usize::MAX).min(MAX_ROWS);
    let total = keyed.len().min(query.limit.unwrap_or(usize::MAX));
    let columns = match &query.mode {
        Mode::Table(columns) => columns.clone(),
        Mode::List(column) => column.iter().cloned().collect(),
    };
    let rows = keyed
        .into_iter()
        .take(limit)
        .map(|(_, document)| QueryRow {
            path: document.path.to_path_buf(),
            name: index.targets().link_name(document.path),
            cells: columns
                .iter()
                .map(|column| document.eval(&column.expr).display())
                .collect(),
        })
        .collect::<Vec<_>>();
    QueryResult {
        table: matches!(query.mode, Mode::Table(_)),
        show_id: !query.without_id,
        headers: columns.into_iter().map(|column| column.header).collect(),
        truncated: total.saturating_sub(rows.len()),
        rows,
    }
}

/// How often visible query blocks read the vault again.
const QUERY_REVALIDATE: Duration = Duration::from_secs(60);

/// The run of one query block: its source, the index it ran on and the
/// background evaluation.
pub(crate) struct QueryJob {
    source: String,
    index: Option<Arc<LinkIndex>>,
    _task: Option<Task<()>>,
}

impl Editor {
    /// Runs the ```dataview blocks of the document whose query or index
    /// changed, in the background.
    pub(super) fn sync_query_blocks(&mut self, cx: &mut Context<Self>) {
        let blocks = self
            .document
            .visible_blocks()
            .iter()
            .filter_map(|visible| {
                let source = visible.entity.read(cx).query_source()?.to_string();
                Some((visible.entity.clone(), source))
            })
            .collect::<Vec<_>>();
        self.query_jobs
            .retain(|id, _| blocks.iter().any(|(block, _)| block.entity_id() == *id));
        if blocks.is_empty() {
            return;
        }
        let root = self.link_root();
        let index = root.as_ref().and_then(|root| {
            let revalidate = self
                .query_revalidated_at
                .is_none_or(|at| at.elapsed() > QUERY_REVALIDATE);
            if revalidate {
                self.query_revalidated_at = Some(Instant::now());
            }
            link_index(root, revalidate, cx)
        });
        let strings = cx.global::<I18nManager>().strings().clone();
        for (block, source) in blocks {
            let id = block.entity_id();
            let unchanged = self.query_jobs.get(&id).is_some_and(|job| {
                job.source == source
                    && match (&job.index, &index) {
                        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                        (None, None) => true,
                        _ => false,
                    }
            });
            if unchanged {
                continue;
            }
            let (view, task) = match (parse_query(&source), index.clone()) {
                (Err(err), _) => (Some(QueryView::Error(err)), None),
                (Ok(_), None) if root.is_none() => (
                    Some(QueryView::Error(strings.workspace_no_file_message.clone())),
                    None,
                ),
                // The index is being read; it repaints the window when ready.
                (Ok(_), None) => (Some(QueryView::Loading), None),
                (Ok(query), Some(index)) => {
                    let run = cx
                        .background_executor()
                        .spawn(async move { run_query(&query, &index) });
                    let weak = block.downgrade();
                    let task = cx.spawn(async move |_, cx| {
                        let result = run.await;
                        let _ = weak.update(cx, |block, cx| {
                            if block.set_query_view(Some(QueryView::Ready(Arc::new(result)))) {
                                cx.notify();
                            }
                        });
                    });
                    // Keep showing the last result until the new one is in.
                    let view = block
                        .read(cx)
                        .query_view()
                        .is_none()
                        .then_some(QueryView::Loading);
                    (view, Some(task))
                }
            };
            if let Some(view) = view {
                block.update(cx, |block, cx| {
                    if block.set_query_view(Some(view)) {
                        cx.notify();
                    }
                });
            }
            self.query_jobs.insert(
                id,
                QueryJob {
                    source,
                    index: index.clone(),
                    _task: task,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{parse_query, run_query};
    use crate::editor::link_index::LinkIndex;

    fn vault() -> LinkIndex {
        let sources: Vec<(PathBuf, String)> = vec![
            (
                "/v/projects/Alpha.md".into(),
                "---\nstatus: active\npriority: 2\ntags: [project]\n---\nSee [[Home]]".into(),
            ),
            (
                "/v/projects/Beta.md".into(),
                "---\nstatus: done\npriority: 10\ndone: true\n---\n#project/sub [[Alpha]]".into(),
            ),
            (
                "/v/projects/Gamma.md".into(),
                "---\nstatus: active\npriority: 1\n---\nno tag".into(),
            ),
            ("/v/Home.md".into(), "[[Alpha]] [[Gamma]] #hub".into()),
        ];
        let files = sources.iter().map(|(path, _)| path.clone()).collect();
        LinkIndex::from_sources(Path::new("/v"), files, sources)
    }

    fn names(query: &str) -> Vec<String> {
        let query = parse_query(query).unwrap_or_else(|err| panic!("{query}: {err}"));
        run_query(&query, &vault())
            .rows
            .into_iter()
            .map(|row| row.name)
            .collect()
    }

    #[test]
    fn sources_select_notes() {
        assert_eq!(names("LIST FROM #project"), ["Alpha", "Beta"]);
        assert_eq!(names("LIST FROM \"projects\" AND -#project"), ["Gamma"]);
        assert_eq!(names("LIST FROM [[Alpha]]"), ["Beta", "Home"]);
        assert_eq!(names("LIST FROM outgoing([[Home]])"), ["Alpha", "Gamma"]);
        assert_eq!(names("LIST FROM #hub OR #project/sub"), ["Beta", "Home"]);
    }

    #[test]
    fn where_sort_and_limit() {
        assert_eq!(
            names("LIST WHERE status = \"active\" SORT priority DESC"),
            ["Alpha", "Gamma"]
        );
        assert_eq!(
            names("LIST FROM \"projects\" WHERE !done SORT file.name"),
            ["Alpha", "Gamma"]
        );
        assert_eq!(
            names("LIST WHERE priority >= 2 SORT priority ASC LIMIT 1"),
            ["Alpha"]
        );
        assert_eq!(
            names("LIST WHERE contains(file.tags, \"#project\")"),
            ["Alpha", "Beta"]
        );
        assert_eq!(
            names("LIST WHERE contains(file.outlinks, \"Home\")"),
            ["Alpha"]
        );
        assert_eq!(
            names("LIST WHERE length(file.inlinks) > 1 SORT file.name"),
            ["Alpha"]
        );
    }

    #[test]
    fn table_columns_and_headers() {
        let query = parse_query(
            "TABLE status AS \"State\", priority, file.folder FROM \"projects\" SORT priority",
        )
        .unwrap();
        let result = run_query(&query, &vault());
        assert!(result.table && result.show_id);
        assert_eq!(result.headers, ["State", "priority", "file.folder"]);
        let rows = result
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.cells.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                (
                    "Gamma",
                    vec!["active".into(), "1".into(), "projects".into()]
                ),
                (
                    "Alpha",
                    vec!["active".into(), "2".into(), "projects".into()]
                ),
                ("Beta", vec!["done".into(), "10".into(), "projects".into()]),
            ]
        );
        let query =
            parse_query("TABLE WITHOUT ID file.name, length(file.outlinks) AS Links").unwrap();
        let result = run_query(&query, &vault());
        assert!(!result.show_id);
        assert_eq!(result.headers, ["file.name", "Links"]);
    }

    #[test]
    fn errors_name_the_problem() {
        assert!(parse_query("").is_err());
        assert!(parse_query("SELECT x").is_err());
        assert!(parse_query("LIST FROM").is_err());
        assert!(parse_query("LIST WHERE (a = 1").is_err());
        assert!(parse_query("LIST LIMIT x").is_err());
        assert!(parse_query("LIST WHERE a = \"open").is_err());
    }
}
