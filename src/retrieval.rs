use std::collections::HashSet;
use std::sync::Arc;

use crate::embedding::cosine;
use crate::models::RetrievedChunk;
use crate::store::SqliteStore;

pub trait Retriever: Send + Sync {
    fn retrieve(
        &self,
        question: &str,
        query_vector: &[f32],
        embedding_model: &str,
        allowed_scopes: &[String],
        top_k: usize,
    ) -> rusqlite::Result<Vec<RetrievedChunk>>;
}

pub struct SqliteRetriever {
    store: Arc<SqliteStore>,
}

impl SqliteRetriever {
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }
}

impl Retriever for SqliteRetriever {
    fn retrieve(
        &self,
        question: &str,
        query_vector: &[f32],
        embedding_model: &str,
        allowed_scopes: &[String],
        top_k: usize,
    ) -> rusqlite::Result<Vec<RetrievedChunk>> {
        let dimensions = query_vector.len();
        let chunks_with_vectors =
            self.store
                .load_chunks_with_embeddings(embedding_model, dimensions, allowed_scopes)?;

        // Detect vector score spread to adapt scoring weights
        let vector_scores: Vec<f32> = chunks_with_vectors
            .iter()
            .map(|(_, v)| cosine(query_vector, v))
            .collect();
        let v_min = vector_scores.iter().copied().fold(f32::MAX, f32::min);
        let v_max = vector_scores.iter().copied().fold(f32::MIN, f32::max);
        let vector_spread = v_max - v_min;
        // Spread > 0.4 means discriminative embeddings (Ollama); <= 0.4 means clustered (hash)
        let vector_weight_factor = if vector_spread > 0.4 { 2.0 } else { 1.0 };

        let query_ctx = PrecomputedQuery::new(question);

        let mut scored: Vec<RetrievedChunk> = chunks_with_vectors
            .into_iter()
            .zip(vector_scores.iter())
            .map(|((chunk, _vector), &vector_score)| {
                let text_score = lexical_score_with_query(&query_ctx, &chunk.text);
                let title_score =
                    title_boost_with_query(&query_ctx, &chunk.document_title, &chunk.section);
                let final_score = combined_score_adaptive(
                    vector_score,
                    text_score,
                    title_score,
                    vector_weight_factor,
                );
                RetrievedChunk {
                    score: final_score,
                    chunk,
                }
            })
            .collect();

        scored.sort_by(|left, right| right.score.total_cmp(&left.score));
        scored.truncate(top_k);
        Ok(scored)
    }
}

fn combined_score_adaptive(
    vector_score: f32,
    lexical_score: f32,
    title_score: f32,
    vwf: f32,
) -> f32 {
    let total_lexical = lexical_score + title_score;
    if title_score >= 0.7 {
        // Strong title match: trust title/lexical heavily
        vector_score.mul_add(0.05, total_lexical * 0.95)
    } else if title_score >= 0.4 {
        // Good title match
        let vw = (0.15 * vwf).min(0.45);
        vector_score.mul_add(vw, total_lexical * (1.0 - vw))
    } else if total_lexical >= 0.6 {
        // Strong lexical match
        let vw = (0.25 * vwf).min(0.55);
        vector_score.mul_add(vw, total_lexical * (1.0 - vw))
    } else if total_lexical >= 0.3 {
        // Moderate lexical: balanced
        let vw = (0.4 * vwf).min(0.65);
        vector_score.mul_add(vw, total_lexical * (1.0 - vw))
    } else if total_lexical > 0.0 {
        // Weak lexical: trust vector more
        let vw = (0.55 * vwf).min(0.8);
        vector_score.mul_add(vw, total_lexical * (1.0 - vw))
    } else {
        // No lexical match: vector only
        vector_score * (0.7 * vwf).min(0.95)
    }
}

