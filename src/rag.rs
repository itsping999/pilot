use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use log::{info, warn};

use crate::agent::{AgentPlan, QueryPlanner};
use crate::embedding::{EmbedError, Embedder};
use crate::generation::{GenerateError, Generator};
use crate::ingestion::chunk_document;
use crate::models::{
    AnswerConfidence, Chunk, Citation, ConfidenceLevel, Document, DocumentSummary,
    QueryHistoryEntry, RagResponse, RetrievedChunk, allowed_access_scopes,
};
use crate::retrieval::Retriever;
use crate::store::SqliteStore;

pub struct RagService {
    store: Arc<SqliteStore>,
    embedder: Arc<dyn Embedder>,
    retriever: Arc<dyn Retriever>,
    generator: Arc<dyn Generator>,
    query_planner: Option<Arc<dyn QueryPlanner>>,
    chunk_size: usize,
    chunk_overlap: usize,
}

impl RagService {
    pub fn new(
        store: Arc<SqliteStore>,
        embedder: Arc<dyn Embedder>,
        retriever: Arc<dyn Retriever>,
        generator: Arc<dyn Generator>,
        query_planner: Option<Arc<dyn QueryPlanner>>,
        chunk_size: usize,
        chunk_overlap: usize,
    ) -> Self {
        Self {
            store,
            embedder,
            retriever,
            generator,
            query_planner,
            chunk_size,
            chunk_overlap,
        }
    }

