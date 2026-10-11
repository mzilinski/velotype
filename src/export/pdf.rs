//! PDF generation through a local Chromium-compatible browser.
//!
//! The browser HTML export is the source of truth for visual PDF fidelity. This
//! module writes that HTML to a temporary file, opens it in headless Chromium,
//! and asks DevTools to print the page to PDF.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, anyhow};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;
use chromiumoxide::detection::{DetectionOptions, default_executable};
use futures::StreamExt;
use uuid::Uuid;

use crate::export::html::{
    css_color, escape_html, render_chromium_pdf_html_with_base_dir_and_fonts,
};
use crate::export::options::PdfExportOptions;
use crate::fonts::FontSettings;
use crate::theme::Theme;

const CHROMIUM_VIEWPORT_WIDTH: u32 = 1280;
const CHROMIUM_VIEWPORT_HEIGHT: u32 = 1600;
const PDF_TIMEOUT: Duration = Duration::from_secs(45);
const MM_PER_INCH: f64 = 25.4;

/// Locates a Chromium-family browser for PDF printing.
///
/// Uses the same lookup as chromiumoxide (`CHROME` variable, `PATH`, the
/// Windows registry and common install locations) and additionally checks
/// install paths it misses, such as per-user Chrome and Brave installs.
pub(crate) fn find_chromium_executable() -> Option<PathBuf> {
    default_executable(DetectionOptions::default())
        .ok()
        .or_else(|| {
            extra_browser_paths()
                .into_iter()
                .find(|path| path.is_file())
        })
}

