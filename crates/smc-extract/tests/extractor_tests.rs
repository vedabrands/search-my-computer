use smc_extract::extractor::{ExtractionLimits, ExtractorRegistry};
use std::io::Write;
use tempfile::NamedTempFile;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

#[test]
fn test_extract_plain_text_and_markdown() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    // 1. Plain text file
    let mut file = NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(
        file,
        "Hello, this is a plain text document.\nIt has two lines."
    )
    .unwrap();
    let doc = registry.extract_file(file.path(), &limits).unwrap();

    assert_eq!(doc.title, None);
    assert!(!doc.blocks.is_empty());
    assert!(doc.full_text().contains("plain text document"));
    assert_eq!(doc.metadata.get("line_count").unwrap(), "2");

    // 2. Markdown file with title
    let mut md_file = NamedTempFile::with_suffix(".md").unwrap();
    writeln!(md_file, "# The Architecture Guide\n\nThis is a section about components.\n\n## Subheading\nDetails here.").unwrap();
    let md_doc = registry.extract_file(md_file.path(), &limits).unwrap();

    assert_eq!(md_doc.title, Some("The Architecture Guide".to_string()));
    assert!(md_doc.full_text().contains("components"));
}

#[test]
fn test_extract_json_and_csv() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    // JSON file
    let mut json_file = NamedTempFile::with_suffix(".json").unwrap();
    writeln!(
        json_file,
        r#"{{"name": "SearchMyComputer", "version": "0.1.0"}}"#
    )
    .unwrap();
    let json_doc = registry.extract_file(json_file.path(), &limits).unwrap();
    assert!(json_doc.full_text().contains("SearchMyComputer"));

    // CSV file
    let mut csv_file = NamedTempFile::with_suffix(".csv").unwrap();
    writeln!(
        csv_file,
        "name,role,department\nAlice,Engineer,Core\nBob,Designer,UI"
    )
    .unwrap();
    let csv_doc = registry.extract_file(csv_file.path(), &limits).unwrap();
    assert!(csv_doc.full_text().contains("Alice"));
    assert!(csv_doc.full_text().contains("Engineer"));
}

#[test]
fn test_extract_code_files() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let mut rs_file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        rs_file,
        "pub fn calculate_score(a: i32, b: i32) -> i32 {{\n    a + b\n}}"
    )
    .unwrap();
    let rs_doc = registry.extract_file(rs_file.path(), &limits).unwrap();

    assert_eq!(rs_doc.language_guess, Some("rust".to_string()));
    assert!(rs_doc.full_text().contains("calculate_score"));

    let mut py_file = NamedTempFile::with_suffix(".py").unwrap();
    writeln!(
        py_file,
        "def process_data(records):\n    return [r.strip() for r in records]"
    )
    .unwrap();
    let py_doc = registry.extract_file(py_file.path(), &limits).unwrap();

    assert_eq!(py_doc.language_guess, Some("python".to_string()));
    assert!(py_doc.full_text().contains("process_data"));
}

#[test]
fn test_extract_docx_file() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let mut temp = NamedTempFile::with_suffix(".docx").unwrap();
    {
        let mut zip = ZipWriter::new(&mut temp);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // Core properties
        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                               xmlns:dc="http://purl.org/dc/elements/1.1/">
                <dc:title>Quarterly Business Report</dc:title>
                <dc:creator>John Doe</dc:creator>
            </cp:coreProperties>"#,
        ).unwrap();

        // Main Document body
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body>
                    <w:p>
                        <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
                        <w:r><w:t>Executive Summary</w:t></w:r>
                    </w:p>
                    <w:p>
                        <w:r><w:t>Revenue grew by 25% in Q3 due to product adoption.</w:t></w:r>
                    </w:p>
                </w:body>
            </w:document>"#,
        )
        .unwrap();

        zip.finish().unwrap();
    }

    let doc = registry.extract_file(temp.path(), &limits).unwrap();
    assert_eq!(doc.title, Some("Quarterly Business Report".to_string()));
    assert_eq!(
        doc.metadata.get("author").map(|s| s.as_str()),
        Some("John Doe")
    );
    assert!(doc.full_text().contains("Executive Summary"));
    assert!(doc.full_text().contains("Revenue grew by 25%"));
    assert_eq!(doc.blocks.len(), 2);
}

