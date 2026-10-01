use serde::Deserialize;

use crate::config::{ChatCompletionsProviderConfig, ClaudeMessagesProviderConfig};
use crate::models::RetrievedChunk;

pub trait QueryPlanner: Send + Sync {
    fn plan(&self, question: &str) -> Result<AgentPlan, AgentError>;
    fn plan_follow_up(
        &self,
        question: &str,
        evidence: &[RetrievedChunk],
    ) -> Result<AgentPlan, AgentError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentPlan {
    search_queries: Vec<String>,
}

impl AgentPlan {
    pub fn new(question: &str, search_queries: Vec<String>) -> Self {
        let mut plan = Self { search_queries };
        plan.sanitize(question);
        plan
    }

    pub fn fallback(question: &str) -> Self {
        Self::new(question, Vec::new())
    }

    pub fn from_queries(search_queries: Vec<String>) -> Self {
        let mut plan = Self { search_queries };
        plan.sanitize_without_seed();
        plan
    }

    pub fn search_queries(&self) -> &[String] {
        &self.search_queries
    }

    fn sanitize(&mut self, question: &str) {
        self.sanitize_with_seed(Some(question));
    }

    fn sanitize_without_seed(&mut self) {
        self.sanitize_with_seed(None);
    }

    fn sanitize_with_seed(&mut self, seed: Option<&str>) {
        const MAX_QUERIES: usize = 4;
        const MAX_QUERY_CHARS: usize = 120;

        let mut queries = Vec::new();
        if let Some(seed) = seed {
            push_unique_query(&mut queries, seed, MAX_QUERY_CHARS);
        }
        for query in &self.search_queries {
            push_unique_query(&mut queries, query, MAX_QUERY_CHARS);
            if queries.len() >= MAX_QUERIES {
                break;
            }
        }
        self.search_queries = queries;
    }
}

fn push_unique_query(queries: &mut Vec<String>, query: &str, max_chars: usize) {
    let query = compact_query(query, max_chars);
    if query.is_empty() || queries.iter().any(|existing| existing == &query) {
        return;
    }
    queries.push(query);
}

fn compact_query(query: &str, max_chars: usize) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

pub struct ChatCompletionsPlanner {
    config: ChatCompletionsProviderConfig,
    client: reqwest::blocking::Client,
}

pub struct ClaudeMessagesPlanner {
    config: ClaudeMessagesProviderConfig,
    client: reqwest::blocking::Client,
}

impl ClaudeMessagesPlanner {
    pub fn new(config: ClaudeMessagesProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl ChatCompletionsPlanner {
    pub fn new(config: ChatCompletionsProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl QueryPlanner for ClaudeMessagesPlanner {
    fn plan(&self, question: &str) -> Result<AgentPlan, AgentError> {
        let response = self.request_plan(
            "You are a RAG query planner. Return JSON only, with the schema {\"search_queries\":[\"...\"]}. Produce 2-3 short search queries in the user's language. Do not answer the question.",
            question,
        )?;
        Ok(AgentPlan::new(question, response.search_queries))
    }

    fn plan_follow_up(
        &self,
        question: &str,
        evidence: &[RetrievedChunk],
    ) -> Result<AgentPlan, AgentError> {
        let prompt = build_follow_up_prompt(question, evidence);
        let response = self.request_plan(
            "You are a RAG evidence expansion planner. Return JSON only, with the schema {\"search_queries\":[\"...\"]}. Inspect the retrieved evidence and create short follow-up search queries for referenced concepts, sections, figures, tables, policies, or named items that are needed to fully answer the question. Return an empty array if no follow-up search is needed. Do not answer the question.",
            &prompt,
        )?;
        Ok(AgentPlan::from_queries(response.search_queries))
    }
}

impl QueryPlanner for ChatCompletionsPlanner {
    fn plan(&self, question: &str) -> Result<AgentPlan, AgentError> {
        let response = self.request_plan(
            "You are a RAG query planner. Return JSON only, with the schema {\"search_queries\":[\"...\"]}. Produce 2-3 short search queries in the user's language. Do not answer the question.",
            question,
        )?;
        Ok(AgentPlan::new(question, response.search_queries))
    }

    fn plan_follow_up(
        &self,
        question: &str,
        evidence: &[RetrievedChunk],
    ) -> Result<AgentPlan, AgentError> {
        let prompt = build_follow_up_prompt(question, evidence);
        let response = self.request_plan(
            "You are a RAG evidence expansion planner. Return JSON only, with the schema {\"search_queries\":[\"...\"]}. Inspect the retrieved evidence and create short follow-up search queries for referenced concepts, sections, figures, tables, policies, or named items that are needed to fully answer the question. Return an empty array if no follow-up search is needed. Do not answer the question.",
            &prompt,
        )?;
        Ok(AgentPlan::from_queries(response.search_queries))
    }
}

impl ChatCompletionsPlanner {
    fn request_plan(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<PlannerResponse, AgentError> {
        let mut request =
            self.client()
                .post(self.config.endpoint_url())
                .json(&ChatCompletionRequest {
                    model: &self.config.model,
                    messages: vec![
                        ChatMessage {
                            role: "system",
                            content: system_prompt,
                        },
                        ChatMessage {
                            role: "user",
                            content: user_prompt,
                        },
                    ],
                    temperature: 0.0,
                });

        request = apply_provider_headers(request, &self.config);

        let response: ChatCompletionResponse = request
            .send()
            .map_err(|error| AgentError::Provider(error.to_string()))?
            .error_for_status()
            .map_err(|error| AgentError::Provider(error.to_string()))?
            .json()
            .map_err(|error| AgentError::Provider(error.to_string()))?;

        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| AgentError::Provider("empty planner response".to_string()))?;

        parse_planner_response(&content)
    }
}

impl ClaudeMessagesPlanner {
    fn request_plan(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<PlannerResponse, AgentError> {
        let mut request = self
            .client()
            .post(self.config.endpoint_url())
            .header("anthropic-version", self.config.anthropic_version.as_str())
            .json(&ClaudeMessagesRequest {
                model: &self.config.model,
                max_tokens: self.config.max_tokens,
                system: system_prompt,
                messages: vec![ClaudeMessage {
                    role: "user",
                    content: user_prompt,
                }],
            });

        request = apply_claude_headers(request, &self.config);

        let response: ClaudeMessagesResponse = request
            .send()
            .map_err(|error| AgentError::Provider(error.to_string()))?
            .error_for_status()
            .map_err(|error| AgentError::Provider(error.to_string()))?
            .json()
            .map_err(|error| AgentError::Provider(error.to_string()))?;

        let content = extract_claude_text(response)
            .ok_or_else(|| AgentError::Provider("empty Claude planner response".to_string()))?;

        parse_planner_response(&content)
    }
}

impl ChatCompletionsPlanner {
    fn client(&self) -> &reqwest::blocking::Client {
        &self.client
    }
}

impl ClaudeMessagesPlanner {
    fn client(&self) -> &reqwest::blocking::Client {
        &self.client
    }
}

fn apply_provider_headers(
    mut request: reqwest::blocking::RequestBuilder,
    config: &ChatCompletionsProviderConfig,
) -> reqwest::blocking::RequestBuilder {
    if let Some(api_key) = &config.api_key {
        let value = match &config.auth_scheme {
            Some(scheme) => format!("{scheme} {api_key}"),
            None => api_key.clone(),
        };
        request = request.header(config.auth_header.as_str(), value);
    }

    for (name, value) in &config.extra_headers {
        request = request.header(name.as_str(), value.as_str());
    }

    request
}

fn apply_claude_headers(
    mut request: reqwest::blocking::RequestBuilder,
    config: &ClaudeMessagesProviderConfig,
) -> reqwest::blocking::RequestBuilder {
    if let Some(api_key) = &config.api_key {
        request = request.header("x-api-key", api_key.as_str());
    }

    for (name, value) in &config.extra_headers {
        request = request.header(name.as_str(), value.as_str());
    }

    request
}

fn extract_claude_text(response: ClaudeMessagesResponse) -> Option<String> {
    let text = response
        .content
        .into_iter()
        .filter_map(|item| item.text)
        .collect::<Vec<_>>()
        .join("");
    (!text.is_empty()).then_some(text)
}

fn build_follow_up_prompt(question: &str, evidence: &[RetrievedChunk]) -> String {
    const MAX_EVIDENCE_CHUNKS: usize = 8;
    const MAX_CHUNK_CHARS: usize = 700;

    let mut prompt = String::new();
    prompt.push_str("Question:\n");
    prompt.push_str(question);
    prompt.push_str("\n\nRetrieved evidence:\n");

    for (index, item) in evidence.iter().take(MAX_EVIDENCE_CHUNKS).enumerate() {
        prompt.push_str(&format!(
            "[{}] title={} section={} source={} chunk_id={} score={:.4}\n{}\n\n",
            index + 1,
            item.chunk.document_title,
            item.chunk.section,
            item.chunk.source,
            item.chunk.id,
            item.score,
            trim_chars(&item.chunk.text, MAX_CHUNK_CHARS)
        ));
    }

    prompt.push_str("Return only follow-up search queries for referenced content that should be retrieved next.");
    prompt
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    let mut trimmed = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        trimmed.push_str("...");
    }
    trimmed
}

fn parse_planner_response(content: &str) -> Result<PlannerResponse, AgentError> {
    let json = extract_json_object(content)
        .ok_or_else(|| AgentError::InvalidPlan("planner response did not contain JSON".into()))?;
    serde_json::from_str(json).map_err(|error| AgentError::InvalidPlan(error.to_string()))
}

fn extract_json_object(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (start < end).then_some(&trimmed[start..=end])
}

#[derive(Deserialize)]
struct PlannerResponse {
    search_queries: Vec<String>,
}

#[derive(serde::Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
}

#[derive(serde::Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct ClaudeMessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<ClaudeMessage<'a>>,
}

#[derive(serde::Serialize)]
struct ClaudeMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Deserialize)]
struct ClaudeMessagesResponse {
    content: Vec<ClaudeContentBlock>,
}

#[derive(Deserialize)]
struct ClaudeContentBlock {
    text: Option<String>,
}

#[derive(Debug)]
pub enum AgentError {
    Provider(String),
    InvalidPlan(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(message) => write!(formatter, "agent provider error: {message}"),
            Self::InvalidPlan(message) => write!(formatter, "invalid agent plan: {message}"),
        }
    }
}

