//! Document export helpers for HTML and PDF output.
//!
//! Export starts from the same Markdown text used by document saving. The
//! module owns format-specific rendering so editor code only chooses paths and
//! supplies the current theme.

use crate::fonts::FontSettings;
use crate::theme::Theme;

mod html;
mod options;
pub(crate) mod pandoc;
mod pdf;

pub(crate) use options::{DEFAULT_PDF_MARGIN_MM, PdfExportOptions, PdfOrientation, PdfPaperSize};
pub(crate) use pdf::find_chromium_executable;

/// Export target selected from the app menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    /// Full HTML document with embedded theme CSS.
    Html,
    /// PDF bytes rendered from the themed HTML document.
    Pdf,
    /// HTML document without any CSS (File > Export > HTML without Styles).
    HtmlPlain,
}

impl ExportFormat {
    /// File extension used for save-dialog defaults.
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::HtmlPlain => "html",
        }
    }
}

#[cfg(test)]
pub(crate) use html::render_html;
pub(crate) use html::{render_html_with_base_dir_and_fonts, render_html_without_styles};

pub(crate) fn render_pdf_with_fonts(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_path: Option<&std::path::Path>,
    fonts: &FontSettings,
    options: &PdfExportOptions,
) -> anyhow::Result<Vec<u8>> {
    pdf::render_pdf_with_fonts(markdown, theme, title, base_path, fonts, options)
}
