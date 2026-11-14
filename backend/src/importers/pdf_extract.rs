//! Thin adapter around the external `pdf-extract` tool.
//!
//! Fynance never renders or parses PDFs itself. PDF-capable models get the
//! native file; providers that cannot take PDFs go through the external
//! `pdf-extract` tool, which splits a PDF into per-page text and page images.
//! The tool contract (final, 2026-11-13) lives in one place in this module:
//! the `OUTPUT_FILE` constant, the argument builder in [`extract_pdf`], and
//! the [`ToolOutput`] serde structs.
//!
//! ```text
//! <FYNANCE_PDF_EXTRACT_CMD> <input.pdf> --out <dir> [FYNANCE_PDF_EXTRACT_ARGS...]
//! ```
//!
//! The tool writes `<dir>/extract.json` plus one PNG per page. Pages are in
//! strictly ascending order by real page number, but need not be 1..n (the
//! tool's `--pages` flag skips pages). Exit 10 means some pages failed: the
//! manifest is still written, good pages are returned and failed pages are
//! reported in [`ExtractedPdf::page_errors`].
//!
//! Page text and file contents are never logged; only counts, byte totals
//! and durations.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Deserialize;
use thiserror::Error;
use tracing::debug;

/// Manifest the tool writes inside its `--out` directory.
const OUTPUT_FILE: &str = "extract.json";
/// Name the input PDF gets inside the temporary working directory.
const INPUT_FILE: &str = "input.pdf";
/// Subdirectory of the temp dir passed as `--out`.
const OUT_DIR: &str = "out";

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const STDERR_TAIL_CHARS: usize = 500;
/// Only used when a page omits `has_text_layer`; the final tool contract
/// always sets it.
const TEXT_LAYER_MIN_NON_WS: usize = 20;
/// Tool exit code for "finished, but some pages failed".
const PARTIAL_SUCCESS_EXIT: i32 = 10;
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Where to find the tool and how long to let it run.
#[derive(Debug, Clone)]
pub struct PdfExtractConfig {
    /// Path to the tool executable (or a wrapper script). `None` when
    /// `FYNANCE_PDF_EXTRACT_CMD` is unset.
    pub cmd: Option<PathBuf>,
    /// Extra arguments from `FYNANCE_PDF_EXTRACT_ARGS` (a JSON string array,
    /// e.g. `["--dpi","150"]`), appended after `--out <dir>`.
    pub extra_args: Vec<String>,
    /// `FYNANCE_PDF_EXTRACT_TIMEOUT_SECS`, default 120.
    pub timeout: Duration,
}

impl PdfExtractConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let cmd = std::env::var("FYNANCE_PDF_EXTRACT_CMD")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        let extra_args = match std::env::var("FYNANCE_PDF_EXTRACT_ARGS") {
            Ok(raw) => serde_json::from_str::<Vec<String>>(&raw).map_err(|_| {
                anyhow::anyhow!(
                    "FYNANCE_PDF_EXTRACT_ARGS must be a JSON array of strings, e.g. [\"--dpi\",\"150\"]"
                )
            })?,
            Err(_) => Vec::new(),
        };
        let timeout = match std::env::var("FYNANCE_PDF_EXTRACT_TIMEOUT_SECS") {
            Ok(raw) => raw
                .parse::<u64>()
                .map_err(|_| {
                    anyhow::anyhow!(
                        "FYNANCE_PDF_EXTRACT_TIMEOUT_SECS must be a whole number of seconds, got {raw:?}"
                    )
                })
                .map(Duration::from_secs)?,
            Err(_) => Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        };
        Ok(Self {
            cmd,
            extra_args,
            timeout,
        })
    }
}

#[derive(Error, Debug)]
pub enum PdfExtractError {
    #[error(
        "no PDF extraction tool is configured: set FYNANCE_PDF_EXTRACT_CMD to the pdf-extract executable, \
         or set FYNANCE_OPENAI_PDF_MODE=native to send the PDF to the model instead"
    )]
    NotConfigured,
    #[error(
        "PDF extraction tool `{cmd}` could not be run (not found or not executable): set \
         FYNANCE_PDF_EXTRACT_CMD to the pdf-extract executable, or set FYNANCE_OPENAI_PDF_MODE=native \
         to send the PDF to the model instead"
    )]
    NotFound { cmd: String },
    #[error("PDF extraction tool failed (exit {status}): {stderr_tail}{meaning}")]
    Failed {
        status: i32,
        stderr_tail: String,
        meaning: &'static str,
    },
    #[error("PDF extraction tool timed out after {secs} seconds")]
    TimedOut { secs: u64 },
    #[error("the PDF extraction tool's output did not match the expected format: {0}")]
    BadOutput(String),
}

