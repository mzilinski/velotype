//! Typora CSS themes. GPUI does not render CSS, so a theme's main colors and
//! sizes are read from its rules (with `var(--x)` resolved) and applied to a
//! built-in base theme; exports embed the original CSS so they look like in
//! Typora.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Hsla, Rgba};

use super::Theme;

/// The rules of a style sheet: selector list and declarations, in order.
#[derive(Debug, Default)]
pub(crate) struct CssRules {
    rules: Vec<(Vec<String>, Vec<(String, String)>)>,
    variables: HashMap<String, String>,
}

/// Removes `/* ... */` comments.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

impl CssRules {
    /// Parses plain rules; at-rules (`@media`, `@font-face`, `@import`, ...)
    /// are skipped, so dark-mode or print variants do not override the base.
    pub(crate) fn parse(css: &str) -> Self {
        let css = strip_comments(css);
        let mut rules = Vec::new();
        let mut rest = css.as_str();
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            if rest.starts_with('@') {
                // Statement at-rule (`@import ...;`) or block at-rule.
                let semicolon = rest.find(';');
                let brace = rest.find('{');
                match (semicolon, brace) {
                    (Some(semicolon), Some(brace)) if semicolon < brace => {
                        rest = &rest[semicolon + 1..];
                    }
                    (_, Some(brace)) => {
                        let end = matching_brace(rest, brace).unwrap_or(rest.len() - 1);
                        rest = &rest[end + 1..];
                    }
                    (Some(semicolon), None) => rest = &rest[semicolon + 1..],
                    (None, None) => break,
                }
                continue;
            }
            let Some(open) = rest.find('{') else {
                break;
            };
            let Some(close) = matching_brace(rest, open) else {
                break;
            };
            let selectors = rest[..open]
                .split(',')
                .map(|selector| selector.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|selector| !selector.is_empty())
                .collect::<Vec<_>>();
            let declarations = rest[open + 1..close]
                .split(';')
                .filter_map(|declaration| {
                    let (name, value) = declaration.split_once(':')?;
                    let value = value.trim().trim_end_matches("!important").trim();
                    Some((name.trim().to_ascii_lowercase(), value.to_string()))
                })
                .filter(|(name, value)| !name.is_empty() && !value.is_empty())
                .collect::<Vec<_>>();
            rules.push((selectors, declarations));
            rest = &rest[close + 1..];
        }
        let mut variables = HashMap::new();
        for (selectors, declarations) in &rules {
            if selectors
                .iter()
                .any(|selector| matches!(selector.as_str(), ":root" | "html" | "body"))
            {
                for (name, value) in declarations {
                    if name.starts_with("--") {
                        variables.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        Self { rules, variables }
    }

    /// The last value of `property` in a rule for one of `selectors`
    /// (earlier selectors in the list win over later ones).
    pub(crate) fn value(&self, selectors: &[&str], property: &str) -> Option<String> {
        for wanted in selectors {
            let found = self
                .rules
                .iter()
                .filter(|(list, _)| list.iter().any(|selector| selector == wanted))
                .flat_map(|(_, declarations)| declarations.iter())
                .filter(|(name, _)| name == property)
                .next_back();
            if let Some((_, value)) = found {
                return Some(self.resolve(value, 0));
            }
        }
        None
    }

    /// `value` with `var(--name, fallback)` replaced.
    fn resolve(&self, value: &str, depth: usize) -> String {
        if depth > 8 {
            return value.to_string();
        }
        let Some(start) = value.find("var(") else {
            return value.to_string();
        };
        let Some(end) = matching_paren(value, start + 3) else {
            return value.to_string();
        };
        let inner = &value[start + 4..end];
        let (name, fallback) = match inner.split_once(',') {
            Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
            None => (inner.trim(), None),
        };
        let replacement = self
            .variables
            .get(name)
            .map(String::as_str)
            .or(fallback)
            .unwrap_or("");
        let replaced = format!("{}{}{}", &value[..start], replacement, &value[end + 1..]);
        self.resolve(&replaced, depth + 1)
    }

    /// A color from `property` (or, for `background`/`border...` shorthands,
    /// the first part that is a color).
    pub(crate) fn color(&self, selectors: &[&str], properties: &[&str]) -> Option<Hsla> {
        properties.iter().find_map(|property| {
            let value = self.value(selectors, property)?;
            parse_color(&value).or_else(|| {
                split_css_words(&value)
                    .into_iter()
                    .find_map(|word| parse_color(&word))
            })
        })
    }

    /// A font size in pixels (`em`/`rem`/`%` relative to `base`).
    pub(crate) fn font_size(&self, selectors: &[&str], base: f32) -> Option<f32> {
        let value = self.value(selectors, "font-size")?;
        parse_length(&value, base)
    }
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + index);
                }
            }
            _ => {}
        }
    }
    None
}

fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Space-separated parts of a value, keeping `rgb(...)` together.
fn split_css_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in value.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ch if ch.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn parse_color(value: &str) -> Option<Hsla> {
    use crate::components::markdown::html::{HtmlCssColor, parse_css_color};
    match parse_css_color(value)? {
        HtmlCssColor::Rgba(rgba) if rgba.alpha > 0.0 => Some(
            Rgba {
                r: f32::from(rgba.red) / 255.0,
                g: f32::from(rgba.green) / 255.0,
                b: f32::from(rgba.blue) / 255.0,
                a: rgba.alpha,
            }
            .into(),
        ),
        _ => None,
    }
}

fn parse_length(value: &str, base: f32) -> Option<f32> {
    let value = value.trim();
    let number = |suffix: &str| value.strip_suffix(suffix)?.trim().parse::<f32>().ok();
    let size = if let Some(px) = number("px") {
        px
    } else if let Some(rem) = number("rem") {
        rem * 16.0
    } else if let Some(em) = number("em") {
        em * base
    } else if let Some(percent) = number("%") {
        percent * base / 100.0
    } else if let Some(pt) = number("pt") {
        pt * 4.0 / 3.0
    } else {
        return None;
    };
    (4.0..=96.0).contains(&size).then_some(size)
}

/// A Velotype theme from a Typora CSS theme: `base` (picked by the caller
/// from the background brightness) with the style sheet's colors and sizes.
pub(crate) fn theme_from_css(name: &str, css: &str, light_base: Theme, dark_base: Theme) -> Theme {
    let rules = CssRules::parse(css);
    let body = ["#write", "body", "html", ":root", "content"];
    let background = rules.color(&body, &["background-color", "background"]);
    let dark = background.is_some_and(|background| background.l < 0.5);
    let mut theme = if dark { dark_base } else { light_base };
    theme.name = name.to_string();
    theme.css = Some(Arc::from(css));
    let c = &mut theme.colors;
    let set = |slot: &mut Hsla, value: Option<Hsla>| {
        if let Some(value) = value {
            *slot = value;
        }
    };
    set(&mut c.editor_background, background);
    let text = rules.color(&body, &["color"]);
    set(&mut c.text_default, text);
    set(&mut c.cursor, text);
    set(
        &mut c.text_link,
        rules.color(&["#write a", "a"], &["color"]),
    );
    let headings = [
        &mut c.text_h1,
        &mut c.text_h2,
        &mut c.text_h3,
        &mut c.text_h4,
        &mut c.text_h5,
        &mut c.text_h6,
    ];
    for (level, slot) in headings.into_iter().enumerate() {
        let selector = format!("h{}", level + 1);
        let write_selector = format!("#write h{}", level + 1);
        set(
            slot,
            rules
                .color(&[write_selector.as_str(), selector.as_str()], &["color"])
                .or(text),
        );
    }
    set(
        &mut c.border_h1,
        rules.color(
            &["#write h1", "h1"],
            &["border-bottom-color", "border-bottom"],
        ),
    );
    set(
        &mut c.border_h2,
        rules.color(
            &["#write h2", "h2"],
            &["border-bottom-color", "border-bottom"],
        ),
    );
    set(
        &mut c.text_quote,
        rules.color(&["#write blockquote", "blockquote"], &["color"]),
    );
    set(
        &mut c.border_quote,
        rules.color(
            &["#write blockquote", "blockquote"],
            &["border-left-color", "border-left", "border-color"],
        ),
    );
    set(
        &mut c.code_bg,
        rules.color(
            &[
                ".md-fences",
                "#write pre",
                "pre",
                "#write code",
                "code",
                "tt",
            ],
            &["background-color", "background"],
        ),
    );
    set(
        &mut c.code_text,
        rules.color(&[".md-fences", "#write code", "code", "pre"], &["color"]),
    );
    set(
        &mut c.table_border,
        rules.color(
            &["table tr", "table td", "td", "th", "table"],
            &["border-color", "border", "border-top"],
        ),
    );
    set(
        &mut c.table_header_bg,
        rules.color(
            &["table thead tr", "thead tr", "th", "table tr th", "thead"],
            &["background-color", "background"],
        ),
    );
    set(
        &mut c.selection,
        rules.color(
            &["::selection", "#write ::selection"],
            &["background", "background-color"],
        ),
    );
    set(
        &mut c.separator_color,
        rules.color(
            &["hr"],
            &[
                "background-color",
                "border-top",
                "border-color",
                "background",
            ],
        ),
    );
    set(
        &mut c.highlight_bg,
        rules.color(&["mark"], &["background-color", "background"]),
    );

    let t = &mut theme.typography;
    let base = rules
        .font_size(&["#write", "body", "html", ":root"], 16.0)
        .unwrap_or(t.text_size);
    t.text_size = base;
    let heading_sizes = [
        &mut t.h1_size,
        &mut t.h2_size,
        &mut t.h3_size,
        &mut t.h4_size,
        &mut t.h5_size,
        &mut t.h6_size,
    ];
    for (level, size) in heading_sizes.into_iter().enumerate() {
        let selector = format!("h{}", level + 1);
        let write_selector = format!("#write h{}", level + 1);
        if let Some(value) = rules.font_size(&[write_selector.as_str(), selector.as_str()], base) {
            *size = value;
        }
    }
    if let Some(code) = rules.font_size(&[".md-fences", "code", "pre"], base) {
        t.code_size = code;
    }
    theme
}

