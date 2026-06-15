use anyhow::{Result, anyhow};
use calamine::{Reader, Xlsx, open_workbook_from_rs};
use std::io::Cursor;

use super::document_parser::{DocumentInput, FileFormat};

pub fn detect_format(filename: &str, bytes: &[u8]) -> FileFormat {
    let ext = filename.rsplit('.').next().map(|e| e.to_ascii_lowercase());

    match ext.as_deref() {
        Some("csv") | Some("tsv") => FileFormat::Csv,
        Some("pdf") => FileFormat::Pdf,
        Some("xlsx") | Some("xls") => FileFormat::Excel,
        _ => detect_from_magic_bytes(bytes),
    }
}

fn detect_from_magic_bytes(bytes: &[u8]) -> FileFormat {
    if bytes.len() >= 4 && &bytes[0..4] == b"%PDF" {
        return FileFormat::Pdf;
    }
    if bytes.len() >= 2 && &bytes[0..2] == b"PK" {
        return FileFormat::Excel;
    }
    FileFormat::Csv
}

pub fn preprocess_file(filename: &str, bytes: Vec<u8>) -> Result<DocumentInput> {
    let format = detect_format(filename, &bytes);
    let original_size = bytes.len();

    match format {
        FileFormat::Csv => {
            let text_content = String::from_utf8(bytes)
                .map_err(|_| anyhow!("file '{}' is not valid UTF-8 (expected CSV)", filename))?;
            Ok(DocumentInput {
                filename: filename.to_string(),
                format: FileFormat::Csv,
                text_content,
                raw_bytes: vec![],
                original_size,
            })
        }
        FileFormat::Excel => {
            let text_content = excel_to_csv_text(filename, &bytes)?;
            Ok(DocumentInput {
                filename: filename.to_string(),
                format: FileFormat::Excel,
                text_content,
                raw_bytes: vec![],
                original_size,
            })
        }
        FileFormat::Pdf => {
            validate_pdf(filename, &bytes)?;
            Ok(DocumentInput {
                filename: filename.to_string(),
                format: FileFormat::Pdf,
                text_content: String::new(),
                raw_bytes: bytes,
                original_size,
            })
        }
    }
}

fn excel_to_csv_text(filename: &str, bytes: &[u8]) -> Result<String> {
    let cursor = Cursor::new(bytes);
    let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor)
        .map_err(|e| anyhow!("failed to open Excel file '{}': {}", filename, e))?;

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(anyhow!("Excel file '{}' has no sheets", filename));
    }

    let first_sheet = &sheet_names[0];
    let range = workbook.worksheet_range(first_sheet).map_err(|e| {
        anyhow!(
            "failed to read sheet '{}' in '{}': {}",
            first_sheet,
            filename,
            e
        )
    })?;

    let mut csv_output = String::new();
    for row in range.rows() {
        let cells: Vec<String> = row
            .iter()
            .map(|cell| {
                let s = cell.to_string();
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s
                }
            })
            .collect();
        csv_output.push_str(&cells.join(","));
        csv_output.push('\n');
    }

    if csv_output.is_empty() {
        return Err(anyhow!(
            "Excel file '{}' sheet '{}' is empty",
            filename,
            first_sheet
        ));
    }

    if csv_output.len() > 200_000 {
        tracing::warn!(
            filename,
            sheet = first_sheet,
            bytes = csv_output.len(),
            "Excel sheet converted to CSV exceeds 200KB; truncating"
        );
        csv_output.truncate(200_000);
    }

    Ok(csv_output)
}

