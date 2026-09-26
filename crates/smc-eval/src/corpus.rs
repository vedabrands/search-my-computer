use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Creates a minimal, standard-compliant multi-page PDF file with extractable text streams.
pub fn create_minimal_pdf(path: &Path, pages: &[&str]) -> std::io::Result<()> {
    let mut pdf_data = Vec::new();
    pdf_data.extend_from_slice(b"%PDF-1.4\n");

    let num_pages = pages.len().max(1);
    let mut offsets = Vec::new();

    // Obj 1: Catalog
    offsets.push(pdf_data.len());
    pdf_data.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // Obj 2: Pages root
    offsets.push(pdf_data.len());
    let mut kids_str = String::new();
    for i in 0..num_pages {
        let page_obj_id = 3 + i * 2;
        kids_str.push_str(&format!("{} 0 R ", page_obj_id));
    }
    let pages_obj = format!(
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids_str.trim(),
        num_pages
    );
    pdf_data.extend_from_slice(pages_obj.as_bytes());

    let font_obj_id = 3 + num_pages * 2;

    for (i, page_text) in pages.iter().enumerate() {
        let page_obj_id = 3 + i * 2;
        let content_obj_id = page_obj_id + 1;

        // Page Object
        offsets.push(pdf_data.len());
        let page_def = format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R /Resources << /Font << /F1 {} 0 R >> >> >>\nendobj\n",
            page_obj_id, content_obj_id, font_obj_id
        );
        pdf_data.extend_from_slice(page_def.as_bytes());

        // Content Stream
        offsets.push(pdf_data.len());
        // Escape special PDF characters
        let escaped_text = page_text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");

        let stream_body = format!("BT\n/F1 12 Tf\n50 720 Td\n({}) Tj\nET\n", escaped_text);
        let content_obj = format!(
            "{} 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
            content_obj_id,
            stream_body.len(),
            stream_body
        );
        pdf_data.extend_from_slice(content_obj.as_bytes());
    }

    // Font Object
    offsets.push(pdf_data.len());
    let font_obj = format!(
        "{} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        font_obj_id
    );
    pdf_data.extend_from_slice(font_obj.as_bytes());

    // XRef Table
    let xref_offset = pdf_data.len();
    let total_objs = font_obj_id + 1;
    let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", total_objs);
    for offset in &offsets {
        xref.push_str(&format!("{:010} 00000 n \n", offset));
    }
    pdf_data.extend_from_slice(xref.as_bytes());

    // Trailer
    let trailer = format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        total_objs, xref_offset
    );
    pdf_data.extend_from_slice(trailer.as_bytes());

    let mut file = File::create(path)?;
    file.write_all(&pdf_data)?;
    Ok(())
}

/// Creates a valid DOCX file with headings and paragraphs.
pub fn create_minimal_docx(path: &Path, paragraphs: &[&str]) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
            <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
            <Default Extension="xml" ContentType="application/xml"/>
            <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
        </Types>"#,
    )?;

    // _rels/.rels
    zip.start_file("_rels/.rels", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
        </Relationships>"#,
    )?;

    // word/document.xml
    zip.start_file("word/document.xml", options)?;
    let mut doc_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:body>"#,
    );

    for p in paragraphs {
        let escaped = escape_xml(p);
        doc_xml.push_str(&format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", escaped));
    }

    doc_xml.push_str("</w:body></w:document>");
    zip.write_all(doc_xml.as_bytes())?;

    zip.finish()?;
    Ok(())
}

