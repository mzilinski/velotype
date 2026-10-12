//! Export to and import from other formats through the Pandoc command line
//! tool (File > Export > Word, LaTeX, ... and File > Import). Pandoc is not
//! bundled; it is looked up in `PATH` and the usual install locations.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, bail};

/// A Pandoc output format offered in the Export menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PandocFormat {
    /// Pandoc writer name, also the action parameter.
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) extension: &'static str,
}

pub(crate) const EXPORT_FORMATS: &[PandocFormat] = &[
    PandocFormat {
        id: "docx",
        label: "Word (.docx)",
        extension: "docx",
    },
    PandocFormat {
        id: "odt",
        label: "OpenDocument (.odt)",
        extension: "odt",
    },
    PandocFormat {
        id: "rtf",
        label: "RTF",
        extension: "rtf",
    },
    PandocFormat {
        id: "epub",
        label: "EPUB",
        extension: "epub",
    },
    PandocFormat {
        id: "latex",
        label: "LaTeX",
        extension: "tex",
    },
    PandocFormat {
        id: "rst",
        label: "reStructuredText",
        extension: "rst",
    },
    PandocFormat {
        id: "mediawiki",
        label: "MediaWiki",
        extension: "wiki",
    },
    PandocFormat {
        id: "org",
        label: "Org Mode",
        extension: "org",
    },
];

pub(crate) fn export_format(id: &str) -> Option<PandocFormat> {
    EXPORT_FORMATS
        .iter()
        .copied()
        .find(|format| format.id == id)
}

/// Where Pandoc's download page is, for the "not installed" prompt.
pub(crate) const INSTALL_URL: &str = "https://pandoc.org/installing.html";

/// Velotype's Markdown as Pandoc reads it: Pandoc Markdown with bare URLs
/// turned into links (GitHub style).
const MARKDOWN_READER: &str = "markdown+autolink_bare_uris";

/// The Pandoc executable, from `PATH` or a standard install location
/// (apps started from the Finder do not see the shell's `PATH`).
pub(crate) fn find_pandoc() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    let from_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    });
    from_path.or_else(|| {
        install_locations()
            .into_iter()
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn install_locations() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                dirs.push(PathBuf::from(base).join("Pandoc"));
            }
        }
    } else {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/bin"));
        }
    }
    dirs
}