fn validate_pdf(filename: &str, bytes: &[u8]) -> Result<()> {
    if bytes.len() < 5 || &bytes[0..4] != b"%PDF" {
        return Err(anyhow!(
            "file '{}' does not appear to be a valid PDF (missing %PDF header)",
            filename
        ));
    }

    let content = String::from_utf8_lossy(bytes);
    let page_count = content
        .matches("/Type /Page")
        .count()
        .max(content.matches("/Type/Page").count());

    // Claude's document API accepts up to 100 pages per PDF; reject only past
    // that hard limit. Output-side truncation (the more common failure for
    // dense statements) is detected separately after the model call.
    const MAX_PDF_PAGES: usize = 100;
    if page_count > MAX_PDF_PAGES {
        return Err(anyhow!(
            "PDF '{}' has approximately {} pages, which exceeds the {}-page per-document \
             limit. Please split it into smaller files (e.g. a few months at a time) and \
             try again.",
            filename,
            page_count,
            MAX_PDF_PAGES
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_csv_by_extension() {
        assert_eq!(
            detect_format("transactions.csv", b"Date,Amount"),
            FileFormat::Csv
        );
        assert_eq!(detect_format("data.tsv", b"Date\tAmount"), FileFormat::Csv);
    }

    #[test]
    fn test_detect_pdf_by_extension() {
        assert_eq!(detect_format("statement.pdf", b"%PDF-1.5"), FileFormat::Pdf);
    }

    #[test]
    fn test_detect_excel_by_extension() {
        assert_eq!(
            detect_format("positions.xlsx", b"PK\x03\x04"),
            FileFormat::Excel
        );
        assert_eq!(
            detect_format("data.xls", b"\xd0\xcf\x11"),
            FileFormat::Excel
        );
    }

    #[test]
    fn test_detect_pdf_by_magic_bytes() {
        assert_eq!(
            detect_format("unknown_file", b"%PDF-1.7 rest of content"),
            FileFormat::Pdf
        );
    }

    #[test]
    fn test_detect_xlsx_by_magic_bytes() {
        assert_eq!(
            detect_format("no_extension", b"PK\x03\x04 zip content"),
            FileFormat::Excel
        );
    }

    #[test]
    fn test_fallback_to_csv() {
        assert_eq!(
            detect_format("data.txt", b"col1,col2\nval1,val2"),
            FileFormat::Csv
        );
        assert_eq!(detect_format("unknown", b"some text"), FileFormat::Csv);
    }

    #[test]
    fn test_preprocess_csv_valid_utf8() {
        let bytes = b"Date,Amount\n2025-01-01,-5.50".to_vec();
        let doc = preprocess_file("test.csv", bytes).unwrap();
        assert_eq!(doc.format, FileFormat::Csv);
        assert_eq!(doc.text_content, "Date,Amount\n2025-01-01,-5.50");
        assert!(doc.raw_bytes.is_empty());
    }

    #[test]
    fn test_preprocess_csv_invalid_utf8() {
        let bytes = vec![0xFF, 0xFE, 0x00, 0x01];
        let result = preprocess_file("bad.csv", bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn test_validate_pdf_missing_header() {
        let result = validate_pdf("fake.pdf", b"not a pdf");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing %PDF header")
        );
    }

    #[test]
    fn test_validate_pdf_too_many_pages() {
        let mut content = b"%PDF-1.5\n".to_vec();
        for _ in 0..105 {
            content.extend_from_slice(b"/Type /Page\n");
        }
        let result = validate_pdf("huge.pdf", &content);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("100-page"));
    }

    #[test]
    fn test_validate_pdf_valid() {
        let mut content = b"%PDF-1.5\n".to_vec();
        for _ in 0..5 {
            content.extend_from_slice(b"/Type /Page\n");
        }
        let result = validate_pdf("ok.pdf", &content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_preprocess_pdf_stores_raw_bytes() {
        let mut content = b"%PDF-1.5\n".to_vec();
        for _ in 0..2 {
            content.extend_from_slice(b"/Type /Page\n");
        }
        let original_len = content.len();
        let doc = preprocess_file("statement.pdf", content).unwrap();
        assert_eq!(doc.format, FileFormat::Pdf);
        assert!(doc.text_content.is_empty());
        assert!(!doc.raw_bytes.is_empty());
        assert_eq!(doc.original_size, original_len);
    }

    #[test]
    fn test_preprocess_excel_invalid_file() {
        let bytes = b"PK\x03\x04invalid".to_vec();
        let result = preprocess_file("bad.xlsx", bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_preprocess_excel_real_fixture() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sample_holdings.xlsx");
        let bytes = std::fs::read(&fixture).expect("fixture file must exist");
        let doc = preprocess_file("sample_holdings.xlsx", bytes).unwrap();
        assert_eq!(doc.format, FileFormat::Excel);
        assert!(!doc.text_content.is_empty());
        assert!(doc.raw_bytes.is_empty());
        let lines: Vec<&str> = doc.text_content.lines().collect();
        assert!(
            lines.len() >= 4,
            "expected header + 3 data rows, got {}",
            lines.len()
        );
    }

    #[test]
    fn test_preprocess_pdf_real_fixture() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sample_statement.pdf");
        let bytes = std::fs::read(&fixture).expect("fixture file must exist");
        let doc = preprocess_file("sample_statement.pdf", bytes).unwrap();
        assert_eq!(doc.format, FileFormat::Pdf);
        assert!(doc.text_content.is_empty());
        assert!(!doc.raw_bytes.is_empty());
    }

    #[test]
    fn test_preprocess_pdf_too_many_pages_fixture() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/too_many_pages.pdf");
        let bytes = std::fs::read(&fixture).expect("fixture file must exist");
        let result = preprocess_file("too_many_pages.pdf", bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("100-page"));
    }

    #[test]
    fn test_preprocess_empty_excel_fixture() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/empty_sheet.xlsx");
        let bytes = std::fs::read(&fixture).expect("fixture file must exist");
        let result = preprocess_file("empty_sheet.xlsx", bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }
}