    pub fn add_document(&self, document: Document) -> Result<usize, RagError> {
        if let Some(chunk_count) = self.store.is_document_up_to_date(
            &document.id,
            &document.title,
            &document.source,
            &document.access_scope,
            &document.text,
            self.embedder.model_name(),
        )? {
            return Ok(chunk_count);
        }

        let chunks = chunk_document(&document, self.chunk_size, self.chunk_overlap);
        let mut embedded_chunks = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let vector = self.embedder.embed(&chunk.text)?;
            embedded_chunks.push((chunk.clone(), vector));
        }
        self.store.replace_document_with_embeddings(
            &document,
            &embedded_chunks,
            self.embedder.model_name(),
        )?;
        Ok(chunks.len())
    }

    pub fn list_documents(&self) -> rusqlite::Result<Vec<DocumentSummary>> {
        self.store.list_documents()
    }

    pub fn get_document(&self, id: &str) -> rusqlite::Result<Option<Document>> {
        self.store.get_document(id)
    }

    pub fn list_document_chunks(&self, id: &str) -> rusqlite::Result<Vec<Chunk>> {
        self.store.list_document_chunks(id)
    }

    pub fn delete_document(&self, id: &str) -> rusqlite::Result<bool> {
        self.store.delete_document(id)
    }

    pub fn list_query_history(&self, limit: usize) -> rusqlite::Result<Vec<QueryHistoryEntry>> {
        self.store.list_query_history(limit)
    }

    pub fn embedding_model_name(&self) -> &str {
        self.embedder.model_name()
    }

    pub fn generator_type(&self) -> &str {
        self.generator.name()
    }

    pub fn ensure_model_configured(&self) -> Result<(), RagError> {
        self.generator.ensure_configured()?;
        Ok(())
    }

    pub fn answer(&self, question: &str, top_k: usize) -> Result<RagResponse, RagError> {
        self.answer_with_scopes(question, top_k, &[])
    }

    pub fn answer_with_scopes(
        &self,
        question: &str,
        top_k: usize,
        access_scopes: &[String],
    ) -> Result<RagResponse, RagError> {
        self.ensure_model_configured()?;
        let allowed_scopes = allowed_access_scopes(access_scopes);
        if let Some(planner) = &self.query_planner {
            return self.answer_agentic(question, top_k, planner.as_ref(), &allowed_scopes);
        }

        self.answer_traditional(question, top_k, &allowed_scopes)
    }

    pub fn answer_stream_with_scopes<F>(
        &self,
        question: &str,
        top_k: usize,
        access_scopes: &[String],
        mut on_token: F,
    ) -> Result<RagResponse, RagError>
    where
        F: FnMut(&str) -> Result<(), GenerateError>,
    {
        self.ensure_model_configured()?;
        let allowed_scopes = allowed_access_scopes(access_scopes);
        let context = if let Some(planner) = &self.query_planner {
            self.retrieve_agentic_context(question, top_k, planner.as_ref(), &allowed_scopes)?
        } else {
            self.retrieve_context(question, top_k, &allowed_scopes)?
        };

        let confidence = confidence_for_context(question, &context);
        let answer = if confidence.level == ConfidenceLevel::Low {
            let ans = insufficient_context_answer(question);
            on_token(&ans)?;
            ans
        } else {
            self.generator
                .generate_stream(question, &context, &mut on_token)?
        };

        let citations = citations_for_answer(&answer, &context);
        let citations_json = serde_json::to_string(&citations)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.store.record_answer(
            question,
            &answer,
            &citations_json,
            confidence.level.as_str(),
            confidence.score,
            &confidence.reason,
        )?;

        Ok(RagResponse {
            answer,
            citations,
            confidence,
        })
    }

    fn answer_traditional(
        &self,
        question: &str,
        top_k: usize,
        allowed_scopes: &[String],
    ) -> Result<RagResponse, RagError> {
        let context = self.retrieve_context(question, top_k, allowed_scopes)?;
        self.answer_from_context(question, context)
    }

    fn answer_agentic(
        &self,
        question: &str,
        top_k: usize,
        planner: &dyn QueryPlanner,
        allowed_scopes: &[String],
    ) -> Result<RagResponse, RagError> {
        let context = self.retrieve_agentic_context(question, top_k, planner, allowed_scopes)?;
        self.answer_from_context(question, context)
    }

    fn retrieve_agentic_context(
        &self,
        question: &str,
        top_k: usize,
        planner: &dyn QueryPlanner,
        allowed_scopes: &[String],
    ) -> Result<Vec<RetrievedChunk>, RagError> {
        const MAX_AGENT_CONTEXT_CHUNKS: usize = 32;

        let plan = match planner.plan(question) {
            Ok(plan) => plan,
            Err(error) => {
                warn!("agentic planner failed, using original question only: {error}");
                AgentPlan::fallback(question)
            }
        };

        let mut seen = HashSet::new();
        let mut context = Vec::new();
        let initial_queries = self.retrieve_context_for_queries(
            plan.search_queries(),
            top_k,
            allowed_scopes,
            &mut seen,
            &mut context,
            MAX_AGENT_CONTEXT_CHUNKS,
        )?;

        let mut follow_up_queries = 0;
        if !context.is_empty() && context.len() < MAX_AGENT_CONTEXT_CHUNKS {
            match planner.plan_follow_up(question, &context) {
                Ok(follow_up_plan) => {
                    follow_up_queries = self.retrieve_context_for_queries(
                        follow_up_plan.search_queries(),
                        top_k,
                        allowed_scopes,
                        &mut seen,
                        &mut context,
                        MAX_AGENT_CONTEXT_CHUNKS,
                    )?;
                }
                Err(error) => {
                    warn!("agentic follow-up planner failed: {error}");
                }
            }
        }

        info!(
            "agentic rag executed: initial_search_queries={} follow_up_search_queries={} context_chunks={}",
            initial_queries,
            follow_up_queries,
            context.len()
        );

        Ok(context)
    }

    fn retrieve_context_for_queries(
        &self,
        queries: &[String],
        top_k: usize,
        allowed_scopes: &[String],
        seen_chunks: &mut HashSet<String>,
        context: &mut Vec<RetrievedChunk>,
        max_context_chunks: usize,
    ) -> Result<usize, RagError> {
        let mut executed_queries = 0;
        for query in queries {
            if context.len() >= max_context_chunks {
                break;
            }

            executed_queries += 1;
            for item in self.retrieve_context(query, top_k, allowed_scopes)? {
                if seen_chunks.insert(item.chunk.id.clone()) {
                    context.push(item);
                    if context.len() >= max_context_chunks {
                        break;
                    }
                }
            }
        }

        Ok(executed_queries)
    }

    fn retrieve_context(
        &self,
        question: &str,
        top_k: usize,
        allowed_scopes: &[String],
    ) -> Result<Vec<RetrievedChunk>, RagError> {
        let query_vector = self.embedder.embed(question)?;
        let retrieved = self.retriever.retrieve(
            question,
            &query_vector,
            self.embedder.model_name(),
            allowed_scopes,
            top_k,
        )?;
        Ok(self.expand_context(&retrieved)?)
    }

    fn answer_from_context(
        &self,
        question: &str,
        context: Vec<RetrievedChunk>,
    ) -> Result<RagResponse, RagError> {
        let confidence = confidence_for_context(question, &context);
        let answer = if confidence.level == ConfidenceLevel::Low {
            insufficient_context_answer(question)
        } else {
            self.generator.generate(question, &context)?
        };
        let citations = citations_for_answer(&answer, &context);

        let citations_json = serde_json::to_string(&citations)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.store.record_answer(
            question,
            &answer,
            &citations_json,
            confidence.level.as_str(),
            confidence.score,
            &confidence.reason,
        )?;

        Ok(RagResponse {
            answer,
            citations,
            confidence,
        })
    }

    fn expand_context(
        &self,
        retrieved: &[RetrievedChunk],
    ) -> rusqlite::Result<Vec<RetrievedChunk>> {
        const NEIGHBOR_WINDOW: usize = 2;
        const MAX_CONTEXT_CHUNKS: usize = 20;

        let mut seen = HashSet::new();
        let mut context = Vec::new();

        for item in retrieved {
            let chunks = self.store.list_document_chunks(&item.chunk.document_id)?;

            for chunk_index in neighbor_indices(item.chunk.chunk_index, NEIGHBOR_WINDOW) {
                let Some(chunk) = chunks
                    .iter()
                    .find(|chunk| chunk.chunk_index == chunk_index)
                    .cloned()
                else {
                    continue;
                };

                if seen.insert(chunk.id.clone()) {
                    let distance = chunk.chunk_index.abs_diff(item.chunk.chunk_index) as f32;
                    context.push(RetrievedChunk {
                        chunk,
                        score: item.score * (1.0 - distance * 0.08).max(0.7),
                    });
                    if context.len() >= MAX_CONTEXT_CHUNKS {
                        return Ok(context);
                    }
                }
            }
        }

        Ok(context)
    }
}