impl std::error::Error for AgentError {}

#[cfg(test)]
mod tests {
    use crate::models::{Chunk, RetrievedChunk};

    use super::{
        AgentPlan, ClaudeContentBlock, ClaudeMessagesResponse, build_follow_up_prompt,
        extract_claude_text, parse_planner_response,
    };

    #[test]
    fn agent_plan_keeps_original_question_first() {
        let plan = AgentPlan::new(
            "数据安全治理的主要目标是什么？",
            vec![
                "数据安全治理目标".to_string(),
                "数据安全治理的主要目标是什么？".to_string(),
                "业务持续健康发展 数据安全".to_string(),
            ],
        );

        assert_eq!(
            plan.search_queries(),
            &[
                "数据安全治理的主要目标是什么？",
                "数据安全治理目标",
                "业务持续健康发展 数据安全"
            ]
        );
    }

    #[test]
    fn planner_response_accepts_fenced_json() {
        let parsed = parse_planner_response(
            "```json\n{\"search_queries\":[\"数据安全治理目标\",\"合规保障 风险管理\"]}\n```",
        )
        .expect("parse planner response");

        assert_eq!(parsed.search_queries.len(), 2);
        assert_eq!(parsed.search_queries[0], "数据安全治理目标");
    }

    #[test]
    fn claude_planner_response_text_blocks_are_joined() {
        let text = extract_claude_text(ClaudeMessagesResponse {
            content: vec![
                ClaudeContentBlock {
                    text: Some("{\"search_queries\":[".to_string()),
                },
                ClaudeContentBlock {
                    text: Some("\"数据安全治理\"]}".to_string()),
                },
            ],
        })
        .expect("extract text");

        assert_eq!(text, "{\"search_queries\":[\"数据安全治理\"]}");
    }

    #[test]
    fn follow_up_plan_can_be_empty_without_original_question() {
        let plan = AgentPlan::from_queries(Vec::new());

        assert!(plan.search_queries().is_empty());
    }

    #[test]
    fn follow_up_prompt_includes_retrieved_references() {
        let evidence = vec![RetrievedChunk {
            score: 0.8,
            chunk: Chunk {
                id: "doc:10".to_string(),
                document_id: "doc".to_string(),
                chunk_index: 10,
                document_title: "测试文档".to_string(),
                section: "参考框架".to_string(),
                text: "参考框架包括数据安全战略、数据全生命周期安全、基础安全三部分，如图 2 所示。"
                    .to_string(),
                source: "测试文档".to_string(),
                access_scope: crate::models::default_access_scope(),
            },
        }];

        let prompt = build_follow_up_prompt("数据安全治理参考框架是什么？", &evidence);

        assert!(prompt.contains("图 2"));
        assert!(prompt.contains("数据全生命周期安全"));
    }
}
