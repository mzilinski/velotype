//! User-configurable PDF page setup.
//!
//! The options are persisted in the `[pdf_export]` section of `config.toml`
//! and shared by the print CSS (`@page`) and the Chromium print parameters.

/// Paper sizes offered for PDF export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PdfPaperSize {
    /// ISO A4, 210 x 297 mm.
    #[default]
    A4,
    /// US Letter, 8.5 x 11 in.
    Letter,
    /// ISO A5, 148 x 210 mm.
    A5,
    /// US Legal, 8.5 x 14 in.
    Legal,
}

impl PdfPaperSize {
    /// Every paper size in the order shown in the preferences window.
    pub(crate) const ALL: [Self; 4] = [Self::A4, Self::Letter, Self::A5, Self::Legal];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::A4 => "a4",
            Self::Letter => "letter",
            Self::A5 => "a5",
            Self::Legal => "legal",
        }
    }

    pub(crate) fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "letter" => Self::Letter,
            "a5" => Self::A5,
            "legal" => Self::Legal,
            _ => Self::A4,
        }
    }

    /// Human-readable name; paper sizes are not translated.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::Letter => "Letter",
            Self::A5 => "A5",
            Self::Legal => "Legal",
        }
    }

    /// Portrait width and height in inches, as Chromium expects them.
    fn portrait_inches(self) -> (f64, f64) {
        match self {
            Self::A4 => (210.0 / MM_PER_INCH, 297.0 / MM_PER_INCH),
            Self::Letter => (8.5, 11.0),
            Self::A5 => (148.0 / MM_PER_INCH, 210.0 / MM_PER_INCH),
            Self::Legal => (8.5, 14.0),
        }
    }
}

/// Page orientation for PDF export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PdfOrientation {
    #[default]
    Portrait,
    Landscape,
}

impl PdfOrientation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }

    pub(crate) fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "landscape" => Self::Landscape,
            _ => Self::Portrait,
        }
    }
}

const MM_PER_INCH: f64 = 25.4;
/// Default page margin, matching the previous hard-coded print CSS.
pub(crate) const DEFAULT_PDF_MARGIN_MM: u32 = 15;
/// Upper bound for the configured margin so pages always keep content area.
pub(crate) const MAX_PDF_MARGIN_MM: u32 = 50;
/// Bottom margin needed to fit the page-number footer.
const MIN_FOOTER_MARGIN_MM: u32 = 10;

/// PDF page setup persisted in the preferences file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PdfExportOptions {
    pub(crate) paper_size: PdfPaperSize,
    pub(crate) orientation: PdfOrientation,
    /// Page margin on every side in millimetres.
    pub(crate) margin_mm: u32,
    /// Print "page / total" centred in the page footer.
    pub(crate) page_numbers: bool,
}

impl Default for PdfExportOptions {
    fn default() -> Self {
        Self {
            paper_size: PdfPaperSize::default(),
            orientation: PdfOrientation::default(),
            margin_mm: DEFAULT_PDF_MARGIN_MM,
            page_numbers: false,
        }
    }
}

impl PdfExportOptions {
    /// Reads the `[pdf_export]` table, falling back per field on bad values.
    pub(crate) fn from_toml(value: Option<&toml::Value>) -> Self {
        let defaults = Self::default();
        let Some(table) = value else {
            return defaults;
        };
        let text = |key: &str| table.get(key).and_then(|value| value.as_str());
        Self {
            paper_size: text("paper_size")
                .map(PdfPaperSize::from_str)
                .unwrap_or(defaults.paper_size),
            orientation: text("orientation")
                .map(PdfOrientation::from_str)
                .unwrap_or(defaults.orientation),
            margin_mm: table
                .get("margin_mm")
                .and_then(|value| value.as_integer())
                .and_then(|value| u32::try_from(value).ok())
                .map(|value| value.min(MAX_PDF_MARGIN_MM))
                .unwrap_or(defaults.margin_mm),
            page_numbers: table
                .get("page_numbers")
                .and_then(|value| value.as_bool())
                .unwrap_or(defaults.page_numbers),
        }
    }

    /// Page width and height in inches after applying the orientation.
    pub(crate) fn paper_inches(&self) -> (f64, f64) {
        let (width, height) = self.paper_size.portrait_inches();
        match self.orientation {
            PdfOrientation::Portrait => (width, height),
            PdfOrientation::Landscape => (height, width),
        }
    }

    /// Margins as `(top, bottom, left/right)` millimetres. The bottom margin is
    /// widened when page numbers need room for the footer.
    pub(crate) fn margins_mm(&self) -> (u32, u32, u32) {
        let margin = self.margin_mm.min(MAX_PDF_MARGIN_MM);
        let bottom = if self.page_numbers {
            margin.max(MIN_FOOTER_MARGIN_MM)
        } else {
            margin
        };
        (margin, bottom, margin)
    }

    /// The `@page` rule used by the Chromium print stylesheet.
    pub(crate) fn page_css(&self) -> String {
        let (top, bottom, side) = self.margins_mm();
        let size = match self.paper_size {
            PdfPaperSize::A4 => "A4",
            PdfPaperSize::Letter => "letter",
            PdfPaperSize::A5 => "A5",
            PdfPaperSize::Legal => "legal",
        };
        format!(
            "@page {{\n  size: {size} {};\n  margin: {top}mm {side}mm {bottom}mm {side}mm;\n}}",
            self.orientation.as_str()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{PdfExportOptions, PdfOrientation, PdfPaperSize};

    #[test]
    fn default_options_match_previous_print_layout() {
        let options = PdfExportOptions::default();

        assert_eq!(options.paper_size, PdfPaperSize::A4);
        assert_eq!(options.orientation, PdfOrientation::Portrait);
        assert_eq!(options.margin_mm, 15);
        assert!(!options.page_numbers);
        assert_eq!(
            options.page_css(),
            "@page {\n  size: A4 portrait;\n  margin: 15mm 15mm 15mm 15mm;\n}"
        );
    }

    #[test]
    fn parses_toml_and_falls_back_per_field() {
        let value: toml::Value = toml::from_str(
            "paper_size = \"Letter\"\norientation = \"sideways\"\nmargin_mm = 400\npage_numbers = true",
        )
        .unwrap();
        let options = PdfExportOptions::from_toml(Some(&value));

        assert_eq!(options.paper_size, PdfPaperSize::Letter);
        assert_eq!(options.orientation, PdfOrientation::Portrait);
        assert_eq!(options.margin_mm, 50);
        assert!(options.page_numbers);
        assert_eq!(
            PdfExportOptions::from_toml(None),
            PdfExportOptions::default()
        );
    }

    #[test]
    fn landscape_swaps_paper_dimensions() {
        let options = PdfExportOptions {
            paper_size: PdfPaperSize::Legal,
            orientation: PdfOrientation::Landscape,
            ..PdfExportOptions::default()
        };

        assert_eq!(options.paper_inches(), (14.0, 8.5));
        assert!(options.page_css().contains("size: legal landscape;"));
    }

    #[test]
    fn page_numbers_reserve_footer_room() {
        let options = PdfExportOptions {
            margin_mm: 0,
            page_numbers: true,
            ..PdfExportOptions::default()
        };

        assert_eq!(options.margins_mm(), (0, 10, 0));
        assert!(options.page_css().contains("margin: 0mm 0mm 10mm 0mm;"));
    }
}