fn confidence_for_context(question: &str, context: &[RetrievedChunk]) -> AnswerConfidence {
    let Some(max_score) = context.iter().map(|item| item.score).max_by(f32::total_cmp) else {
        return AnswerConfidence {
            level: ConfidenceLevel::Low,
            score: 0.0,
            reason: "no_retrieved_context".to_string(),
        };
    };

    let lexical_hits = confidence_lexical_hits(question, context);
    let lexical_score = if lexical_hits >= 2 {
        0.35 + (lexical_hits.min(8) as f32 * 0.03)
    } else {
        0.0
    };
    let score = max_score.max(lexical_score).clamp(0.0, 1.0);
    let (level, reason) = if score >= 0.7 {
        (ConfidenceLevel::High, "strong_retrieval_support")
    } else if score >= 0.3 {
        (ConfidenceLevel::Medium, "moderate_retrieval_support")
    } else {
        (ConfidenceLevel::Low, "weak_retrieval_support")
    };

    AnswerConfidence {
        level,
        score,
        reason: reason.to_string(),
    }
}

fn confidence_lexical_hits(question: &str, context: &[RetrievedChunk]) -> usize {
    let terms = confidence_terms(question);
    if terms.is_empty() {
        return 0;
    }

    let text = context
        .iter()
        .map(|item| item.chunk.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .count()
}

fn confidence_terms(text: &str) -> Vec<String> {
    let chars: Vec<char> = text
        .chars()
        .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
        .collect();
    let mut terms = HashSet::new();

    for width in 2..=3 {
        for window in chars.windows(width) {
            let term = window.iter().collect::<String>();
            if !is_generic_confidence_term(&term) {
                terms.insert(term);
            }
        }
    }

    terms.into_iter().collect()
}

fn is_generic_confidence_term(term: &str) -> bool {
    [
        "哪些",
        "什么",
        "如何",
        "怎么",
        "包括",
        "内容",
        "步骤",
        "流程",
        "字段",
        "需要",
        "应该",
        "应当",
        "覆盖",
        "关注",
        "事项",
        "哪些内",
        "些内容",
    ]
    .iter()
    .any(|generic| term.contains(generic))
}

fn insufficient_context_answer(question: &str) -> String {
    if question
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    {
        "根据当前资料无法确认。已检索到的相关来源仅供核对，请补充更匹配的文档后再提问。".to_string()
    } else {
        "I do not have enough retrieved context to answer. The retrieved sources are only related references; add a more relevant document and ask again.".to_string()
    }
}

fn neighbor_indices(center: usize, window: usize) -> Vec<usize> {
    let mut indices = Vec::with_capacity(window * 2 + 1);
    indices.push(center);

    for distance in 1..=window {
        if let Some(before) = center.checked_sub(distance) {
            indices.push(before);
        }
        indices.push(center + distance);
    }

    indices
}

fn citations_for_answer(answer: &str, context: &[RetrievedChunk]) -> Vec<Citation> {
    let support_lines = answer_support_lines(answer);
    let mut selected: Vec<&RetrievedChunk> = context
        .iter()
        .filter(|item| chunk_supports_answer_lines(&support_lines, &item.chunk.text))
        .collect();

    if selected.is_empty() {
        selected = context.iter().take(5).collect();
    } else {
        let best_sources = best_supported_sources(&support_lines, &selected);
        selected.retain(|item| best_sources.contains(&source_support_key(item)));
    }

    let mut seen = HashSet::new();
    selected
        .into_iter()
        .filter(|item| seen.insert(item.chunk.id.clone()))
        .map(|item| Citation {
            document_id: item.chunk.document_id.clone(),
            chunk_id: item.chunk.id.clone(),
            document_title: item.chunk.document_title.clone(),
            section: item.chunk.section.clone(),
            source: item.chunk.source.clone(),
            access_scope: item.chunk.access_scope.clone(),
            score: item.score,
        })
        .collect()
}

fn best_supported_sources(
    support_lines: &[String],
    selected: &[&RetrievedChunk],
) -> HashSet<String> {
    let mut support_by_source: HashMap<String, SourceSupport> = HashMap::new();
    for (position, item) in selected.iter().enumerate() {
        let source_key = source_support_key(item);
        let matched_lines = matching_support_lines(support_lines, &item.chunk.text);
        let support = support_by_source
            .entry(source_key)
            .or_insert_with(|| SourceSupport {
                first_position: position,
                ..SourceSupport::default()
            });
        support.best_retrieval_score = support.best_retrieval_score.max(item.score);
        support.first_position = support.first_position.min(position);

        for line in matched_lines {
            if support.matched_lines.insert(line.clone()) {
                support.weight += support_line_weight(&line);
            }
        }
    }

    let mut ranked: Vec<(String, SourceSupport)> = support_by_source.into_iter().collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .weight
            .cmp(&left.1.weight)
            .then_with(|| right.1.matched_lines.len().cmp(&left.1.matched_lines.len()))
            .then_with(|| {
                right
                    .1
                    .best_retrieval_score
                    .total_cmp(&left.1.best_retrieval_score)
            })
            .then_with(|| left.1.first_position.cmp(&right.1.first_position))
    });

    let Some((best_source, best_support)) = ranked.first() else {
        return HashSet::new();
    };
    let best_weight = best_support.weight;
    let best_line_count = best_support.matched_lines.len();

    let Some((_, second_support)) = ranked.get(1) else {
        return HashSet::from([best_source.clone()]);
    };

    if best_weight >= second_support.weight + 8
        || (best_weight > second_support.weight
            && best_line_count >= second_support.matched_lines.len())
    {
        return HashSet::from([best_source.clone()]);
    }

    ranked
        .into_iter()
        .filter_map(|(source, support)| {
            (support.weight == best_weight && support.matched_lines.len() == best_line_count)
                .then_some(source)
        })
        .collect()
}