/// What each documented pdf-extract exit code means, in one clause appended
/// to the `Failed` message. Codes outside the tool's documented set add
/// nothing; the stderr tail carries the tool's own words.
fn exit_code_meaning(status: i32) -> &'static str {
    match status {
        1 => ", the tool hit an unexpected internal error",
        2 => ", the tool was called with invalid arguments",
        3 => ", the tool could not find the input file",
        4 => ", the PDF is password protected and the tool was given no password",
        5 => ", the PDF is password protected and the password the tool was given is wrong",
        6 => ", the input is not a readable PDF",
        7 => ", the tool could not write to its output folder",
        _ => "",
    }
}

#[derive(Debug)]
pub struct ExtractedPdf {
    /// Pages that completed, in ascending real-page order.
    pub pages: Vec<ExtractedPage>,
    /// (real page number, tool's error description) for pages that failed on
    /// a partial-success run. Empty on a clean run.
    pub page_errors: Vec<(u32, String)>,
}

#[derive(Debug)]
pub struct ExtractedPage {
    /// Real page number in the document, not a 1..n index into `pages`.
    pub page: u32,
    pub text: String,
    pub has_text_layer: bool,
    /// Rendered page image. Empty when the tool ran with `--no-images`.
    pub png: Vec<u8>,
}

/// Run the tool over `bytes` and read its pages back. `filename` only labels
/// log and error messages; the file written to the temp dir is always
/// `input.pdf`.
pub async fn extract_pdf(
    bytes: &[u8],
    filename: &str,
    cfg: &PdfExtractConfig,
) -> Result<ExtractedPdf, PdfExtractError> {
    let cmd = cfg.cmd.clone().ok_or(PdfExtractError::NotConfigured)?;
    let started = Instant::now();

    let temp = tempfile::TempDir::new()
        .map_err(|e| PdfExtractError::BadOutput(format!("could not create a temp dir: {e}")))?;
    let input = temp.path().join(INPUT_FILE);
    let out = temp.path().join(OUT_DIR);
    std::fs::create_dir(&out).map_err(|e| {
        PdfExtractError::BadOutput(format!("could not create {}: {e}", out.display()))
    })?;
    std::fs::write(&input, bytes)
        .map_err(|e| PdfExtractError::BadOutput(format!("could not write input PDF: {e}")))?;

    let mut command = tokio::process::Command::new(&cmd);
    command
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .args(&cfg.extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = match command.spawn() {
        Ok(child) => child,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            return Err(PdfExtractError::NotFound {
                cmd: cmd.display().to_string(),
            });
        }
        Err(e) => {
            return Err(PdfExtractError::NotFound {
                cmd: format!("{} ({e})", cmd.display()),
            });
        }
    };

    let secs = cfg.timeout.as_secs();
    let output = match tokio::time::timeout(cfg.timeout, child.wait_with_output()).await {
        Err(_) => return Err(PdfExtractError::TimedOut { secs }),
        Ok(Err(e)) => {
            return Err(PdfExtractError::NotFound {
                cmd: format!("{} ({e})", cmd.display()),
            });
        }
        Ok(Ok(output)) => output,
    };

    let partial = match output.status.code() {
        Some(0) => false,
        Some(PARTIAL_SUCCESS_EXIT) => true,
        Some(code) => {
            return Err(failed_from_stderr(code, &output.stderr));
        }
        // Killed by a signal (Unix): no exit code, treat as a plain failure.
        None => {
            return Err(failed_from_stderr(-1, &output.stderr));
        }
    };

    let manifest_path = out.join(OUTPUT_FILE);
    let manifest = std::fs::read(&manifest_path).map_err(|_| {
        PdfExtractError::BadOutput(format!(
            "the tool produced no {OUTPUT_FILE} in its output directory"
        ))
    })?;
    let tool: ToolOutput = serde_json::from_slice(&manifest).map_err(|e| {
        PdfExtractError::BadOutput(format!(
            "{OUTPUT_FILE} did not match the expected schema: {e}"
        ))
    })?;
    if tool.schema_version != 1 {
        return Err(PdfExtractError::BadOutput(format!(
            "unsupported schema_version {} (expected 1)",
            tool.schema_version
        )));
    }

    let mut pages = Vec::new();
    let mut page_errors = Vec::new();
    let mut prev_page: Option<u32> = None;
    for tool_page in tool.pages {
        if let Some(prev) = prev_page {
            if tool_page.page <= prev {
                return Err(PdfExtractError::BadOutput(format!(
                    "pages are not strictly ascending by page number (page {} follows page {})",
                    tool_page.page, prev
                )));
            }
        }
        prev_page = Some(tool_page.page);

        if let Some(error) = tool_page.error {
            page_errors.push((tool_page.page, error_message(&error)));
            continue;
        }

        let png = match tool_page.image {
            None => Vec::new(),
            Some(image) => read_page_png(&out, &image.path)?,
        };
        let has_text_layer = tool_page
            .has_text_layer
            .unwrap_or_else(|| non_whitespace_chars(&tool_page.text) >= TEXT_LAYER_MIN_NON_WS);
        pages.push(ExtractedPage {
            page: tool_page.page,
            text: tool_page.text,
            has_text_layer,
            png,
        });
    }

    if pages.is_empty() {
        return Err(if partial {
            PdfExtractError::BadOutput("every page of the PDF failed to extract".to_string())
        } else {
            PdfExtractError::BadOutput("the tool reported no pages".to_string())
        });
    }

    debug!(
        filename,
        pages = pages.len(),
        page_errors = page_errors.len(),
        png_bytes = pages.iter().map(|p| p.png.len()).sum::<usize>(),
        duration_ms = started.elapsed().as_millis() as u64,
        "pdf-extract finished"
    );
    Ok(ExtractedPdf { pages, page_errors })
}