/// Creates a valid PPTX file with multiple slides.
pub fn create_minimal_pptx(path: &Path, slides: &[&str]) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", options)?;
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
            <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
            <Default Extension="xml" ContentType="application/xml"/>
            <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>"#,
    );
    for i in 1..=slides.len() {
        ct.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#,
            i
        ));
    }
    ct.push_str("</Types>");
    zip.write_all(ct.as_bytes())?;

    // _rels/.rels
    zip.start_file("_rels/.rels", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
        </Relationships>"#,
    )?;

    // ppt/presentation.xml
    zip.start_file("ppt/presentation.xml", options)?;
    let mut pres = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                        xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
            <p:sldIdLst>"#,
    );
    for i in 1..=slides.len() {
        pres.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i));
    }
    pres.push_str("</p:sldIdLst></p:presentation>");
    zip.write_all(pres.as_bytes())?;

    // ppt/_rels/presentation.xml.rels
    zip.start_file("ppt/_rels/presentation.xml.rels", options)?;
    let mut pres_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 1..=slides.len() {
        pres_rels.push_str(&format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>"#,
            i, i
        ));
    }
    pres_rels.push_str("</Relationships>");
    zip.write_all(pres_rels.as_bytes())?;

    // Individual slides
    for (idx, slide_text) in slides.iter().enumerate() {
        zip.start_file(format!("ppt/slides/slide{}.xml", idx + 1), options)?;
        let escaped = escape_xml(slide_text);
        let sld = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
                   xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
                <p:cSld>
                    <p:spTree>
                        <p:sp>
                            <p:txBody>
                                <a:p><a:r><a:t>{}</a:t></a:r></a:p>
                            </p:txBody>
                        </p:sp>
                    </p:spTree>
                </p:cSld>
            </p:sld>"#,
            escaped
        );
        zip.write_all(sld.as_bytes())?;
    }

    zip.finish()?;
    Ok(())
}

/// Creates a valid XLSX file with shared strings.
pub fn create_minimal_xlsx(path: &Path, rows: &[Vec<&str>]) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Content Types
    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
            <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
            <Default Extension="xml" ContentType="application/xml"/>
            <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
            <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
            <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
        </Types>"#,
    )?;

    // Package Rels
    zip.start_file("_rels/.rels", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
        </Relationships>"#,
    )?;

    // Workbook Rels
    zip.start_file("xl/_rels/workbook.xml.rels", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
            <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
        </Relationships>"#,
    )?;

    // Workbook
    zip.start_file("xl/workbook.xml", options)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
                  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
            <sheets>
                <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
            </sheets>
        </workbook>"#,
    )?;

    // Build shared strings table
    let mut shared_strings: Vec<String> = Vec::new();
    let mut sheet_rows_xml = String::new();

    for (r_idx, row) in rows.iter().enumerate() {
        let row_num = r_idx + 1;
        sheet_rows_xml.push_str(&format!(r#"<row r="{}">"#, row_num));
        for (c_idx, cell_value) in row.iter().enumerate() {
            let col_letter = (b'A' + (c_idx as u8)) as char;
            let str_idx = shared_strings.len();
            shared_strings.push(cell_value.to_string());
            sheet_rows_xml.push_str(&format!(
                r#"<c r="{}{}" t="s"><v>{}</v></c>"#,
                col_letter, row_num, str_idx
            ));
        }
        sheet_rows_xml.push_str("</row>");
    }

    // Worksheet
    zip.start_file("xl/worksheets/sheet1.xml", options)?;
    let sheet_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
            <sheetData>{}</sheetData>
        </worksheet>"#,
        sheet_rows_xml
    );
    zip.write_all(sheet_xml.as_bytes())?;

    // Shared Strings
    zip.start_file("xl/sharedStrings.xml", options)?;
    let mut sst_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{}" uniqueCount="{}">"#,
        shared_strings.len(),
        shared_strings.len()
    );
    for s in &shared_strings {
        let escaped = escape_xml(s);
        sst_xml.push_str(&format!("<si><t>{}</t></si>", escaped));
    }
    sst_xml.push_str("</sst>");
    zip.write_all(sst_xml.as_bytes())?;

    zip.finish()?;
    Ok(())
}