struct PrecomputedQuery {
    terms: Vec<String>,
    technical_terms: Vec<String>,
    anchors: Vec<String>,
    cjk_keywords: Vec<String>,
    subject_before_goal: Option<String>,
    active_type_patterns: Vec<(&'static str, f32)>,
}

impl PrecomputedQuery {
    fn new(question: &str) -> Self {
        let terms = lexical_terms(question);
        let technical_terms = terms
            .iter()
            .filter(|term| term.len() >= 3 && term.is_ascii())
            .cloned()
            .collect();
        let question_compact = compact_cjk_and_ascii(question);
        let anchors = answer_anchor_phrases(&question_compact);
        let cjk_keywords = extract_cjk_keywords(&question_compact);
        let subject_before_goal = subject_before_goal(&question_compact);

        const PATTERNS: &[(&str, f32)] = &[
            ("负责哪些", 0.3),
            ("分为哪些阶段", 0.3),
            ("能力域", 0.2),
            ("成熟度等级", 0.2),
            ("评估方法", 0.2),
            ("改进路径", 0.2),
            ("整改闭环", 0.3),
            ("应急响应", 0.3),
            ("备份策略", 0.3),
            ("备份恢复", 0.3),
            ("脱敏", 0.2),
            ("跨境", 0.2),
            ("出境", 0.2),
            ("数据质量", 0.2),
            ("风险评估", 0.2),
            ("embedding", 0.3),
            ("知识库", 0.2),
            ("准入指标", 0.3),
            ("发布流程", 0.3),
            ("响应时间", 0.3),
            ("事件升级", 0.3),
            ("事后回顾", 0.3),
            ("数据校验", 0.3),
            ("会计核算", 0.3),
            ("敏感个人信息", 0.3),
            ("合法性基础", 0.3),
            ("必填字段", 0.3),
            ("采集方式", 0.3),
            ("RTO", 0.3),
            ("RPO", 0.3),
            ("灾备", 0.3),
            ("恢复组织", 0.3),
            ("设备准入", 0.3),
            ("网络分区", 0.3),
            ("Purdue", 0.3),
        ];

        let active_type_patterns = PATTERNS
            .iter()
            .filter(|(pattern, _)| question_compact.contains(*pattern))
            .copied()
            .collect();

        Self {
            terms,
            technical_terms,
            anchors,
            cjk_keywords,
            subject_before_goal,
            active_type_patterns,
        }
    }
}

fn title_boost_with_query(query: &PrecomputedQuery, title: &str, section: &str) -> f32 {
    if query.terms.is_empty() {
        return 0.0;
    }

    let title_compact = compact_cjk_and_ascii(title);
    let section_compact = compact_cjk_and_ascii(section);
    let title_lower = title_compact.to_lowercase();
    let section_lower = section_compact.to_lowercase();

    let title_matches = query
        .terms
        .iter()
        .filter(|term| title_lower.contains(term.as_str()))
        .count();
    let section_matches = query
        .terms
        .iter()
        .filter(|term| section_lower.contains(term.as_str()))
        .count();

    let title_ratio = title_matches as f32 / query.terms.len() as f32;
    let section_ratio = section_matches as f32 / query.terms.len() as f32;

    let mut boost = 0.0;
    if title_ratio > 0.1 {
        boost += 0.3;
    }
    if title_ratio > 0.3 {
        boost += 0.2;
    }
    if section_ratio > 0.2 {
        boost += 0.25;
    } else if section_ratio > 0.1 {
        boost += 0.15;
    }

    let technical_title_hits = query
        .technical_terms
        .iter()
        .filter(|term| title_lower.contains(term.as_str()))
        .count();
    if technical_title_hits > 0 && technical_title_hits == query.technical_terms.len() {
        boost += 0.4;
    }

    if query
        .anchors
        .iter()
        .any(|anchor| title_lower.contains(anchor.as_str()))
    {
        boost += 0.3;
    }

    let title_keyword_hits = query
        .cjk_keywords
        .iter()
        .filter(|kw| title_lower.contains(kw.as_str()))
        .count();
    if title_keyword_hits > 0 {
        boost += 0.2 * title_keyword_hits as f32;
    }

    boost
}

#[cfg(test)]
fn title_boost(question: &str, title: &str, section: &str) -> f32 {
    let query = PrecomputedQuery::new(question);
    title_boost_with_query(&query, title, section)
}

/// Extract 4+ character CJK keywords from text
fn extract_cjk_keywords(text: &str) -> Vec<String> {
    let mut keywords = Vec::new();
    let mut cjk_buf: Vec<char> = Vec::new();

    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            cjk_buf.push(ch);
        } else {
            if cjk_buf.len() >= 3 {
                let keyword: String = cjk_buf.iter().collect();
                keywords.push(keyword);
            }
            cjk_buf.clear();
        }
    }

    if cjk_buf.len() >= 3 {
        let keyword: String = cjk_buf.iter().collect();
        keywords.push(keyword);
    }

    keywords
}

