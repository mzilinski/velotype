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

/// Arguments for converting `input` into GitHub Markdown on stdout, with
/// embedded images written below `media_dir`.
pub(crate) fn import_args(input: &Path, media_dir: &Path) -> Vec<String> {
    vec![
        "--to".to_string(),
        "gfm".to_string(),
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
    let result = command(pandoc)
        .args(import_args(input, media_dir))
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to start '{}'", pandoc.display()))?;
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
        let args = import_args(&input, &media);
        assert_eq!(&args[..3], ["--to", "gfm", "--wrap=none"]);
        assert_eq!(args.last().unwrap(), &input.to_string_lossy().to_string());
    }

    /// Runs only where Pandoc is installed.
    #[test]
    fn round_trip_through_installed_pandoc() {
        let Some(pandoc) = find_pandoc() else {
            return;
        };
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
}