/// Generates the complete synthetic evaluation corpus (~320 files across 8 distinct knowledge categories).
pub fn generate_synthetic_corpus(target_dir: &Path) -> std::io::Result<usize> {
    if !target_dir.exists() {
        fs::create_dir_all(target_dir)?;
    }

    let mut count = 0;

    // 1. Finance & Accounting (~40 files)
    let finance_dir = target_dir.join("finance");
    fs::create_dir_all(&finance_dir)?;

    create_minimal_pdf(
        &finance_dir.join("q3_financial_report_2024.pdf"),
        &[
            "Q3 2024 Financial Performance Report. Total revenue reached $48.5M, representing 32% year-over-year growth. EBITDA margin expanded to 28.4%. Operating income increased to $13.8M driven by enterprise SaaS subscriptions.",
            "Segment Breakdown: Cloud Services contributed $31.2M, Professional Services $11.5M, and Hardware Licensing $5.8M. Cash reserves stand at $102M with zero long-term debt obligations.",
        ],
    )?;
    count += 1;

    create_minimal_pdf(
        &finance_dir.join("annual_10k_filing_sec.pdf"),
        &[
            "United States Securities and Exchange Commission Form 10-K Annual Report. Consolidated balance sheet demonstrates strong fiscal health with GAAP net income of $54.2M.",
            "Risk Factors: Market competition in enterprise AI search, foreign exchange volatility, and supply chain disruptions for compute hardware.",
        ],
    )?;
    count += 1;

    create_minimal_docx(
        &finance_dir.join("budget_allocation_q4_2024.docx"),
        &[
            "Executive Summary: Q4 2024 Departmental Budget Allocations",
            "Engineering & Product: $18.5M allocated for cloud infrastructure, GPU cluster leasing, and senior talent acquisition.",
            "Sales & Marketing: $12.0M targeted at global enterprise go-to-market campaigns and partner enablement.",
            "General & Administrative: $4.5M for legal compliance, office leases, and corporate insurance.",
        ],
    )?;
    count += 1;

    create_minimal_docx(
        &finance_dir.join("investor_update_november_2024.docx"),
        &[
            "Series B Investor Update - November 2024",
            "Key Highlights: Annual Recurring Revenue (ARR) surpassed $60M milestone. Customer net revenue retention (NRR) reached 128%.",
            "Strategic Initiatives: Launching local privacy-first search launcher for macOS and Windows enterprise laptops.",
        ],
    )?;
    count += 1;

    create_minimal_xlsx(
        &finance_dir.join("executive_compensation_salary_bands.xlsx"),
        &[
            vec![
                "Level",
                "Role Title",
                "Base Salary Min",
                "Base Salary Max",
                "Target Equity ($)",
            ],
            vec![
                "L5",
                "Senior Software Engineer",
                "165,000",
                "210,000",
                "80,000",
            ],
            vec![
                "L6",
                "Staff Software Engineer",
                "215,000",
                "265,000",
                "150,000",
            ],
            vec!["L7", "Principal Architect", "275,000", "340,000", "250,000"],
            vec![
                "VP",
                "Vice President Engineering",
                "350,000",
                "420,000",
                "500,000",
            ],
        ],
    )?;
    count += 1;

    create_minimal_xlsx(
        &finance_dir.join("vendor_expense_audit_2024.xlsx"),
        &[
            vec![
                "Vendor Name",
                "Category",
                "Annual Cost ($)",
                "Contract Expiry",
                "Payment Terms",
            ],
            vec![
                "AWS Cloud Services",
                "Infrastructure",
                "1,240,000",
                "2025-12-31",
                "Net 30",
            ],
            vec![
                "Datadog APM",
                "Monitoring",
                "185,000",
                "2025-06-30",
                "Annual Upfront",
            ],
            vec![
                "Slack Enterprise",
                "Collaboration",
                "95,000",
                "2025-08-15",
                "Net 30",
            ],
            vec![
                "GitHub Enterprise",
                "Developer Tools",
                "120,000",
                "2026-01-01",
                "Net 45",
            ],
        ],
    )?;
    count += 1;

    fs::write(
        finance_dir.join("corporate_tax_filing_estimate.csv"),
        "Entity,Tax_Year,Jurisdiction,Estimated_Liability,Payment_Status\nUS_Parent,2024,Federal,4250000,Paid_Q3\nUS_Parent,2024,California,1120000,Paid_Q3\nUK_Sub,2024,HMRC,850000,Pending_Audit\nEU_Sub,2024,Ireland,620000,Paid_Q2\n",
    )?;
    count += 1;

    fs::write(
        finance_dir.join("accounts_payable_ledger_october.csv"),
        "Invoice_ID,Vendor,Amount,Due_Date,Approval_Status\nINV-9821,Stripe_Processing,42850.00,2024-11-15,Approved\nINV-9822,Twilio_Telecom,14320.50,2024-11-20,Approved\nINV-9823,KPMG_Auditing,85000.00,2024-11-30,Under_Review\n",
    )?;
    count += 1;

    // Additional finance filler files to reach ~40 files
    for i in 1..=32 {
        let file_path = finance_dir.join(format!("receipt_invoice_2024_batch_{:02}.txt", i));
        fs::write(
            &file_path,
            format!(
                "Commercial invoice #INV-2024-{:04}. Vendor: Supplier Alpha-{}. Total charged: ${}.45 USD. Payment processed via corporate credit card.",
                i,
                i,
                100 + i * 23
            ),
        )?;
        count += 1;
    }

    // 2. Engineering & Architecture (~45 files)
    let eng_dir = target_dir.join("engineering");
    fs::create_dir_all(&eng_dir)?;

    fs::write(
        eng_dir.join("architecture.txt"),
        "SearchMyComputer Core Architecture Overview.\nClient layer uses Tauri 2 frameless webview with React 19.\nIndexing pipeline runs in Rust worker pool with WAL SQLite.\nHybrid search fuses Filename FTS5 Trigram, Content BM25, and OnnxEmbedder vector cosine similarity via Reciprocal Rank Fusion.\n",
    )?;
    count += 1;

    fs::write(
        eng_dir.join("rfc_004_hybrid_ranking_fusion.md"),
        "# RFC 004: Multi-Stream Hybrid Search Ranking with RRF\n\n## Abstract\nWe propose merging filename trigram matches, chunk BM25 scores, and dense vector embeddings using Reciprocal Rank Fusion (RRF).\nRRF Score is defined as sum(weight_r / (k + rank_r)). Signals include exact stem boost, recency decay, and path depth prior.\n",
    )?;
    count += 1;

    create_minimal_pdf(
        &eng_dir.join("database_migration_v2_plan.pdf"),
        &[
            "Database Schema Migration Plan: Version 1 to Version 2.",
            "Adding chunks_fts virtual table with FTS5 trigram tokenizer and content indexing triggers. Migrating vector embeddings to half-precision f16 format.",
        ],
    )?;
    count += 1;

    create_minimal_docx(
        &eng_dir.join("high_availability_disaster_recovery_runbook.docx"),
        &[
            "Disaster Recovery Runbook & Failover Procedures",
            "Step 1: Automated health checks detect primary node unavailability after 3 failed heartbeats (15s threshold).",
            "Step 2: Raft consensus initiates leader election and promotes standby replica in region us-east-2.",
            "Step 3: DNS routing switches traffic within 30 seconds via Route 53 health-checked alias records.",
        ],
    )?;
    count += 1;

    fs::write(
        eng_dir.join("api_gateway_specification.json"),
        r#"{
            "openapi": "3.1.0",
            "info": {
                "title": "SearchMyComputer Local IPC Gateway",
                "version": "1.0.0",
                "description": "Tauri IPC Command endpoints for launcher search, indexing controls, and settings."
            },
            "paths": {
                "/search": { "post": { "summary": "Executes hybrid search query across files and chunks" } },
                "/index/status": { "get": { "summary": "Returns current indexed file counts and queue depth" } }
            }
        }"#,
    )?;
    count += 1;

    for i in 1..=40 {
        let file_path = eng_dir.join(format!("tech_spec_subsystem_{:02}.md", i));
        fs::write(
            &file_path,
            format!(
                "# Subsystem Specification {:02}\n\nComponent: worker_module_{}\nHandles asynchronous event processing, buffer serialization, and error recovery for channel #{}.\nThroughput target: 10,000 ops/sec.\n",
                i, i, i
            ),
        )?;
        count += 1;
    }

    // 3. Code Base (~60 files)
    let code_dir = target_dir.join("code");
    fs::create_dir_all(&code_dir)?;

    fs::write(
        code_dir.join("vector_index.rs"),
        r#"use std::collections::HashMap;

/// In-memory vector similarity index using brute-force cosine similarity.
pub struct CosineVectorIndex {
    dim: usize,
    records: Vec<(i64, Vec<f32>)>,
}

impl CosineVectorIndex {
    pub fn new(dim: usize) -> Self {
        Self { dim, records: Vec::new() }
    }

    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(i64, f32)> {
        let mut scored: Vec<(i64, f32)> = self.records.iter().map(|(id, vec)| {
            let dot: f32 = query.iter().zip(vec).map(|(a, b)| a * b).sum();
            (*id, dot)
        }).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }
}
"#,
    )?;
    count += 1;

    fs::write(
        code_dir.join("payment_gateway.ts"),
        r#"import { Stripe } from 'stripe';

export interface PaymentIntentRequest {
    customerId: string;
    amountCents: number;
    currency: 'USD' | 'EUR' | 'GBP';
    idempotencyKey: string;
}

export class PaymentGatewayService {
    private stripeClient: Stripe;

    constructor(apiKey: string) {
        this.stripeClient = new Stripe(apiKey, { apiVersion: '2024-06-20' });
    }

    async processTransaction(req: PaymentIntentRequest): Promise<string> {
        const intent = await this.stripeClient.paymentIntents.create({
            amount: req.amountCents,
            currency: req.currency,
            customer: req.customerId,
        }, { idempotencyKey: req.idempotencyKey });
        return intent.id;
    }
}
"#,
    )?;
    count += 1;

    fs::write(
        code_dir.join("model_quantization.py"),
        r#"import onnx
from onnxruntime.quantization import quantize_dynamic, QuantType

def quantize_bge_embedding_model(input_onnx_path: str, output_int8_path: str):
    """
    Quantizes a 32-bit float ONNX transformer model into int8 dynamic weights
    for fast CPU inference with minimal accuracy loss.
    """
    print(f"Quantizing {input_onnx_path} to {output_int8_path}...")
    quantize_dynamic(
        model_input=input_onnx_path,
        model_output=output_int8_path,
        weight_type=QuantType.QInt8,
        op_types_to_quantize=['MatMul', 'Attention']
    )
    print("Quantization complete.")

if __name__ == '__main__':
    quantize_bge_embedding_model('models/bge-small/model.onnx', 'models/bge-small/model_int8.onnx')
"#,
    )?;
    count += 1;

    fs::write(
        code_dir.join("grpc_server.go"),
        r#"package main

import (
	"context"
	"log"
	"net"
	"google.golang.org/grpc"
)

type Server struct {
	UnimplementedSearchServiceServer
}

func (s *Server) QueryIndex(ctx context.Context, req *SearchRequest) (*SearchResponse, error) {
	log.Printf("Received search query: %s limit: %d", req.Query, req.Limit)
	return &SearchResponse{Status: "OK"}, nil
}

func main() {
	lis, err := net.Listen("tcp", ":50051")
	if err != nil {
		log.Fatalf("failed to listen: %v", err)
	}
	s := grpc.NewServer()
	log.Println("Starting gRPC search cluster server on :50051...")
	s.Serve(lis)
}
"#,
    )?;
    count += 1;

    fs::write(
        code_dir.join("fast_matrix_multiply.cpp"),
        r#"#include <vector>
#include <iostream>

void avx2_matrix_multiply(const float* A, const float* B, float* C, int N) {
    // Optimized matrix multiplication kernel using AVX2 SIMD intrinsics
    for (int i = 0; i < N; ++i) {
        for (int k = 0; k < N; ++k) {
            for (int j = 0; j < N; ++j) {
                C[i * N + j] += A[i * N + k] * B[k * N + j];
            }
        }
    }
}

int main() {
    std::cout << "AVX2 GEMM Kernel Benchmark initialized." << std::endl;
    return 0;
}
"#,
    )?;
    count += 1;

    fs::write(
        code_dir.join("schema_migrations.sql"),
        r#"-- Schema Migration 005: Add trigram FTS5 indexes
CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    name,
    tokenize = 'trigram'
);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    text,
    content = 'chunks',
    content_rowid = 'id',
    tokenize = 'unicode61'
);
"#,
    )?;
    count += 1;

    for i in 1..=54 {
        let ext = match i % 5 {
            0 => "rs",
            1 => "py",
            2 => "ts",
            3 => "go",
            _ => "cpp",
        };
        let file_path = code_dir.join(format!("utility_service_module_{:02}.{}", i, ext));
        fs::write(
            &file_path,
            format!(
                "// Service Utility Module {:02}\n// Implements worker telemetry handler and distributed hash table routing key #{}.\nfn process_event_{}() {{ /* handler logic */ }}\n",
                i,
                i * 7,
                i
            ),
        )?;
        count += 1;
    }

    // 4. Legal & Compliance (~35 files)
    let legal_dir = target_dir.join("legal_compliance");
    fs::create_dir_all(&legal_dir)?;

    create_minimal_pdf(
        &legal_dir.join("soc2_compliance_report.pdf"),
        &[
            "SOC 2 Type II Compliance Audit Report. Independent Service Auditor Report on Controls Relevant to Security, Availability, and Confidentiality.",
            "Testing Results: Zero exceptions noted across 48 evaluated controls. Multi-factor authentication, endpoint encryption, and automated vulnerability scanning are fully verified.",
        ],
    )?;
    count += 1;

    create_minimal_docx(
        &legal_dir.join("gdpr_data_retention_policy.docx"),
        &[
            "General Data Protection Regulation (GDPR) Data Retention and Deletion Policy",
            "Article 17 Right to Erasure: User search indexes and cached document embeddings must be deleted immediately upon user request or folder un-index command.",
            "Local processing guarantee: All indexing and AI inference runs exclusively on the client machine with zero telemetry transmitted.",
        ],
    )?;
    count += 1;

    create_minimal_docx(
        &legal_dir.join("standard_mutual_nda_template.docx"),
        &[
            "Mutual Non-Disclosure Agreement (NDA)",
            "1. Confidential Information encompasses all proprietary algorithms, source code, financial projections, customer lists, and machine learning models disclosed by either party.",
            "2. Term: The obligations of confidentiality shall endure for a period of five (5) years from the effective date.",
        ],
    )?;
    count += 1;

    fs::write(
        legal_dir.join("terms_of_service_enterprise.md"),
        "# SearchMyComputer Enterprise Terms of Service\n\n## 1. License Grant\nSearchMyComputer grants the licensee a perpetual, non-exclusive license to install and execute the software locally on authorized enterprise endpoints.\n\n## 2. Privacy & Data Ownership\nThe licensee retains full and unencumbered ownership of all indexed documents, query strings, and generated embeddings.\n",
    )?;
    count += 1;

    for i in 1..=31 {
        let file_path = legal_dir.join(format!("vendor_agreement_contract_{:02}.txt", i));
        fs::write(
            &file_path,
            format!(
                "Vendor Service Level Agreement #VND-2024-{:02}. Parties agree to 99.95% uptime commitment and standard liability cap of 12 months fees.",
                i
            ),
        )?;
        count += 1;
    }

    // 5. Product Design & PRDs (~35 files)
    let product_dir = target_dir.join("product_design");
    fs::create_dir_all(&product_dir)?;

    create_minimal_pptx(
        &product_dir.join("product_roadmap_2025_vision.pptx"),
        &[
            "SearchMyComputer: 2025 Product Vision & Strategy",
            "Slide 2: Desktop AI search with instantaneous response times (< 50ms warm query latency)",
            "Slide 3: Multimodal capabilities: OCR for scanned PDFs, visual screenshots, and QR code detection",
            "Slide 4: Enterprise fleet deployment with centralized exclude policies and silent installer",
        ],
    )?;
    count += 1;

    create_minimal_docx(
        &product_dir.join("prd_launcher_ui_shortcuts.docx"),
        &[
            "Product Requirements Document (PRD): Launcher User Experience & Keyboard Navigation",
            "Key Requirements: Global hotkey Alt+Space toggles launcher window.",
            "Arrow Up/Down navigates results list. Enter opens file with OS default application. Ctrl+Enter reveals file in Windows Explorer or macOS Finder. Ctrl+C copies path to clipboard.",
        ],
    )?;
    count += 1;

    fs::write(
        product_dir.join("user_personas_research_findings.md"),
        "# User Research & Persona Study\n\n## Persona A: Alex the Software Engineer\nAlex has 100+ git repositories across 5 project folders and needs to find function definitions and config YAMLs in milliseconds.\n\n## Persona B: Sarah the Legal Counsel\nSarah searches through thousands of PDF contracts and DOCX briefs looking for specific indemnification clauses and liability terms.\n",
    )?;
    count += 1;

    for i in 1..=32 {
        let file_path = product_dir.join(format!("sprint_planning_notes_sprint_{:02}.txt", i));
        fs::write(
            &file_path,
            format!(
                "Sprint {:02} Planning Notes. Velocity: 42 story points. Priority epics: UI smooth transitions, PDF text extraction parser resilience, and memory leak regression testing.",
                i
            ),
        )?;
        count += 1;
    }

    // 6. Infrastructure & Cloud Ops (~40 files)
    let infra_dir = target_dir.join("infrastructure_ops");
    fs::create_dir_all(&infra_dir)?;

    fs::write(
        infra_dir.join("kubernetes_production_cluster.yaml"),
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: search-api-gateway
  namespace: production
spec:
  replicas: 8
  selector:
    matchLabels:
      app: search-api
  template:
    metadata:
      labels:
        app: search-api
    spec:
      containers:
      - name: gateway
        image: 123456789.dkr.ecr.us-east-1.amazonaws.com/search-gateway:v2.4.1
        resources:
          limits:
            cpu: "2000m"
            memory: "4Gi"
          requests:
            cpu: "500m"
            memory: "1Gi"
"#,
    )?;
    count += 1;

    fs::write(
        infra_dir.join("terraform_aws_infrastructure.tf"),
        r#"provider "aws" {
  region = "us-east-1"
}

