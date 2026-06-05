use std::fs;
use std::path::{Path, PathBuf};

use crate::app::build_rag_service;
use crate::config::Config;
use crate::ingestion::document_title_from_text;
use crate::models::{ConfidenceLevel, Document, default_access_scope};

pub fn run(args: &[String]) -> Option<Result<(), Box<dyn std::error::Error>>> {
    match args.first().map(String::as_str) {
        Some("eval") => Some(run_eval()),
        Some("ingest") => Some(run_ingest(&args[1..])),
        _ => None,
    }
}

fn run_ingest(paths: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if paths.is_empty() {
        return Err("usage: knowledge-pilot ingest <file-or-directory> [...]".into());
    }

    let config = Config::from_env();
    let rag = build_rag_service(&config)?;
    let mut files = Vec::new();

    for path in paths {
        collect_ingest_files(Path::new(path), &mut files)?;
    }

    let mut indexed = 0;
    let mut chunks = 0;
    for path in files {
        let text = fs::read_to_string(&path)?;
        if text.trim().is_empty() {
            continue;
        }

        let source = path.to_string_lossy().to_string();
        let title = document_title_from_text(&text).unwrap_or_else(|| title_from_path(&path));
        let document = Document {
            id: document_id_from_path(&path),
            title,
            source,
            access_scope: default_access_scope(),
            text,
        };
        let chunk_count = rag.add_document(document)?;
        indexed += 1;
        chunks += chunk_count;
    }

    println!("ingest_result indexed={} chunks={}", indexed, chunks);
    Ok(())
}

fn collect_ingest_files(
    path: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_file() {
        if is_supported_text_file(path) {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_ingest_files(&entry?.path(), files)?;
        }
        return Ok(());
    }

    Err(format!("ingest path does not exist: {}", path.display()).into())
}

fn is_supported_text_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_lowercase()),
        Some(extension) if matches!(extension.as_str(), "md" | "markdown" | "txt")
    )
}

fn document_id_from_path(path: &Path) -> String {
    let source = path.to_string_lossy();
    let mut id = String::from("file-");
    for byte in source.as_bytes() {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

fn run_eval() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    let rag = build_rag_service(&config)?;
    let eval_root = PathBuf::from("eval");
    let documents_dir = eval_root.join("documents");
    let cases_path = eval_root.join("cases.json");

    let mut files = Vec::new();
    collect_ingest_files(&documents_dir, &mut files)?;
    let diverse_dir = documents_dir.join("diverse");
    if diverse_dir.is_dir() {
        collect_ingest_files(&diverse_dir, &mut files)?;
    }
    for path in files {
        let text = fs::read_to_string(&path)?;
        let source = path.to_string_lossy().to_string();
        let title = document_title_from_text(&text).unwrap_or_else(|| title_from_path(&path));
        rag.add_document(Document {
            id: document_id_from_path(&path),
            title,
            source,
            access_scope: default_access_scope(),
            text,
        })?;
    }

    let cases: Vec<EvalCase> = serde_json::from_str(&fs::read_to_string(cases_path)?)?;

    let mut passed = 0;
    for case in &cases {
        let top_k = case.top_k.unwrap_or(5).max(1);
        let response = rag.answer(&case.question, top_k)?;
        let source_hit = case.allow_additional_sources
            || case.expected_source.as_ref().is_none_or(|expected_source| {
                response
                    .citations
                    .iter()
                    .any(|citation| citation.source == *expected_source)
            });
        let unexpected_citation_sources: Vec<String> =
            match (case.allow_additional_sources, case.expected_source.as_ref()) {
                (true, _) | (_, None) => Vec::new(),
                (false, Some(expected_source)) => response
                    .citations
                    .iter()
                    .filter(|citation| citation.source != *expected_source)
                    .map(|citation| citation.source.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            };
        let missing_answer_contains: Vec<&str> = case
            .expected_answer_contains
            .iter()
            .filter(|phrase| !response.answer.contains(phrase.as_str()))
            .map(String::as_str)
            .collect();
        let passed_case = source_hit
            && unexpected_citation_sources.is_empty()
            && missing_answer_contains.is_empty()
            && case
                .expected_confidence_level
                .as_ref()
                .is_none_or(|level| response.confidence.level == *level);
        if passed_case {
            passed += 1;
        }
        let missing_json = serde_json::to_string(&missing_answer_contains)?;
        let unexpected_sources_json = serde_json::to_string(&unexpected_citation_sources)?;
        println!(
            "{}\tquestion=\"{}\"\texpected_source=\"{}\"\tsource_hit={}\tunexpected_citation_sources={}\ttop_k={}\tconfidence={:?}:{:.3}\tmissing_answer_contains={}\tanswer=\"{}\"",
            if passed_case { "PASS" } else { "FAIL" },
            case.question,
            case.expected_source.as_deref().unwrap_or(""),
            source_hit,
            unexpected_sources_json,
            top_k,
            response.confidence.level,
            response.confidence.score,
            missing_json,
            response.answer.replace('\n', " ")
        );
    }

    println!("eval_result passed={} total={}", passed, cases.len());

    if passed == cases.len() {
        Ok(())
    } else {
        Err("evaluation failed".into())
    }
}

fn title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document")
        .replace(['_', '-'], " ")
}

#[derive(serde::Deserialize)]
struct EvalCase {
    question: String,
    expected_source: Option<String>,
    #[serde(default)]
    expected_answer_contains: Vec<String>,
    #[serde(default)]
    allow_additional_sources: bool,
    expected_confidence_level: Option<ConfidenceLevel>,
    top_k: Option<usize>,
}