/// The first file named `name` in a directory of `PATH`.
#[cfg(all(unix, not(target_os = "macos")))]
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn extra_browser_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        const SUFFIXES: [&str; 5] = [
            r"Google\Chrome\Application\chrome.exe",
            r"Microsoft\Edge\Application\msedge.exe",
            r"BraveSoftware\Brave-Browser\Application\brave.exe",
            r"Chromium\Application\chrome.exe",
            r"Vivaldi\Application\vivaldi.exe",
        ];
        for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(root) {
                let root = PathBuf::from(root);
                paths.extend(SUFFIXES.iter().map(|suffix| root.join(suffix)));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for app in [
                "Google Chrome.app/Contents/MacOS/Google Chrome",
                "Chromium.app/Contents/MacOS/Chromium",
                "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "Brave Browser.app/Contents/MacOS/Brave Browser",
            ] {
                paths.push(home.join("Applications").join(app));
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for name in ["brave-browser", "brave", "vivaldi", "vivaldi-stable"] {
            if let Some(path) = find_in_path(name) {
                paths.push(path);
            }
        }
        for path in [
            "/snap/bin/chromium",
            "/snap/bin/brave",
            "/var/lib/flatpak/exports/bin/com.google.Chrome",
            "/var/lib/flatpak/exports/bin/org.chromium.Chromium",
            "/opt/brave.com/brave/brave",
            "/opt/microsoft/msedge/msedge",
        ] {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}

pub(crate) fn render_pdf_with_fonts(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_path: Option<&Path>,
    fonts: &FontSettings,
    options: &PdfExportOptions,
) -> anyhow::Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("velotype-pdf-export")
        .build()
        .context("failed to create PDF export runtime")?;
    runtime.block_on(async move {
        tokio::time::timeout(
            PDF_TIMEOUT,
            render_pdf_async_with_fonts(markdown, theme, title, base_path, fonts, options),
        )
        .await
        .map_err(|_| anyhow!("PDF export timed out while waiting for Chromium"))?
    })
}

async fn render_pdf_async_with_fonts(
    markdown: &str,
    theme: &Theme,
    title: &str,
    base_path: Option<&Path>,
    fonts: &FontSettings,
    options: &PdfExportOptions,
) -> anyhow::Result<Vec<u8>> {
    let html = render_chromium_pdf_html_with_base_dir_and_fonts(
        markdown, theme, title, base_path, fonts, options,
    );
    let temp = PdfTempFiles::create(&html)?;
    let params = chromium_pdf_params(options, theme, fonts);
    let result = render_pdf_from_html_file_async(temp.html_path.clone(), params).await;
    temp.cleanup();
    result
}

async fn render_pdf_from_html_file_async(
    html_path: PathBuf,
    params: PrintToPdfParams,
) -> anyhow::Result<Vec<u8>> {
    let executable = find_chromium_executable().ok_or_else(|| {
        anyhow!(
            "no Chromium-compatible browser found for PDF export. Install Chrome, Chromium, or Edge, or set the CHROME environment variable to the browser executable path"
        )
    })?;
    let user_data_dir = unique_temp_path("velotype-chromium-profile");
    fs::create_dir_all(&user_data_dir)
        .with_context(|| format!("failed to create '{}'", user_data_dir.display()))?;

    let config = BrowserConfig::builder()
        .chrome_executable(executable)
        .new_headless_mode()
        .window_size(CHROMIUM_VIEWPORT_WIDTH, CHROMIUM_VIEWPORT_HEIGHT)
        .user_data_dir(user_data_dir.clone())
        .build()
        .map_err(|err| anyhow!("failed to build Chromium browser config: {err}"))?;

    let (mut browser, mut handler) = Browser::launch(config).await.map_err(|err| {
        anyhow!(
            "failed to launch Chromium for PDF export: {err}. Install Chrome, Chromium, or Edge, or set the CHROME environment variable to the browser executable path"
        )
    })?;

    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let result = async {
        let file_url = file_url_from_path(&html_path)?;
        let page = browser
            .new_page(file_url.as_str())
            .await
            .context("failed to open export HTML in Chromium")?;
        page.wait_for_navigation()
            .await
            .context("Chromium did not finish loading export HTML")?;

        page.pdf(params)
            .await
            .context("Chromium failed to print export HTML to PDF")
    }
    .await;

    let _ = browser.close().await;
    handler_task.abort();
    let _ = fs::remove_dir_all(&user_data_dir);

    result
}

/// Print parameters mirroring the `@page` rule of the print stylesheet. The
/// CSS page size wins in Chromium; the explicit size and margins keep the
/// output identical if a browser ignores `prefer_css_page_size`.
fn chromium_pdf_params(
    options: &PdfExportOptions,
    theme: &Theme,
    fonts: &FontSettings,
) -> PrintToPdfParams {
    let (width, height) = options.paper_inches();
    let (top, bottom, side) = options.margins_mm();
    let inches = |mm: u32| f64::from(mm) / MM_PER_INCH;
    let footer = options.page_numbers.then(|| {
        (
            // An empty element suppresses Chromium's default date/title header.
            "<span></span>".to_string(),
            page_number_footer_template(theme, fonts),
        )
    });
    let display_header_footer = footer.is_some().then_some(true);
    let (header_template, footer_template) = footer.unzip();
    PrintToPdfParams {
        print_background: Some(true),
        prefer_css_page_size: Some(true),
        paper_width: Some(width),
        paper_height: Some(height),
        margin_top: Some(inches(top)),
        margin_bottom: Some(inches(bottom)),
        margin_left: Some(inches(side)),
        margin_right: Some(inches(side)),
        display_header_footer,
        header_template,
        footer_template,
        ..PrintToPdfParams::default()
    }
}

/// Centred "page / total" footer. Chromium renders templates in an isolated
/// document, so every style has to be inline.
fn page_number_footer_template(theme: &Theme, fonts: &FontSettings) -> String {
    format!(
        "<div style=\"width: 100%; font-size: 9px; text-align: center; color: {color}; font-family: {family};\"><span class=\"pageNumber\"></span> / <span class=\"totalPages\"></span></div>",
        color = css_color(theme.colors.dialog_muted),
        family = escape_html(&fonts.body.css_font_family()),
    )
}

fn file_url_from_path(path: &Path) -> anyhow::Result<url::Url> {
    url::Url::from_file_path(path)
        .map_err(|_| anyhow!("failed to convert '{}' to a file URL", path.display()))
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
}

struct PdfTempFiles {
    html_path: PathBuf,
}

impl PdfTempFiles {
    fn create(html: &str) -> anyhow::Result<Self> {
        let html_path = unique_temp_path("velotype-export").with_extension("html");
        fs::write(&html_path, html)
            .with_context(|| format!("failed to write temporary HTML '{}'", html_path.display()))?;
        Ok(Self { html_path })
    }

    fn cleanup(&self) {
        let _ = fs::remove_file(&self.html_path);
    }
}

impl Drop for PdfTempFiles {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::{chromium_pdf_params, file_url_from_path, render_pdf_with_fonts};
    use crate::export::html::render_chromium_pdf_html_with_base_dir_and_fonts;
    use crate::export::options::{PdfExportOptions, PdfOrientation, PdfPaperSize};
    use crate::fonts::{FontCatalog, FontPreferences, FontSettings, SYSTEM_UI_FONT};
    use crate::theme::Theme;

    fn default_fonts() -> FontSettings {
        FontSettings::resolve(
            FontPreferences::default(),
            &FontCatalog::from_names(vec![SYSTEM_UI_FONT.into()]),
            std::env::consts::OS,
        )
        .expect("default font stacks are valid")
    }

    #[test]
    fn chromium_pdf_html_uses_print_layout_and_preserves_resources() {
        let html = render_chromium_pdf_html_with_base_dir_and_fonts(
            "# Title\n\n```mermaid\nflowchart LR\nA --> B\n```\n\n$$\nx^2\n$$",
            &Theme::default_theme(),
            "Doc",
            None,
            &default_fonts(),
            &PdfExportOptions::default(),
        );

        assert!(html.contains("@page"));
        assert!(html.contains("size: A4 portrait"));
        assert!(html.contains("margin: 15mm 15mm 15mm 15mm"));
        assert!(html.contains("class=\"vlt-document\""));
        assert!(html.contains("data:image/svg+xml;base64,"));
        assert!(html.contains("<svg"));
        assert!(!html.contains("width: min(100% - 48px, 920px);"));
    }

    #[test]
    fn chromium_pdf_html_follows_page_setup() {
        let options = PdfExportOptions {
            paper_size: PdfPaperSize::Letter,
            orientation: PdfOrientation::Landscape,
            margin_mm: 20,
            page_numbers: false,
        };
        let html = render_chromium_pdf_html_with_base_dir_and_fonts(
            "text",
            &Theme::default_theme(),
            "Doc",
            None,
            &default_fonts(),
            &options,
        );

        assert!(html.contains("size: letter landscape;"));
        assert!(html.contains("margin: 20mm 20mm 20mm 20mm;"));
        assert!(!html.contains("size: A4"));
    }

    #[test]
    fn chromium_pdf_params_use_page_css_and_backgrounds() {
        let params = chromium_pdf_params(
            &PdfExportOptions::default(),
            &Theme::default_theme(),
            &default_fonts(),
        );

        assert_eq!(params.print_background, Some(true));
        assert_eq!(params.prefer_css_page_size, Some(true));
        let a4 = params.paper_width.zip(params.paper_height).unwrap();
        assert!((a4.0 - 8.27).abs() < 0.01 && (a4.1 - 11.69).abs() < 0.01);
        let margin = 15.0 / 25.4;
        assert_eq!(params.margin_top, Some(margin));
        assert_eq!(params.margin_bottom, Some(margin));
        assert_eq!(params.margin_left, Some(margin));
        assert_eq!(params.margin_right, Some(margin));
        assert_eq!(params.display_header_footer, None);
        assert_eq!(params.footer_template, None);
    }

    #[test]
    fn chromium_pdf_params_add_page_number_footer() {
        let options = PdfExportOptions {
            paper_size: PdfPaperSize::A5,
            orientation: PdfOrientation::Landscape,
            page_numbers: true,
            ..PdfExportOptions::default()
        };
        let params = chromium_pdf_params(&options, &Theme::default_theme(), &default_fonts());

        assert_eq!(params.display_header_footer, Some(true));
        assert_eq!(params.header_template.as_deref(), Some("<span></span>"));
        let footer = params.footer_template.expect("footer template");
        assert!(
            footer.contains(
                "<span class=\"pageNumber\"></span> / <span class=\"totalPages\"></span>"
            )
        );
        let (width, height) = params.paper_width.zip(params.paper_height).unwrap();
        assert!(width > height, "landscape pages are wider than tall");
    }

    #[test]
    fn file_url_from_path_supports_local_paths() {
        let path = std::env::temp_dir().join("velotype pdf test.html");
        let url = file_url_from_path(&path).expect("file url");

        assert_eq!(url.scheme(), "file");
        assert!(url.as_str().contains("velotype%20pdf%20test.html"));
    }

    #[test]
    fn render_pdf_reports_actionable_error_without_chromium() {
        match render_pdf_with_fonts(
            "# Title\n\nBody",
            &Theme::default_theme(),
            "Doc",
            None,
            &default_fonts(),
            &PdfExportOptions::default(),
        ) {
            Ok(pdf) => assert!(pdf.starts_with(b"%PDF")),
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains("Chromium")
                        || message.contains("Chrome")
                        || message.contains("CHROME"),
                    "unexpected PDF export error: {message}"
                );
            }
        }
    }
}