#[test]
fn test_extract_pptx_file() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let mut temp = NamedTempFile::with_suffix(".pptx").unwrap();
    {
        let mut zip = ZipWriter::new(&mut temp);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                               xmlns:dc="http://purl.org/dc/elements/1.1/">
                <dc:title>AI Strategy Presentation</dc:title>
            </cp:coreProperties>"#,
        ).unwrap();

        // Slide 1
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
                   xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
                <p:cSld>
                    <p:spTree>
                        <p:sp>
                            <p:txBody>
                                <a:p><a:r><a:t>Local-First Semantic Search</a:t></a:r></a:p>
                            </p:txBody>
                        </p:sp>
                    </p:spTree>
                </p:cSld>
            </p:sld>"#,
        )
        .unwrap();

        // Slide 2
        zip.start_file("ppt/slides/slide2.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
                   xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
                <p:cSld>
                    <p:spTree>
                        <p:sp>
                            <p:txBody>
                                <a:p><a:r><a:t>Zero Network Calls and Complete Privacy</a:t></a:r></a:p>
                            </p:txBody>
                        </p:sp>
                    </p:spTree>
                </p:cSld>
            </p:sld>"#,
        ).unwrap();

        zip.finish().unwrap();
    }

    let doc = registry.extract_file(temp.path(), &limits).unwrap();
    assert_eq!(doc.title, Some("AI Strategy Presentation".to_string()));
    assert_eq!(
        doc.metadata.get("slide_count").map(|s| s.as_str()),
        Some("2")
    );
    assert_eq!(doc.blocks.len(), 2);
    assert_eq!(doc.blocks[0].page, Some(1));
    assert_eq!(doc.blocks[0].section, Some("Slide 1".to_string()));
    assert!(doc.blocks[0].text.contains("Local-First"));
    assert_eq!(doc.blocks[1].page, Some(2));
    assert_eq!(doc.blocks[1].section, Some("Slide 2".to_string()));
    assert!(doc.blocks[1].text.contains("Zero Network Calls"));
}

#[test]
fn test_extract_xlsx_file() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let mut temp = NamedTempFile::with_suffix(".xlsx").unwrap();
    {
        let mut zip = ZipWriter::new(&mut temp);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        // Content Types
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
                <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
                <Default Extension="xml" ContentType="application/xml"/>
                <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
                <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
                <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
            </Types>"#,
        ).unwrap();

        // Package Rels
        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
            </Relationships>"#,
        ).unwrap();

        // Workbook Rels
        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
                <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
            </Relationships>"#,
        ).unwrap();

        // Workbook
        zip.start_file("xl/workbook.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
                      xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
                <sheets>
                    <sheet name="Financials" sheetId="1" r:id="rId1"/>
                </sheets>
            </workbook>"#,
        )
        .unwrap();

        // Shared Strings
        zip.start_file("xl/sharedStrings.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">
                <si><t>Revenue</t></si>
                <si><t>Expenses</t></si>
                <si><t>Profit</t></si>
            </sst>"#,
        ).unwrap();

        // Sheet 1
        zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                <sheetData>
                    <row r="1">
                        <c r="A1" t="s"><v>0</v></c>
                        <c r="B1"><v>100000</v></c>
                    </row>
                    <row r="2">
                        <c r="A2" t="s"><v>1</v></c>
                        <c r="B2"><v>60000</v></c>
                    </row>
                    <row r="3">
                        <c r="A3" t="s"><v>2</v></c>
                        <c r="B3"><v>40000</v></c>
                    </row>
                </sheetData>
            </worksheet>"#,
        )
        .unwrap();

        zip.finish().unwrap();
    }

    let doc = registry.extract_file(temp.path(), &limits).unwrap();
    assert_eq!(
        doc.metadata.get("sheet_count").map(|s| s.as_str()),
        Some("1")
    );
    assert!(doc.full_text().contains("Revenue"));
    assert!(doc.full_text().contains("100000"));
    assert!(doc.full_text().contains("Profit"));
}