#[derive(Default)]
struct SourceSupport {
    matched_lines: HashSet<String>,
    weight: usize,
    best_retrieval_score: f32,
    first_position: usize,
}

fn support_line_weight(line: &str) -> usize {
    let chars = line.chars().count();
    chars + if chars >= 12 { 8 } else { 0 }
}

fn source_support_key(item: &RetrievedChunk) -> String {
    format!("{}::{}", item.chunk.document_id, item.chunk.source)
}

fn chunk_supports_answer_lines(support_lines: &[String], chunk_text: &str) -> bool {
    let matched_lines = matching_support_lines(support_lines, chunk_text);
    let matched_count = matched_lines.len();

    if support_lines.len() <= 2 {
        return matched_count > 0;
    }

    if support_lines.len() <= 5 {
        return matched_count >= 2 || matched_lines.iter().any(|line| line.chars().count() >= 8);
    }

    let required_matches = if support_lines.len() <= 10 { 3 } else { 4 };
    matched_count >= required_matches
}

fn matching_support_lines(support_lines: &[String], chunk_text: &str) -> Vec<String> {
    let chunk_text = compact_for_support(chunk_text);
    support_lines
        .iter()
        .filter(|line| chunk_text.contains(line.as_str()))
        .cloned()
        .collect()
}