fn lexical_score_with_query(query: &PrecomputedQuery, text: &str) -> f32 {
    if query.terms.is_empty() {
        return 0.0;
    }

    let text_lower = text.to_lowercase();
    let matches = query
        .terms
        .iter()
        .filter(|term| text_lower.contains(term.as_str()))
        .count();
    let score = matches as f32 / query.terms.len() as f32;
    (score + semantic_phrase_bonus_with_query(query, &text_lower)).min(1.0)
}

#[cfg(test)]
fn lexical_score(question: &str, text: &str) -> f32 {
    let query = PrecomputedQuery::new(question);
    lexical_score_with_query(&query, text)
}

fn semantic_phrase_bonus_with_query(query: &PrecomputedQuery, text_lower: &str) -> f32 {
    let mut bonus = 0.0f32;

    for anchor in &query.anchors {
        if text_lower.contains(anchor.as_str()) {
            bonus += 0.4;
        }
    }

    let text_compact = compact_cjk_and_ascii(text_lower);

    if let Some(subject) = &query.subject_before_goal {
        if text_compact.contains(subject) {
            bonus += 0.2;
        }
        if text_lower.contains(&format!("{subject}的目标"))
            || text_lower.contains(&format!("{subject}目标"))
            || text_lower.contains(&format!("{subject}的目的"))
            || text_lower.contains(&format!("{subject}目的"))
        {
            bonus += 0.3;
        }
    }

    for (pattern, weight) in &query.active_type_patterns {
        if text_compact.contains(pattern) {
            bonus += weight;
        }
    }

    bonus
}

fn answer_anchor_phrases(question: &str) -> Vec<String> {
    let mut anchors = Vec::new();
    for (question_suffix, answer_suffix) in [
        ("需要评估哪些内容", "需要评估"),
        ("需要检查哪些事项", "需要检查"),
        ("应明确哪些事项", "应明确"),
        ("应覆盖哪些类型", "应覆盖"),
        ("应覆盖哪些内容", "应覆盖"),
        ("需要包含哪些维度", "需要包含"),
        ("应记录哪些字段", "应记录"),
        ("通常包括哪些", "通常包括"),
        ("包括哪些步骤", "包括以下步骤"),
        ("包括哪些阶段", "包括以下阶段"),
        ("包括哪些类型", "包括以下类型"),
        ("包括哪些维度", "包括以下维度"),
        ("包括哪些内容", "包括以下内容"),
        ("包括哪些", "包括"),
        ("分为哪些阶段", "分为以下几个阶段"),
        ("分为哪些等级", "分为以下几个等级"),
        ("分为哪些级别", "分为以下几个级别"),
        ("分为哪些", "分为"),
        ("需要检查哪些内容", "检查内容包括"),
        ("需要记录哪些内容", "需要记录"),
        ("应关注哪些内容", "应关注"),
        ("时应关注什么", "时应关注"),
        ("负责哪些事项", "负责"),
        ("应满足哪些要求", "应满足以下要求"),
        ("应包含哪些要求", "应包含以下要求"),
        ("应实施哪些", "应实施以下"),
        ("应检测哪些", "应检测以下"),
        ("应开展哪些", "应开展以下"),
        ("基于哪些核心原则", "基于以下核心原则"),
        ("基于哪些原则", "基于以下原则"),
        ("采用哪些方式", "采用以下方式"),
        ("分为哪些类别", "分为以下类别"),
        ("满足哪些条件", "满足以下条件"),
        ("遵循哪些原则", "遵循以下原则"),
        ("应进行哪些校验", "应进行以下校验"),
    ] {
        if let Some(subject) = question.strip_suffix(question_suffix)
            && subject.chars().count() >= 3
        {
            anchors.push(format!("{subject}{answer_suffix}"));
        }
    }

    anchors
}

fn compact_cjk_and_ascii(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || is_cjk(*ch))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn subject_before_goal(question: &str) -> Option<String> {
    let marker_index = question.find("目标").or_else(|| question.find("目的"))?;
    let mut subject = question[..marker_index].to_string();

    for suffix in ["的主要", "主要", "的核心", "核心", "的"] {
        if subject.ends_with(suffix) {
            let keep = subject.len() - suffix.len();
            subject.truncate(keep);
            break;
        }
    }

    let subject = subject
        .trim_matches(|ch: char| !is_cjk(ch))
        .trim_start_matches("请问")
        .trim_start_matches("说明")
        .trim_start_matches("介绍")
        .to_string();

    (subject.chars().count() >= 2).then_some(subject)
}