fn failed_from_stderr(status: i32, stderr: &[u8]) -> PdfExtractError {
    PdfExtractError::Failed {
        status,
        stderr_tail: stderr_tail(stderr),
        meaning: exit_code_meaning(status),
    }
}

/// Last [`STDERR_TAIL_CHARS`] characters of the tool's stderr, lossy-decoded.
/// Truncation is by char boundary, so the tail never splits a multi-byte
/// character.
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    if text.chars().count() <= STDERR_TAIL_CHARS {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    let start = chars.len() - STDERR_TAIL_CHARS;
    let tail: String = chars.drain(start..).collect();
    format!("\u{2026}{tail}")
}

/// Resolve a tool-reported image path inside `out` only, and read it. The
/// contract guarantees relative, `..`-free paths; the checks here are the
/// defensive half of that.
fn read_page_png(out: &Path, path: &str) -> Result<Vec<u8>, PdfExtractError> {
    let rel = Path::new(path);
    if path.is_empty()
        || rel.is_absolute()
        || rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PdfExtractError::BadOutput(format!(
            "pages[].image.path must be a plain relative path inside the output directory, got {path:?}"
        )));
    }
    let resolved = out.join(rel);
    let png = std::fs::read(&resolved).map_err(|e| {
        PdfExtractError::BadOutput(format!(
            "pages[].image.path {path:?} could not be read from the output directory: {e}"
        ))
    })?;
    if !png.starts_with(PNG_SIGNATURE) {
        return Err(PdfExtractError::BadOutput(format!(
            "pages[].image.path {path:?} is not a PNG"
        )));
    }
    Ok(png)
}