fn answer_support_lines(answer: &str) -> Vec<String> {
    answer
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line
                .split_once(". ")
                .map(|(_, tail)| tail)
                .unwrap_or(line)
                .trim_matches(['。', '；', ';', ' ', '\t']);
            let line = compact_for_support(line);
            (line.chars().count() >= 4
                && !line.starts_with("根据检索到的资料")
                && !line.starts_with("Based on"))
            .then_some(line)
        })
        .collect()
}

fn compact_for_support(text: &str) -> String {
    text.split_whitespace().collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retrieved_chunk(index: usize, text: &str) -> RetrievedChunk {
        RetrievedChunk {
            chunk: Chunk {
                id: format!("doc-{index}:0"),
                document_id: format!("doc-{index}"),
                chunk_index: 0,
                document_title: format!("文档 {index}"),
                section: "测试章节".to_string(),
                text: text.to_string(),
                source: format!("eval/documents/doc-{index}.md"),
                access_scope: "public".to_string(),
            },
            score: 0.8,
        }
    }

    #[test]
    fn citations_keep_only_chunks_that_support_answer_lines() {
        let answer = "根据检索到的资料，答案如下：\n\n1. 数据资产台账编制\n2. 数据资产登记\n";
        let context = vec![
            retrieved_chunk(
                1,
                "数据资产全过程管理重点环节包括数据资产台账编制、数据资产登记、授权运营。",
            ),
            retrieved_chunk(
                2,
                "网络安全事件研判步骤包括确认事件类型、判断影响范围和证据保全。",
            ),
        ];

        let citations = citations_for_answer(answer, &context);

        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].source, "eval/documents/doc-1.md");
    }

    #[test]
    fn citations_ignore_chunks_that_match_only_one_generic_short_item() {
        let answer = "根据检索到的资料，答案如下：\n\n1. 数据资源名称\n2. 业务系统\n3. 数据来源\n4. 责任部门\n5. 权属情况\n";
        let context = vec![
            retrieved_chunk(
                1,
                "数据资产台账需要记录数据资源名称、业务系统、数据来源、责任部门、权属情况。",
            ),
            retrieved_chunk(
                2,
                "风险评估材料应说明数据来源、责任人、存储位置和流转路径。",
            ),
        ];

        let citations = citations_for_answer(answer, &context);

        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].source, "eval/documents/doc-1.md");
    }

    #[test]
    fn citations_prefer_stronger_supported_source() {
        let answer = "根据检索到的资料，答案如下：\n\n1. 整改计划必须包含风险描述、影响资产、责任部门、整改措施、临时控制、完成期限、验证方式和复核人。\n2. 高风险问题需要在七个工作日内完成临时控制，在三十个自然日内完成根因整改。\n";
        let context = vec![
            retrieved_chunk(
                1,
                "整改闭环要求明确责任人、整改措施、完成期限、验证方式和复核结论。高风险问题原则上应在七个工作日内完成临时控制，在三十个自然日内完成根因整改。",
            ),
            retrieved_chunk(
                2,
                "整改计划必须包含风险描述、影响资产、责任部门、整改措施、临时控制、完成期限、验证方式和复核人。高风险问题需要在七个工作日内完成临时控制，在三十个自然日内完成根因整改。",
            ),
        ];

        let citations = citations_for_answer(answer, &context);

        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].source, "eval/documents/doc-2.md");
    }

    #[test]
    fn citations_fallback_to_first_context_chunks_when_no_line_matches() {
        let answer = "根据检索到的资料，答案如下：\n\n1. 根据当前资料无法确认\n";
        let context = (0..6)
            .map(|index| retrieved_chunk(index, &format!("第 {index} 个候选上下文")))
            .collect::<Vec<_>>();

        let citations = citations_for_answer(answer, &context);

        assert_eq!(citations.len(), 5);
        assert_eq!(citations[0].chunk_id, "doc-0:0");
        assert_eq!(citations[4].chunk_id, "doc-4:0");
    }

    #[test]
    fn confidence_marks_weak_retrieval_as_low() {
        let context = vec![RetrievedChunk {
            score: 0.12,
            ..retrieved_chunk(1, "弱相关候选上下文")
        }];

        let confidence = confidence_for_context("公司食堂下周菜单有哪些菜？", &context);

        assert_eq!(confidence.level, ConfidenceLevel::Low);
        assert_eq!(confidence.reason, "weak_retrieval_support");
    }

    #[test]
    fn confidence_uses_lexical_support_for_medium_retrieval() {
        let context = vec![RetrievedChunk {
            score: 0.24,
            ..retrieved_chunk(1, "数据出境自评估需要关注出境目的合法性和数据规模。")
        }];

        let confidence = confidence_for_context("数据出境自评估需要关注哪些重点？", &context);

        assert_eq!(confidence.level, ConfidenceLevel::Medium);
    }

    #[test]
    fn insufficient_context_answer_is_localized() {
        let answer = insufficient_context_answer("公司食堂下周菜单有哪些菜？");

        assert!(answer.contains("根据当前资料无法确认"));
    }
}

#[derive(Debug)]
pub enum RagError {
    Database(rusqlite::Error),
    Embed(EmbedError),
    Generate(GenerateError),
}

impl From<rusqlite::Error> for RagError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<GenerateError> for RagError {
    fn from(error: GenerateError) -> Self {
        Self::Generate(error)
    }
}

impl From<EmbedError> for RagError {
    fn from(error: EmbedError) -> Self {
        Self::Embed(error)
    }
}

impl std::fmt::Display for RagError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "{error}"),
            Self::Embed(error) => write!(formatter, "{error}"),
            Self::Generate(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RagError {}