fn lexical_terms(text: &str) -> Vec<String> {
    let mut terms = HashSet::new();
    let mut ascii = String::new();
    let mut cjk = Vec::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            flush_cjk_terms(&mut terms, &mut cjk);
            ascii.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_ascii_term(&mut terms, &mut ascii);
            cjk.push(ch);
        } else {
            flush_ascii_term(&mut terms, &mut ascii);
            flush_cjk_terms(&mut terms, &mut cjk);
        }
    }

    flush_ascii_term(&mut terms, &mut ascii);
    flush_cjk_terms(&mut terms, &mut cjk);

    let mut terms: Vec<String> = terms.into_iter().collect();
    terms.sort();
    terms
}

fn flush_ascii_term(terms: &mut HashSet<String>, ascii: &mut String) {
    if ascii.len() >= 2 {
        terms.insert(std::mem::take(ascii));
    } else {
        ascii.clear();
    }
}

fn flush_cjk_terms(terms: &mut HashSet<String>, cjk: &mut Vec<char>) {
    for width in 2..=3 {
        if cjk.len() < width {
            continue;
        }
        for window in cjk.windows(width) {
            terms.insert(window.iter().collect());
        }
    }
    cjk.clear();
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{f900}'..='\u{faff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_score_handles_ascii_queries() {
        let question = "How does RAG work?";
        let relevant = "RAG retrieves context and generates answers.";
        let unrelated = "The weather is nice today.";

        assert!(lexical_score(question, relevant) > lexical_score(question, unrelated));
    }

    #[test]
    fn lexical_score_handles_chinese_queries() {
        let question = "数据安全治理的主要目标是什么？";
        let relevant = "数据安全治理的主要目标是降低数据泄露风险并满足监管要求。";
        let generic = "网络数据安全风险评估应覆盖数据处理目的、处理方式、数据规模和安全措施。";

        assert!(lexical_score(question, relevant) > lexical_score(question, generic));
    }

    #[test]
    fn title_boost_gives_bonus_for_matching_title() {
        let question = "企业选择 embedding 模型需要检查哪些事项？";
        let matching_title = "企业知识库 Embedding 模型治理规范（样例）";
        let unrelated_title = "企业数据跨境流动合规路径判定指引（样例）";

        assert!(
            title_boost(question, matching_title, "") > title_boost(question, unrelated_title, "")
        );
    }

    #[test]
    fn lexical_score_boosts_exact_answer_anchors() {
        let question = "数据安全治理的主要目标是什么？";
        let relevant = "数据安全治理的主要目标是降低数据泄露风险并满足监管要求。";
        let generic = "数据安全治理委员会负责审批年度治理目标、数据安全预算和重大制度变更。";

        assert!(lexical_score(question, relevant) > lexical_score(question, generic));
    }

    #[test]
    fn lexical_score_boosts_chinese_goal_phrases() {
        let question = "数据安全治理的主要目标是什么？";
        let relevant = "数据安全治理的主要目标是降低数据泄露风险并满足监管要求。";
        let generic = "网络数据安全风险评估应覆盖数据处理目的、处理方式、数据规模和安全措施。";

        assert!(lexical_score(question, relevant) > lexical_score(question, generic));
    }

    #[test]
    fn lexical_score_boosts_embedding_model_selection_anchor() {
        let question = "企业选择 embedding 模型需要检查哪些事项？";
        let relevant = "企业选择 embedding 模型需要检查语义检索效果、中文行业术语覆盖、向量维度稳定性、批量嵌入吞吐、离线部署能力和商用授权。";
        let generic =
            "跨境合规路径选择因素包括接收方数量、业务稳定性、数据主体规模和后续变更频率。";

        assert!(lexical_score(question, relevant) > lexical_score(question, generic));
    }

    #[test]
    fn lexical_score_boosts_acceptance_testing_type_anchor() {
        let question = "商业化知识库验收测试应覆盖哪些类型？";
        let relevant = "商业化知识库验收测试应覆盖文档接入测试、格式解析测试、切分质量测试、语义检索测试和回答完整性测试。";
        let generic = "embedding 模型变更发布流程包括运行标准评测集和业务高频问题集。";

        assert!(lexical_score(question, relevant) > lexical_score(question, generic));
    }
}