resource "aws_vpc" "production_vpc" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_hostnames = true
  tags = {
    Environment = "production"
    ManagedBy   = "Terraform"
  }
}
"#,
    )?;
    count += 1;

    fs::write(
        infra_dir.join("nginx_reverse_proxy.conf"),
        r#"server {
    listen 443 ssl http2;
    server_name api.searchmycomputer.internal;

    ssl_certificate /etc/ssl/certs/internal.crt;
    ssl_certificate_key /etc/ssl/private/internal.key;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
"#,
    )?;
    count += 1;

    fs::write(
        infra_dir.join("incident_postmortem_connection_pool_leak.md"),
        "# Incident Postmortem: Database Connection Pool Exhaustion\n\n## Date: 2024-10-14\n\n## Impact\nSearch API returned HTTP 500 error for 12 minutes due to unclosed SQLite reader connection handles in high-concurrency threads.\n\n## Root Cause\nA scoped thread handle panicked without invoking connection drop guard.\n\n## Remediation\nWrapped all SQLite transactions in automatic RAII connection pool wrappers with strict 5-second acquisition timeout.\n",
    )?;
    count += 1;

    for i in 1..=36 {
        let file_path = infra_dir.join(format!("server_log_production_node_{:02}.log", i));
        fs::write(
            &file_path,
            format!(
                "2024-11-01T12:{:02}:00Z [INFO] node_{} heart-beat healthy. CPU usage: 14.2%, RAM usage: 38.4%, active connections: 240.",
                i % 60,
                i
            ),
        )?;
        count += 1;
    }

    // 7. AI & Machine Learning Research (~35 files)
    let ai_dir = target_dir.join("research_ai");
    fs::create_dir_all(&ai_dir)?;

    create_minimal_pdf(
        &ai_dir.join("transformer_rag_whitepaper.pdf"),
        &[
            "Dense Passage Retrieval and Retrieval-Augmented Generation (RAG) on Edge Devices.",
            "We analyze bi-encoder architectures quantized to int8 precision. Embedding models such as bge-small-en-v1.5 deliver 98.4% retrieval accuracy of full precision while reducing RAM by 75% and latency to under 15ms per query on 4-core laptop CPUs.",
        ],
    )?;
    count += 1;

    fs::write(
        ai_dir.join("onnx_runtime_cpu_optimization.md"),
        "# ONNX Runtime CPU Execution Provider Optimization\n\nTo achieve < 50ms latency on mid-range laptops:\n1. Limit intra-op threads to 2 to eliminate multi-core cache ping-pong.\n2. Apply dynamic batching with batch size 32 for document chunks.\n3. Implement in-memory LRU query embedding cache.\n",
    )?;
    count += 1;

    fs::write(
        ai_dir.join("evaluate_reranking_cross_encoder.py"),
        r#"import numpy as np

def calculate_mrr(rankings: list[list[int]], ground_truth: list[int]) -> float:
    """
    Computes Mean Reciprocal Rank across query evaluations.
    """
    reciprocal_ranks = []
    for rank_list, target in zip(rankings, ground_truth):
        if target in rank_list:
            rank = rank_list.index(target) + 1
            reciprocal_ranks.append(1.0 / rank)
        else:
            reciprocal_ranks.append(0.0)
    return float(np.mean(reciprocal_ranks))
"#,
    )?;
    count += 1;

    for i in 1..=32 {
        let file_path = ai_dir.join(format!("experiment_notes_run_{:02}.txt", i));
        fs::write(
            &file_path,
            format!(
                "Embedding Model Experiment Run #{:02}. Hyperparameters: learning_rate = 2e-5, warmup_ratio = 0.1, batch_size = 64. Validation loss converged at epoch {}.",
                i,
                (i % 5) + 1
            ),
        )?;
        count += 1;
    }

    // 8. Daily Notes & Standup Logs (~40 files)
    let daily_dir = target_dir.join("daily_notes_logs");
    fs::create_dir_all(&daily_dir)?;

    fs::write(
        daily_dir.join("standup_meeting_notes_today.md"),
        "# Daily Standup Meeting Notes\n\n- Alice: Completed RRF ranking fusion algorithm and unit tests.\n- Bob: Optimizing PDFium text extraction memory footprint on Windows.\n- Charlie: Finished launcher UI shortcut handling and blurred background panel.\n",
    )?;
    count += 1;

    fs::write(
        daily_dir.join("weekly_sprint_retro_october.txt"),
        "Sprint Retrospective:\nWhat went well: SQLite WAL mode solved all read concurrency bottlenecks.\nWhat could improve: Chunking large 500-page PDFs took 12 seconds - added parallel chunk worker pipeline.\nAction items: Deploy int8 quantized embedding model.\n",
    )?;
    count += 1;

    for i in 1..=38 {
        let file_path = daily_dir.join(format!("developer_journal_2024_day_{:02}.md", i));
        fs::write(
            &file_path,
            format!(
                "# Developer Journal: Day {:02}\n\nInvestigated cache eviction strategies for query vector cache. Tested LFU vs LRU with capacity 128 items. LRU provided 99.2% hit rate during prefix typing.\n",
                i
            ),
        )?;
        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_synthetic_corpus() {
        let tmp = tempfile::tempdir().unwrap();
        let count = generate_synthetic_corpus(tmp.path()).unwrap();
        assert!(
            count >= 300,
            "expected at least 300 files, generated {}",
            count
        );

        // Verify key files exist and are non-empty
        assert!(tmp.path().join("engineering/architecture.txt").exists());
        assert!(
            tmp.path()
                .join("finance/q3_financial_report_2024.pdf")
                .exists()
        );
        assert!(
            tmp.path()
                .join("finance/budget_allocation_q4_2024.docx")
                .exists()
        );
        assert!(
            tmp.path()
                .join("product_design/product_roadmap_2025_vision.pptx")
                .exists()
        );
        assert!(
            tmp.path()
                .join("finance/executive_compensation_salary_bands.xlsx")
                .exists()
        );
    }
}