#[cfg(test)]
mod tests {
    use super::{CssRules, theme_from_css};
    use crate::theme::Theme;

    const SAMPLE: &str = r#"
        @import "my-theme/fonts.css";
        /* comment { with braces } */
        :root { --bg-color: #fdf6e3; --text-color: rgb(88, 110, 117); }
        html { font-size: 17px; }
        body, #write { background: var(--bg-color); color: var(--text-color); }
        #write a { color: #268bd2; }
        h1 { font-size: 2em; color: #cb4b16; border-bottom: 1px solid #93a1a1; }
        h2, h3 { color: hsl(205, 69%, 49%); }
        blockquote { border-left: 4px solid #2aa198; color: #657b83; }
        .md-fences, code { background-color: #eee8d5 !important; }
        @media (prefers-color-scheme: dark) { body { background: #002b36; } }
    "#;

    #[test]
    fn rules_resolve_variables_and_skip_at_rules() {
        let rules = CssRules::parse(SAMPLE);
        assert_eq!(
            rules.value(&["body"], "background").as_deref(),
            Some("#fdf6e3")
        );
        assert_eq!(
            rules.value(&["#write"], "color").as_deref(),
            Some("rgb(88, 110, 117)")
        );
        assert_eq!(rules.font_size(&["html"], 16.0), Some(17.0));
        assert_eq!(rules.font_size(&["h1"], 17.0), Some(34.0));
        assert!(rules.value(&["missing"], "color").is_none());
    }

    #[test]
    fn css_theme_maps_typora_rules_onto_a_base_theme() {
        let theme = theme_from_css(
            "Solarized",
            SAMPLE,
            Theme::builtin("github").unwrap(),
            Theme::builtin("night").unwrap(),
        );
        let rgb = |color: gpui::Hsla| {
            let rgba = gpui::Rgba::from(color);
            (
                (rgba.r * 255.0).round() as u8,
                (rgba.g * 255.0).round() as u8,
                (rgba.b * 255.0).round() as u8,
            )
        };
        assert_eq!(theme.name, "Solarized");
        assert_eq!(rgb(theme.colors.editor_background), (0xfd, 0xf6, 0xe3));
        assert_eq!(rgb(theme.colors.text_default), (88, 110, 117));
        assert_eq!(rgb(theme.colors.text_link), (0x26, 0x8b, 0xd2));
        assert_eq!(rgb(theme.colors.text_h1), (0xcb, 0x4b, 0x16));
        assert_eq!(rgb(theme.colors.border_quote), (0x2a, 0xa1, 0x98));
        assert_eq!(rgb(theme.colors.code_bg), (0xee, 0xe8, 0xd5));
        assert_eq!(theme.typography.text_size, 17.0);
        assert_eq!(theme.typography.h1_size, 34.0);
        assert!(theme.css.is_some());

        // A dark background picks the dark base theme.
        let dark = theme_from_css(
            "Dark",
            "body { background: #1e1e1e; color: #ddd; }",
            Theme::builtin("github").unwrap(),
            Theme::builtin("night").unwrap(),
        );
        assert!(dark.colors.editor_background.l < 0.5);
        assert!(dark.colors.dialog_surface.l < 0.5);
    }
}

#[cfg(test)]
mod loading_tests {
    use crate::config::VelotypeConfigDirs;
    use crate::theme::ThemeManager;

    #[test]
    fn css_files_in_the_theme_folder_become_themes_and_export_their_css() {
        let root =
            std::env::temp_dir().join(format!("velotype-css-themes-{}", uuid::Uuid::new_v4()));
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::create_dir_all(dirs.themes_dir()).unwrap();
        std::fs::write(
            dirs.themes_dir().join("Paper White.css"),
            "body { background: #fafafa; color: #222; } h1 { color: #c00; }",
        )
        .unwrap();
        let mut manager = ThemeManager::default();
        manager
            .load_custom_themes_from_dirs_for_test(&dirs)
            .unwrap();
        let entry = manager
            .available_themes()
            .iter()
            .find(|entry| entry.id == "css-Paper_White" || entry.id.starts_with("css-"))
            .expect("css theme listed")
            .clone();
        assert!(entry.name.starts_with("Paper White"), "{}", entry.name);
        assert!(manager.set_theme_by_id(&entry.id));
        let theme = manager.unzoomed().clone();
        assert_eq!(theme.name, "Paper White");

        let html = crate::export::render_html("# Hi", &theme, "Doc");
        assert!(html.contains("h1 { color: #c00; }"), "{html}");
        assert!(html.contains("id=\"write\""));
        let _ = std::fs::remove_dir_all(root);
    }
}