fn command(pandoc: &Path) -> Command {
    let mut command = Command::new(pandoc);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // CREATE_NO_WINDOW: no console window flashing up.
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Arguments for converting Markdown from stdin into `format` at `output`.
pub(crate) fn export_args(
    format: PandocFormat,
    output: &Path,
    resource_dir: Option<&Path>,
    title: &str,
) -> Vec<String> {
    let mut args = vec![
        "--from".to_string(),
        MARKDOWN_READER.to_string(),
        "--to".to_string(),
        format.id.to_string(),
        "--standalone".to_string(),
        // Used only when the document has no title of its own.
        format!("--metadata=pagetitle:{title}"),
        "--output".to_string(),
        output.to_string_lossy().to_string(),
    ];
    if let Some(dir) = resource_dir {
        args.push(format!("--resource-path={}", dir.to_string_lossy()));
    }
    args
}

/// GitHub Markdown with `$…$` / `$$…$$` math as Velotype writes it.
const MARKDOWN_WRITER: &str = "gfm-tex_math_gfm+tex_math_dollars";
/// For Pandoc versions before `tex_math_gfm` existed (math then comes out
/// in GitHub's `` $`…`$ `` form).
const FALLBACK_MARKDOWN_WRITER: &str = "gfm";

/// Arguments for converting `input` into GitHub Markdown (`writer`) on
/// stdout, with embedded images written below `media_dir`.
pub(crate) fn import_args(input: &Path, media_dir: &Path, writer: &str) -> Vec<String> {
    vec![
        "--to".to_string(),
        writer.to_string(),
        "--wrap=none".to_string(),
        format!("--extract-media={}", media_dir.to_string_lossy()),
        input.to_string_lossy().to_string(),
    ]
}

/// Converts `markdown` with Pandoc and writes the result to `output`.
pub(crate) fn export(
    pandoc: &Path,
    markdown: &str,
    format: PandocFormat,
    output: &Path,
    resource_dir: Option<&Path>,
    title: &str,
) -> anyhow::Result<()> {
    let mut child = command(pandoc)
        .args(export_args(format, output, resource_dir, title))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start '{}'", pandoc.display()))?;
    child
        .stdin
        .take()
        .context("pandoc stdin unavailable")?
        .write_all(markdown.as_bytes())
        .context("failed to pass the document to pandoc")?;
    let result = child.wait_with_output().context("pandoc did not finish")?;
    if !result.status.success() {
        bail!("{}", String::from_utf8_lossy(&result.stderr).trim());
    }
    Ok(())
}

/// Converts `input` (Word, OpenDocument, HTML, LaTeX, EPUB, ...) to
/// Markdown; images go to `media_dir`.
pub(crate) fn import(pandoc: &Path, input: &Path, media_dir: &Path) -> anyhow::Result<String> {
    let run = |writer: &str| {
        command(pandoc)
            .args(import_args(input, media_dir, writer))
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to start '{}'", pandoc.display()))
    };
    let mut result = run(MARKDOWN_WRITER)?;
    if !result.status.success() && String::from_utf8_lossy(&result.stderr).contains("tex_math_gfm")
    {
        result = run(FALLBACK_MARKDOWN_WRITER)?;
    }
    if !result.status.success() {
        bail!("{}", String::from_utf8_lossy(&result.stderr).trim());
    }
    String::from_utf8(result.stdout).context("pandoc returned text that is not UTF-8")
}

/// Folder for the images of an imported file: `<name>-media` next to it.
pub(crate) fn media_dir_for(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "import".to_string());
    input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(format!("{stem}-media"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_arguments_name_reader_writer_and_resources() {
        let format = export_format("docx").unwrap();
        let args = export_args(
            format,
            Path::new("out.docx"),
            Some(Path::new("docs")),
            "Notes",
        );
        assert_eq!(&args[..4], ["--from", MARKDOWN_READER, "--to", "docx"]);
        assert!(args.contains(&"--standalone".to_string()));
        assert!(args.contains(&"--metadata=pagetitle:Notes".to_string()));
        assert!(args.contains(&"out.docx".to_string()));
        assert!(args.contains(&"--resource-path=docs".to_string()));
        assert!(export_format("pdf").is_none());
    }

    #[test]
    fn import_writes_media_next_to_the_source() {
        let input = Path::new("dir").join("Report.docx");
        let media = media_dir_for(&input);
        assert_eq!(media, Path::new("dir").join("Report-media"));
        let args = import_args(&input, &media, MARKDOWN_WRITER);
        assert_eq!(&args[..3], ["--to", MARKDOWN_WRITER, "--wrap=none"]);
        assert_eq!(args.last().unwrap(), &input.to_string_lossy().to_string());
    }

    /// Runs only where Pandoc is installed.
    #[test]
    fn round_trip_through_installed_pandoc() {
        let Some(pandoc) = find_pandoc() else {
            eprintln!("pandoc not installed, round trip skipped");
            return;
        };
        eprintln!("using {}", pandoc.display());
        let dir = std::env::temp_dir().join(format!("velotype-pandoc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("doc.docx");
        export(
            &pandoc,
            "# Title\n\nSome **bold** text.\n",
            export_format("docx").unwrap(),
            &output,
            None,
            "doc",
        )
        .unwrap();
        let markdown = import(&pandoc, &output, &media_dir_for(&output)).unwrap();
        assert!(markdown.contains("# Title"), "{markdown}");
        assert!(markdown.contains("**bold**"), "{markdown}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Every export format, then importing the Word file again, in a folder
    /// whose name has spaces and an umlaut. Runs only where Pandoc is
    /// installed.
    #[test]
    fn all_formats_and_import_with_images() {
        let Some(pandoc) = find_pandoc() else {
            eprintln!("pandoc not installed, export formats skipped");
            return;
        };
        let dir = std::env::temp_dir()
            .join(format!("velotype pandoc {}", uuid::Uuid::new_v4()))
            .join("Ordner mit Ümlaut");
        std::fs::create_dir_all(dir.join("img")).unwrap();
        // A 1x1 PNG.
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99,
            0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(dir.join("img").join("bild 1.png"), PNG).unwrap();
        let markdown = [
            "# Überschrift",
            "",
            "Text mit **fett**, $E = mc^2$ und Fußnote[^1].",
            "",
            "| A | B |",
            "|---|---|",
            "| 1 | 2 |",
            "",
            "- [x] erledigt",
            "- [ ] offen",
            "",
            "![Bild](<img/bild 1.png>)",
            "",
            "[^1]: Die Fußnote.",
            "",
        ]
        .join("\n");
        let markdown = markdown.as_str();
        for format in EXPORT_FORMATS {
            let output = dir.join(format!("Dokument äö.{}", format.extension));
            export(&pandoc, markdown, *format, &output, Some(&dir), "Dokument")
                .unwrap_or_else(|err| panic!("{}: {err}", format.id));
            let size = std::fs::metadata(&output)
                .map(|meta| meta.len())
                .unwrap_or(0);
            assert!(size > 0, "{} produced no output", format.id);
        }
        let latex = std::fs::read_to_string(dir.join("Dokument äö.tex")).unwrap();
        assert!(latex.contains("Überschrift"), "{latex}");
        assert!(latex.contains("E = mc^2"), "{latex}");

        let docx = dir.join("Dokument äö.docx");
        let media = media_dir_for(&docx);
        let imported = import(&pandoc, &docx, &media).unwrap();
        assert!(imported.contains("# Überschrift"), "{imported}");
        assert!(imported.contains("| A"), "{imported}");
        assert!(imported.contains("Fußnote"), "{imported}");
        assert!(imported.contains("$E = mc^{2}$"), "{imported}");
        let extracted = std::fs::read_dir(media.join("media"))
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert!(
            extracted > 0,
            "no image extracted:
{imported}"
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