fn error_message(error: &serde_json::Value) -> String {
    match error {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn non_whitespace_chars(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

/// The tool's manifest, as far as Fynance reads it. Unknown fields (numbers,
/// dates, width_pt, tool, source, document, options) are ignored; within
/// schema_version 1 the tool only ever adds fields. `numbers` is deliberately
/// not modelled: no Fynance code reads it.
#[derive(Debug, Deserialize)]
struct ToolOutput {
    schema_version: u32,
    #[allow(dead_code)]
    #[serde(default)]
    page_count: Option<u32>,
    pages: Vec<ToolPage>,
}

#[derive(Debug, Deserialize)]
struct ToolPage {
    page: u32,
    #[serde(default)]
    text: String,
    has_text_layer: Option<bool>,
    image: Option<ToolImage>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ToolImage {
    path: String,
}

// `pub(crate)` so the provider tests (plan 30 Part 2) can drive the fake
// tool; everything in here stays test-only.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Mutex;

    /// Serialises the two tests that mutate `FYNANCE_PDF_EXTRACT_*`, so their
    /// `set_var` calls cannot interleave with each other's env reads. Tests
    /// that pass an explicit `PdfExtractConfig` never touch env and stay
    /// parallel-safe.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("pdf_extract")
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        fs::read(fixtures_dir().join(name)).expect("fixture present")
    }

    pub(crate) fn sample_pdf_bytes() -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("synthetic_statement.pdf"),
        )
        .expect("synthetic_statement.pdf present")
    }

    /// What the fake tool does when run. Scripts bake in literal paths, so
    /// tests need no env and can run in parallel.
    pub(crate) enum Behaviour {
        /// Copy the fixture manifest and PNGs into `--out`.
        Ok,
        /// stderr + exit 3.
        Fail,
        /// extract.json containing `not json`, exit 0.
        Garbage,
        /// Manifest plus only two of the three PNGs.
        MissingPng,
        /// Manifest with `pages[0].image.path = "../x.png"`.
        EscapingPath,
        /// Sleep well past any test timeout.
        Slow,
        /// Write argv one per line to `dir/argv.txt`, then behave like Ok.
        EchoArgs,
        /// Manifest with page 2 carrying an `error`, exit 10.
        PartialFail,
        /// Manifest renumbered 2,3,4 (simulating `--pages 2-4`), exit 0.
        NumberedFrom2,
        /// Manifest with page numbers 1,2,2.
        DuplicatePages,
        /// Manifest with page numbers 2,1,3.
        DescendingPages,
        /// Two pages with `has_text_layer` omitted and 19 / 20 non-whitespace
        /// chars of text each.
        FlagThreshold,
        /// Two pages with `has_text_layer` omitted and 25 whitespace
        /// characters each.
        WhitespaceText,
    }

    fn variant_manifest(
        fixture: &serde_json::Value,
        behaviour: &Behaviour,
    ) -> Option<serde_json::Value> {
        let mut doc = fixture.clone();
        let pages = doc["pages"].as_array_mut().unwrap();
        match behaviour {
            Behaviour::EscapingPath => {
                pages[0]["image"]["path"] = serde_json::json!("../x.png");
            }
            Behaviour::PartialFail => {
                pages[1]["error"] = serde_json::json!("render failed");
            }
            Behaviour::NumberedFrom2 => {
                for (i, page) in pages.iter_mut().enumerate() {
                    page["page"] = serde_json::json!(i as u32 + 2);
                }
            }
            Behaviour::DuplicatePages => {
                pages[2]["page"] = serde_json::json!(2);
            }
            Behaviour::DescendingPages => {
                pages[0]["page"] = serde_json::json!(2);
                pages[1]["page"] = serde_json::json!(1);
            }
            Behaviour::FlagThreshold => {
                *pages = serde_json::from_str(
                    r#"[{"page":1,"text":"aaaaaaaaaaaaaaaaaaa","image":{"path":"page-001.png"}},
                       {"page":2,"text":"aaaaaaaaaaaaaaaaaaaa","image":{"path":"page-002.png"}}]"#,
                )
                .unwrap();
            }
            Behaviour::WhitespaceText => {
                *pages = serde_json::from_str(
                    r#"[{"page":1,"text":"                         ","image":{"path":"page-001.png"}},
                       {"page":2,"text":"a a a a a a a a a a a","image":{"path":"page-002.png"}}]"#,
                )
                .unwrap();
            }
            _ => return None,
        }
        Some(doc)
    }

    /// Write a fake pdf-extract tool (an `sh` script on Unix, a `.cmd` batch
    /// file on Windows) into `dir`, plus any variant manifest JSON it needs.
    /// Returns the path to the tool.
    pub(crate) fn write_fake_tool(dir: &Path, behaviour: Behaviour) -> PathBuf {
        let fix = fixtures_dir().display().to_string();
        let variant_path = dir.join("manifest_variant.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&fixture_bytes("extract.json")).unwrap();
        let variant = variant_manifest(&fixture, &behaviour);
        let manifest_src = match &variant {
            Some(v) => {
                fs::write(&variant_path, serde_json::to_vec_pretty(v).unwrap()).unwrap();
                variant_path.display().to_string()
            }
            None => fix.clone(),
        };

        let copy_manifest = if variant.is_some() {
            format!(r#""{manifest_src}""#)
        } else {
            format!(r#""{fix}\extract.json""#)
        };

        let script = if cfg!(windows) {
            let copy_pngs = |names: &str| format!(r#"copy /Y "{fix}\{names}" "%OUT%" >nul"#);
            let mut lines = vec![
                "@echo off".to_string(),
                "setlocal".to_string(),
                r#"set "OUT=%~3""#.to_string(),
                r#"if not exist "%OUT%" mkdir "%OUT%""#.to_string(),
            ];
            match behaviour {
                Behaviour::Ok => {
                    lines.push(format!("copy /Y \"{fix}\\extract.json\" \"%OUT%\" >nul"));
                    lines.push(copy_pngs("page-00*.png"));
                }
                Behaviour::Fail => {
                    lines.push("echo pdf-extract: error: cannot open input file 1>&2".to_string());
                    lines.push("exit /b 3".to_string());
                }
                Behaviour::Garbage => {
                    lines.push(r#"> "%OUT%\extract.json" echo not json"#.to_string());
                }
                Behaviour::MissingPng => {
                    lines.push(format!("copy /Y \"{fix}\\extract.json\" \"%OUT%\" >nul"));
                    lines.push(copy_pngs("page-001.png"));
                    lines.push(copy_pngs("page-003.png"));
                }
                Behaviour::EscapingPath
                | Behaviour::PartialFail
                | Behaviour::NumberedFrom2
                | Behaviour::DuplicatePages
                | Behaviour::DescendingPages => {
                    lines.push(format!(
                        "copy /Y {copy_manifest} \"%OUT%\\extract.json\" >nul"
                    ));
                    lines.push(copy_pngs("page-00*.png"));
                }
                Behaviour::Slow => {
                    lines.push("ping -n 31 127.0.0.1 >nul".to_string());
                }
                Behaviour::EchoArgs => {
                    let argv = dir.join("argv.txt").display().to_string();
                    lines.push(format!(r#"> "{argv}" echo %~1"#));
                    lines.push(format!(r#">> "{argv}" echo %~2"#));
                    lines.push(format!(r#">> "{argv}" echo %~3"#));
                    lines.push(format!(r#">> "{argv}" echo %~4"#));
                    lines.push(format!(r#">> "{argv}" echo %~5"#));
                    lines.push(format!("copy /Y \"{fix}\\extract.json\" \"%OUT%\" >nul"));
                    lines.push(copy_pngs("page-00*.png"));
                }
                Behaviour::FlagThreshold | Behaviour::WhitespaceText => {
                    lines.push(format!(
                        "copy /Y {copy_manifest} \"%OUT%\\extract.json\" >nul"
                    ));
                    lines.push(copy_pngs("page-001.png"));
                    lines.push(copy_pngs("page-002.png"));
                }
            }
            lines.push("exit /b 0".to_string());
            lines.join("\r\n")
        } else {
            let mut lines = vec![
                "#!/bin/sh".to_string(),
                r#"OUT="$3""#.to_string(),
                r#"mkdir -p "$OUT""#.to_string(),
            ];
            match behaviour {
                Behaviour::Ok => {
                    lines.push(format!(r#"cp '{fix}/extract.json' "$OUT/""#));
                    lines.push(format!(r#"cp '{fix}'/page-00*.png "$OUT/""#));
                }
                Behaviour::Fail => {
                    lines.push("echo 'pdf-extract: error: cannot open input file' >&2".to_string());
                    lines.push("exit 3".to_string());
                }
                Behaviour::Garbage => {
                    lines.push("printf 'not json' > \"$OUT/extract.json\"".to_string());
                }
                Behaviour::MissingPng => {
                    lines.push(format!(r#"cp '{fix}/extract.json' "$OUT/""#));
                    lines.push(format!(
                        r#"cp '{fix}/page-001.png' '{fix}/page-003.png' "$OUT/""#
                    ));
                }
                Behaviour::EscapingPath
                | Behaviour::PartialFail
                | Behaviour::NumberedFrom2
                | Behaviour::DuplicatePages
                | Behaviour::DescendingPages => {
                    lines.push(format!(r#"cp '{manifest_src}' "$OUT/extract.json""#));
                    lines.push(format!(r#"cp '{fix}'/page-00*.png "$OUT/""#));
                }
                Behaviour::Slow => lines.push("sleep 30".to_string()),
                Behaviour::EchoArgs => {
                    let argv = dir.join("argv.txt").display().to_string();
                    lines.push(format!(
                        r#"for a in "$@"; do printf '%s\n' "$a"; done > '{argv}'"#
                    ));
                    lines.push(format!(r#"cp '{fix}/extract.json' "$OUT/""#));
                    lines.push(format!(r#"cp '{fix}'/page-00*.png "$OUT/""#));
                }
                Behaviour::FlagThreshold | Behaviour::WhitespaceText => {
                    lines.push(format!(r#"cp '{manifest_src}' "$OUT/extract.json""#));
                    lines.push(format!(
                        r#"cp '{fix}/page-001.png' '{fix}/page-002.png' "$OUT/""#
                    ));
                }
            }
            lines.push("exit 0".to_string());
            lines.join("\n")
        };

        let name = if cfg!(windows) {
            "fake_tool.cmd"
        } else {
            "fake_tool.sh"
        };
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn config_for(cmd: PathBuf, timeout: Duration) -> PdfExtractConfig {
        PdfExtractConfig {
            cmd: Some(cmd),
            extra_args: Vec::new(),
            timeout,
        }
    }

    /// Run `f` with `vars` (a `None` value unsets) in the environment, then
    /// restore the original values.
    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| ((*k).to_string(), std::env::var(*k).ok()))
            .collect();
        for (k, v) in vars {
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        f();
        for (k, v) in saved {
            unsafe {
                match v {
                    Some(v) => std::env::set_var(&k, v),
                    None => std::env::remove_var(&k),
                }
            }
        }
    }

    fn fake_config(dir: &Path, behaviour: Behaviour, timeout: Duration) -> PdfExtractConfig {
        config_for(write_fake_tool(dir, behaviour), timeout)
    }

    #[tokio::test]
    async fn ok_returns_three_pages() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::Ok, Duration::from_secs(30));
        let out = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect("Ok fake should succeed");
        assert_eq!(out.pages.len(), 3);
        assert_eq!(out.page_errors, vec![]);
        let numbers: Vec<u32> = out.pages.iter().map(|p| p.page).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
        assert!(out.pages[0].text.contains("Corner Shop"));
        assert_eq!(out.pages[2].text, "");
        assert!(!out.pages[2].has_text_layer);
        let fixture_pngs = [
            fixture_bytes("page-001.png"),
            fixture_bytes("page-002.png"),
            fixture_bytes("page-003.png"),
        ];
        for (page, fixture) in out.pages.iter().zip(fixture_pngs) {
            assert!(page.png.starts_with(PNG_SIGNATURE));
            assert_eq!(page.png, fixture);
        }
    }

    #[tokio::test]
    async fn extra_args_are_passed_through_and_temp_dir_is_cleaned_up() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut cfg = fake_config(dir.path(), Behaviour::EchoArgs, Duration::from_secs(30));
        cfg.extra_args = vec!["--dpi".to_string(), "150".to_string()];
        let out = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect("EchoArgs fake should succeed");
        assert_eq!(out.pages.len(), 3);

        let argv_raw = fs::read_to_string(dir.path().join("argv.txt")).unwrap();
        let argv: Vec<String> = argv_raw
            .lines()
            .map(|l| l.trim_end().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(argv.len(), 5, "argv: {argv:?}");
        let input = Path::new(&argv[0]);
        assert_eq!(input.file_name().unwrap(), INPUT_FILE);
        assert_eq!(argv[1], "--out");
        let out_dir = Path::new(&argv[2]);
        assert_eq!(out_dir, input.parent().unwrap().join(OUT_DIR));
        assert_eq!(argv[3], "--dpi");
        assert_eq!(argv[4], "150");

        assert!(!out_dir.exists(), "temp dir must be removed after the call");
    }

    #[tokio::test]
    async fn missing_cmd_gives_not_configured() {
        let cfg = PdfExtractConfig {
            cmd: None,
            extra_args: Vec::new(),
            timeout: Duration::from_secs(5),
        };
        let err = extract_pdf(b"%PDF-1.4 fake", "x.pdf", &cfg)
            .await
            .expect_err("no cmd must fail");
        match err {
            PdfExtractError::NotConfigured => {}
            other => panic!("expected NotConfigured, got {other:?}"),
        }
        assert!(err.to_string().contains("FYNANCE_PDF_EXTRACT_CMD"));
    }

    #[tokio::test]
    async fn nonexistent_cmd_gives_not_found() {
        let cfg = config_for(fixtures_dir().join("no_such_tool"), Duration::from_secs(5));
        let err = extract_pdf(b"%PDF-1.4 fake", "x.pdf", &cfg)
            .await
            .expect_err("missing tool must fail");
        match err {
            PdfExtractError::NotFound { .. } => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
        assert!(err.to_string().contains("FYNANCE_PDF_EXTRACT_CMD"));
    }

    #[tokio::test]
    async fn failing_tool_gives_failed_with_stderr() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::Fail, Duration::from_secs(30));
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("Fail fake must fail");
        match &err {
            PdfExtractError::Failed { status, .. } => assert_eq!(*status, 3),
            other => panic!("expected Failed, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("cannot open input file"), "message: {msg}");
        assert!(
            msg.contains("could not find the input file"),
            "message: {msg}"
        );
    }

    #[tokio::test]
    async fn garbage_output_gives_bad_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::Garbage, Duration::from_secs(30));
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("garbage must fail");
        assert!(matches!(err, PdfExtractError::BadOutput(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_png_gives_bad_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::MissingPng, Duration::from_secs(30));
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("missing png must fail");
        assert!(matches!(err, PdfExtractError::BadOutput(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn escaping_image_path_gives_bad_output_without_reading_outside_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::EscapingPath, Duration::from_secs(30));
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("escaping path must fail");
        assert!(matches!(err, PdfExtractError::BadOutput(_)), "got {err:?}");
        assert!(err.to_string().contains("image.path"), "message: {err}");
    }

    #[tokio::test]
    async fn slow_tool_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::Slow, Duration::from_secs(1));
        let started = Instant::now();
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("Slow fake must time out");
        let elapsed = started.elapsed();
        assert!(
            matches!(err, PdfExtractError::TimedOut { secs: 1 }),
            "got {err:?}"
        );
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }

    #[tokio::test]
    async fn partial_exit_10_returns_good_pages_and_recorded_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(dir.path(), Behaviour::PartialFail, Duration::from_secs(30));
        let out = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect("exit 10 is a partial success");
        let numbers: Vec<u32> = out.pages.iter().map(|p| p.page).collect();
        assert_eq!(numbers, vec![1, 3]);
        assert_eq!(out.page_errors, vec![(2, "render failed".to_string())]);
    }

    #[tokio::test]
    async fn page_numbers_may_start_above_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(
            dir.path(),
            Behaviour::NumberedFrom2,
            Duration::from_secs(30),
        );
        let out = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect("ascending numbers not starting at 1 are valid (--pages)");
        let numbers: Vec<u32> = out.pages.iter().map(|p| p.page).collect();
        assert_eq!(numbers, vec![2, 3, 4]);
    }

    #[tokio::test]
    async fn duplicate_page_numbers_give_bad_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(
            dir.path(),
            Behaviour::DuplicatePages,
            Duration::from_secs(30),
        );
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("duplicate numbers must fail");
        assert!(matches!(err, PdfExtractError::BadOutput(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn out_of_order_page_numbers_give_bad_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(
            dir.path(),
            Behaviour::DescendingPages,
            Duration::from_secs(30),
        );
        let err = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect_err("out-of-order numbers must fail");
        assert!(matches!(err, PdfExtractError::BadOutput(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_has_text_layer_flag_derives_from_text_length() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(
            dir.path(),
            Behaviour::FlagThreshold,
            Duration::from_secs(30),
        );
        let out = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect("manifest is valid");
        assert!(
            !out.pages[0].has_text_layer,
            "19 non-whitespace chars is below the threshold"
        );
        assert!(
            out.pages[1].has_text_layer,
            "20 non-whitespace chars meets it"
        );
    }

    #[tokio::test]
    async fn whitespace_only_text_never_counts_as_a_text_layer() {
        let dir = tempfile::TempDir::new().unwrap();
        let cfg = fake_config(
            dir.path(),
            Behaviour::WhitespaceText,
            Duration::from_secs(30),
        );
        let out = extract_pdf(&sample_pdf_bytes(), "statement.pdf", &cfg)
            .await
            .expect("manifest is valid");
        assert!(
            !out.pages[0].has_text_layer,
            "25 spaces contain no non-whitespace chars"
        );
        assert!(
            !out.pages[1].has_text_layer,
            "interleaved letters are under 20"
        );
    }

    #[test]
    fn from_env_rejects_non_array_args() {
        with_env(
            &[
                ("FYNANCE_PDF_EXTRACT_CMD", Some("pdf-extract")),
                ("FYNANCE_PDF_EXTRACT_ARGS", Some(r#"{"a":1}"#)),
                ("FYNANCE_PDF_EXTRACT_TIMEOUT_SECS", None),
            ],
            || {
                assert!(PdfExtractConfig::from_env().is_err());
            },
        );
    }

    #[test]
    fn from_env_rejects_non_numeric_timeout() {
        with_env(
            &[
                ("FYNANCE_PDF_EXTRACT_CMD", Some("pdf-extract")),
                ("FYNANCE_PDF_EXTRACT_ARGS", None),
                ("FYNANCE_PDF_EXTRACT_TIMEOUT_SECS", Some("soon")),
            ],
            || {
                assert!(PdfExtractConfig::from_env().is_err());
            },
        );
    }

    #[test]
    fn from_env_reads_cmd_args_and_timeout() {
        with_env(
            &[
                (
                    "FYNANCE_PDF_EXTRACT_CMD",
                    Some("/usr/local/bin/pdf-extract"),
                ),
                ("FYNANCE_PDF_EXTRACT_ARGS", Some(r#"["--dpi","150"]"#)),
                ("FYNANCE_PDF_EXTRACT_TIMEOUT_SECS", Some("45")),
            ],
            || {
                let cfg = PdfExtractConfig::from_env().unwrap();
                assert_eq!(cfg.cmd, Some(PathBuf::from("/usr/local/bin/pdf-extract")));
                assert_eq!(cfg.extra_args, vec!["--dpi".to_string(), "150".to_string()]);
                assert_eq!(cfg.timeout, Duration::from_secs(45));
            },
        );
    }

    // ------------------------------------------------------------------------
    // Synthetic PDF generator + committed-fixture drift guard.
    // ------------------------------------------------------------------------

    fn uncompressed_pdf() -> Vec<u8> {
        struct Obj {
            num: u32,
            body: Vec<u8>,
        }
        let pages: Vec<Vec<u8>> = vec![
            br#"BT /F1 11 Tf 60 740 Td (Example Bank - Monthly Statement - November 2026) Tj ET
BT /F1 10 Tf 60 710 Td (Date Description Amount) Tj ET
BT /F1 10 Tf 60 690 Td (2026-11-02 Corner Shop (12.00)) Tj ET
BT /F1 10 Tf 60 672 Td (2026-11-03 Coffee Cart (3.75)) Tj ET
BT /F1 10 Tf 60 654 Td (2026-11-05 Salary ACME Ltd 2,450.00) Tj ET
BT /F1 10 Tf 60 636 Td (2026-11-07 Corner Shop (8.00)) Tj ET
BT /F1 10 Tf 60 618 Td (2026-11-08 Utility Bill (60.00)) Tj ET
BT /F1 10 Tf 60 600 Td (2026-11-09 Coffee Cart (4.25)) Tj ET"#
                .to_vec(),
            br#"BT /F1 10 Tf 60 700 Td (2026-11-02 Bus Top Up (5.00)) Tj ET
BT /F1 10 Tf 60 682 Td (2026-11-05 Corner Shop (14.20)) Tj ET
BT /F1 10 Tf 60 664 Td (2026-11-09 Bus Top Up (5.00)) Tj ET
BT /F1 10 Tf 60 646 Td (2026-11-10 Coffee Cart (3.10)) Tj ET
BT /F1 10 Tf 60 628 Td (2026-11-11 Book Shop (19.99)) Tj ET
BT /F1 10 Tf 60 610 Td (2026-11-12 Salary ACME Ltd 2,450.00) Tj ET"#
                .to_vec(),
            br#"q 0.6 0.6 0.6 rg 60 400 300 250 re f Q"#.to_vec(),
        ];

        let image_stream = b"\x00\x80\xff\x00\x80\xff\x00\x80\xff\x00\x80\xff\x00\x80\xff";

        let objects: Vec<Obj> = vec![
            Obj {
                num: 1,
                body: b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            },
            Obj {
                num: 2,
                body: b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_vec(),
            },
            Obj {
                num: 3,
                body: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 9 0 R >> >> /Contents 6 0 R >>".to_vec(),
            },
            Obj {
                num: 4,
                body: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 9 0 R >> >> /Contents 7 0 R >>".to_vec(),
            },
            Obj {
                num: 5,
                body: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im1 10 0 R >> >> /Contents 8 0 R >>".to_vec(),
            },
            Obj {
                num: 6,
                body: format_stream(&pages[0]),
            },
            Obj {
                num: 7,
                body: format_stream(&pages[1]),
            },
            Obj {
                num: 8,
                body: format_stream(&pages[2]),
            },
            Obj {
                num: 9,
                body: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
            },
            Obj {
                num: 10,
                body: format_stream(image_stream),
            },
        ];

        let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let mut offsets = BTreeMap::new();
        for obj in &objects {
            offsets.insert(obj.num, out.len() as u32);
            out.extend_from_slice(format!("{} 0 obj\n", obj.num).as_bytes());
            out.extend_from_slice(&obj.body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = out.len() as u32;
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for num in 1..=(objects.len() as u32) {
            out.extend_from_slice(format!("{:010} 00000 n \n", offsets[&num]).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn format_stream(data: &[u8]) -> Vec<u8> {
        let mut body = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        body
    }

    /// The committed fixture must equal the generator's output. Regenerate
    /// with the ignored test below if the generator changes.
    #[test]
    fn synthetic_statement_fixture_matches_generator() {
        let committed = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("synthetic_statement.pdf"),
        )
        .unwrap();
        assert_eq!(committed, uncompressed_pdf());
    }

    #[test]
    #[ignore]
    fn regenerate_synthetic_statement_fixture() {
        fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("synthetic_statement.pdf"),
            uncompressed_pdf(),
        )
        .unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn smoke_against_real_tool() {
        // Manual only: point FYNANCE_PDF_EXTRACT_CMD at the real pdf-extract
        // tool and run `cargo test -- --ignored`. Needs the tool installed;
        // never runs in CI.
        let cfg = PdfExtractConfig::from_env().unwrap();
        let out = extract_pdf(&sample_pdf_bytes(), "synthetic_statement.pdf", &cfg)
            .await
            .expect("real tool run");
        for page in &out.pages {
            println!(
                "page {}: text {} chars, png {} bytes",
                page.page,
                page.text.len(),
                page.png.len()
            );
        }
        println!("page errors: {:?}", out.page_errors);
    }
}
