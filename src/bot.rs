use std::collections::HashMap;
use std::sync::Arc;

use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentError, extract_json_object};
use crate::config::{ChatCompletionsProviderConfig, ClaudeMessagesProviderConfig};
use crate::contracts::BotChatResponse;
use crate::models::{AnswerConfidence, ConfidenceLevel, allowed_access_scopes};
use crate::rag::{RagError, RagService};
use crate::skills::{
    DataSourceManager, QueryResult, SkillError, SqlSkill, SqlSkillRegistry,
    format_query_result_markdown,
};

#[derive(Debug)]
pub enum BotError {
    Rag(RagError),
    Skill(SkillError),
    Internal(String),
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rag(err) => write!(f, "RAG error: {err}"),
            Self::Skill(err) => write!(f, "Skill error: {err}"),
            Self::Internal(msg) => write!(f, "Bot internal error: {msg}"),
        }
    }
}

impl std::error::Error for BotError {}

impl From<RagError> for BotError {
    fn from(err: RagError) -> Self {
        Self::Rag(err)
    }
}

impl From<SkillError> for BotError {
    fn from(err: SkillError) -> Self {
        Self::Skill(err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BotAction {
    Skill {
        skill_name: String,
        #[serde(default)]
        parameters: HashMap<String, serde_json::Value>,
    },
    Rag {
        query: String,
    },
    Direct {
        response: String,
    },
}

pub trait BotPlanner: Send + Sync {
    fn plan(&self, message: &str, skills: &[&SqlSkill]) -> Result<BotAction, AgentError>;
}

pub struct ChatCompletionsBotPlanner {
    config: ChatCompletionsProviderConfig,
    client: reqwest::blocking::Client,
}

impl ChatCompletionsBotPlanner {
    pub fn new(config: ChatCompletionsProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl BotPlanner for ChatCompletionsBotPlanner {
    fn plan(&self, message: &str, skills: &[&SqlSkill]) -> Result<BotAction, AgentError> {
        let system_prompt = build_bot_system_prompt(skills);
        let mut request = self
            .client
            .post(self.config.endpoint_url())
            .json(&serde_json::json!({
                "model": self.config.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": message}
                ],
                "temperature": 0.1
            }));

        if let Some(api_key) = &self.config.api_key {
            let auth_value = match &self.config.auth_scheme {
                Some(scheme) => format!("{scheme} {api_key}"),
                None => api_key.clone(),
            };
            request = request.header(self.config.auth_header.as_str(), auth_value);
        }

        for (name, val) in &self.config.extra_headers {
            request = request.header(name.as_str(), val.as_str());
        }

        let resp: serde_json::Value = request
            .send()
            .map_err(|e| AgentError::Provider(e.to_string()))?
            .error_for_status()
            .map_err(|e| AgentError::Provider(e.to_string()))?
            .json()
            .map_err(|e| AgentError::Provider(e.to_string()))?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AgentError::Provider("Empty LLM response".into()))?;

        parse_bot_action(content)
    }
}

pub struct ClaudeMessagesBotPlanner {
    config: ClaudeMessagesProviderConfig,
    client: reqwest::blocking::Client,
}

impl ClaudeMessagesBotPlanner {
    pub fn new(config: ClaudeMessagesProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl BotPlanner for ClaudeMessagesBotPlanner {
    fn plan(&self, message: &str, skills: &[&SqlSkill]) -> Result<BotAction, AgentError> {
        let system_prompt = build_bot_system_prompt(skills);
        let mut request = self
            .client
            .post(self.config.endpoint_url())
            .header("anthropic-version", self.config.anthropic_version.as_str())
            .json(&serde_json::json!({
                "model": self.config.model,
                "max_tokens": self.config.max_tokens,
                "system": system_prompt,
                "messages": [
                    {"role": "user", "content": message}
                ]
            }));

        if let Some(api_key) = &self.config.api_key {
            request = request.header("x-api-key", api_key.as_str());
        }

        for (name, val) in &self.config.extra_headers {
            request = request.header(name.as_str(), val.as_str());
        }

        let resp: serde_json::Value = request
            .send()
            .map_err(|e| AgentError::Provider(e.to_string()))?
            .error_for_status()
            .map_err(|e| AgentError::Provider(e.to_string()))?
            .json()
            .map_err(|e| AgentError::Provider(e.to_string()))?;

        let content = resp["content"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
                    .into()
            })
            .map(|parts: Vec<&str>| parts.join(""))
            .unwrap_or_default();

        parse_bot_action(&content)
    }
}

fn build_bot_system_prompt(skills: &[&SqlSkill]) -> String {
    let mut skills_text = String::new();
    if skills.is_empty() {
        skills_text.push_str("（当前暂未配置任何 SQL 技能）\n");
    } else {
        for skill in skills {
            skills_text.push_str(&format!(
                "- 技能名称: {}\n  描述: {}\n  参数:\n",
                skill.name, skill.description
            ));
            for param in &skill.parameters {
                let req_text = if param.required { "必填" } else { "选填" };
                skills_text.push_str(&format!(
                    "    * {} (类型: {}, {}): {}\n",
                    param.name, param.param_type, req_text, param.description
                ));
            }
        }
    }

    format!(
        r#"你是企业内部智能平台服务机器人。你拥有两大能力：
1. 平台文档与知识库问答 (RAG)：回答平台使用方法、架构规范、开发指南、FAQ、流程与政策。
2. 预定义业务数据查询技能 (SQL Skills)：调用预设模板查询实时平台数据（如用户状态、服务指标、订单数据等）。

可用技能列表：
{skills_text}

根据用户的输入，分析其真实意图并做出路由判断：
- 如果用户意图是查询实时业务数据且有匹配的技能，返回 JSON:
  {{"action":"skill","skill_name":"<技能名>","parameters":{{"<参数名>":<提取值>}}}}
- 如果用户意图是询问平台使用方法、概念、流程、常见问题或未匹配到数据技能，返回 JSON:
  {{"action":"rag","query":"<提取并优化后的检索关键词>"}}
- 如果用户输入为简单问候（如“你好”、“在吗”）、功能咨询（“你能做什么”、“你现在知道什么”）、能力介绍或无须查询的闲聊，返回 JSON:
  {{"action":"direct","response":"<礼貌热情地回答用户并简述你的两大能力>"}}

必须严格仅返回一个合法 JSON 对象，不要输出 markdown 代码块标记，不要包含其他文字。"#
    )
}

fn parse_bot_action(content: &str) -> Result<BotAction, AgentError> {
    let json_str = extract_json_object(content).ok_or_else(|| {
        AgentError::InvalidPlan("Bot planner response did not contain JSON".into())
    })?;
    serde_json::from_str(json_str).map_err(|e| AgentError::InvalidPlan(format!("{e}: {json_str}")))
}

fn is_capabilities_or_greeting_query(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let exact_greetings = ["你好", "您好", "hello", "hi", "在吗", "帮助", "help"];
    if exact_greetings.iter().any(|&g| {
        lower == g
            || lower == format!("{g}呀")
            || lower == format!("{g}啊")
            || lower == format!("{g}！")
            || lower == format!("{g}!")
    }) {
        return true;
    }

    let capability_patterns = [
        "知道什么",
        "知道些什么",
        "懂什么",
        "能做什么",
        "你会什么",
        "你能干什么",
        "有哪些技能",
        "有哪些功能",
        "支持什么",
        "能查什么",
        "可以查什么",
        "介绍一下",
        "自我介绍",
        "有什么功能",
        "有什么技能",
        "知识库有什么",
        "有哪些知识",
        "你是什么",
        "你的功能",
        "使用帮助",
    ];

    capability_patterns
        .iter()
        .any(|&pattern| lower.contains(pattern))
}

/// Fallback rule-based router when LLM is unavailable or in extractive mode
pub fn rule_based_route(message: &str, skills: &[&SqlSkill]) -> BotAction {
    let trimmed = message.trim();

    // Check greetings / bot capabilities query
    if is_capabilities_or_greeting_query(trimmed) {
        return BotAction::Direct {
            response: String::new(),
        };
    }

    // Match skills by name or keywords
    for skill in skills {
        let name_matched = trimmed.contains(&skill.name);
        let keyword_matched = skill
            .keywords
            .iter()
            .any(|kw| !kw.is_empty() && trimmed.contains(kw));

        if name_matched || keyword_matched {
            let mut parameters = HashMap::new();
            for param in &skill.parameters {
                if param.param_type == "integer" {
                    if let Some(num) = extract_number_from_text(trimmed) {
                        parameters.insert(param.name.clone(), serde_json::json!(num));
                    } else if let Some(def) = &param.default {
                        parameters.insert(param.name.clone(), def.clone());
                    }
                } else if param.param_type == "string" {
                    if let Some(val) = extract_identifier_from_text(trimmed, &skill.keywords) {
                        parameters.insert(param.name.clone(), serde_json::json!(val));
                    } else if let Some(def) = &param.default {
                        parameters.insert(param.name.clone(), def.clone());
                    }
                } else if let Some(def) = &param.default {
                    parameters.insert(param.name.clone(), def.clone());
                }
            }

            return BotAction::Skill {
                skill_name: skill.name.clone(),
                parameters,
            };
        }
    }

    // Default to RAG knowledge query
    BotAction::Rag {
        query: trimmed.to_string(),
    }
}

fn extract_number_from_text(text: &str) -> Option<i64> {
    let mut current_digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            current_digits.push(c);
        } else if !current_digits.is_empty() {
            break;
        }
    }
    current_digits.parse::<i64>().ok()
}

fn extract_identifier_from_text(text: &str, exclude_keywords: &[String]) -> Option<String> {
    // Check for quoted tokens: "..." or '...' or “...”
    for (start_quote, end_quote) in [('"', '"'), ('\'', '\''), ('“', '”'), ('‘', '’')] {
        if let Some(start) = text.find(start_quote)
            && let Some(end) = text[start + start_quote.len_utf8()..].find(end_quote)
        {
            let candidate =
                text[start + start_quote.len_utf8()..start + start_quote.len_utf8() + end].trim();
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
        }
    }

    // Check words/tokens with ascii alphanumeric + hyphens/underscores (e.g. auth-service, order-api, alice)
    let words = text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');

    for word in words {
        let w = word.trim().trim_matches(|c| c == '-' || c == '_');
        if w.is_empty() || w.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let is_excluded = exclude_keywords.iter().any(|k| k.eq_ignore_ascii_case(w));
        if !is_excluded {
            return Some(w.to_string());
        }
    }

    None
}

pub struct BotService {
    rag: Arc<RagService>,
    skills: Arc<SqlSkillRegistry>,
    data_sources: Arc<DataSourceManager>,
    planner: Option<Arc<dyn BotPlanner>>,
}

impl BotService {
    pub fn new(
        rag: Arc<RagService>,
        skills: Arc<SqlSkillRegistry>,
        data_sources: Arc<DataSourceManager>,
        planner: Option<Arc<dyn BotPlanner>>,
    ) -> Self {
        Self {
            rag,
            skills,
            data_sources,
            planner,
        }
    }

    pub fn list_skills(&self) -> Vec<SqlSkill> {
        self.skills.list().into_iter().cloned().collect()
    }

    pub fn test_skill(
        &self,
        skill_name: &str,
        parameters: &HashMap<String, serde_json::Value>,
    ) -> Result<QueryResult, SkillError> {
        let skill = self
            .skills
            .get(skill_name)
            .ok_or_else(|| SkillError::SkillNotFound(skill_name.to_string()))?;

        let mut merged_params = HashMap::new();
        for param in &skill.parameters {
            if let Some(val) = parameters.get(&param.name) {
                merged_params.insert(param.name.clone(), val.clone());
            } else if let Some(def) = &param.default {
                merged_params.insert(param.name.clone(), def.clone());
            } else if param.required {
                return Err(SkillError::ParameterError(format!(
                    "Missing required parameter '{}' for skill '{}'",
                    param.name, skill.name
                )));
            }
        }

        self.data_sources.execute_query(
            &skill.connection_ref,
            &skill.sql_template,
            &merged_params,
            skill.max_rows.unwrap_or(50),
        )
    }

    pub fn build_capabilities_overview(&self) -> String {
        let skills = self.skills.list();
        let mut text = String::new();
        text.push_str("您好！我是企业平台内部智能助手。目前我掌握以下两类核心能力：\n\n");

        text.push_str("📊 **实时数据查询技能（SQL Skills）**：\n");
        if skills.is_empty() {
            text.push_str("- （当前暂未加载数据技能）\n");
        } else {
            for skill in skills {
                text.push_str(&format!("- **{}**：{}\n", skill.name, skill.description));
            }
        }

        text.push_str("\n📖 **平台知识库与使用指南（RAG 检索）**：\n");
        if let Ok(docs) = self.rag.list_documents() {
            if docs.is_empty() {
                text.push_str("- （知识库暂未入库文档，可通过 `/upload` 页面上传）\n");
            } else {
                text.push_str(&format!("- 已收录 {} 篇知识文档，包括：", docs.len()));
                let sample_titles: Vec<&str> =
                    docs.iter().take(3).map(|d| d.title.as_str()).collect();
                text.push_str(&sample_titles.join("、"));
                if docs.len() > 3 {
                    text.push_str(" 等");
                }
                text.push('\n');
            }
        }

        text.push_str("\n💡 **您可以直接向我提问，例如**：\n");
        text.push_str("- “查一下微服务 auth-service 最近的指标”\n");
        text.push_str("- “查询用户 1001 的基本信息”\n");
        text.push_str("- “企业 API 安全管理规范是什么？”\n");

        text
    }

    pub fn chat(
        &self,
        message: &str,
        top_k: usize,
        access_scopes: &[String],
    ) -> Result<BotChatResponse, BotError> {
        let available_skills = self.skills.list();
        let action = if let Some(planner) = &self.planner {
            match planner.plan(message, &available_skills) {
                Ok(act) => act,
                Err(err) => {
                    warn!("LLM bot planning failed, falling back to rule-based: {err}");
                    rule_based_route(message, &available_skills)
                }
            }
        } else {
            rule_based_route(message, &available_skills)
        };

        match action {
            BotAction::Skill {
                skill_name,
                parameters,
            } => {
                info!("Bot executing skill: {skill_name}");
                match self.test_skill(&skill_name, &parameters) {
                    Ok(result) => {
                        let formatted = format_query_result_markdown(&result);
                        Ok(BotChatResponse {
                            reply: formatted,
                            action_taken: "skill".to_string(),
                            skill_used: Some(skill_name),
                            skill_data: Some(result),
                            citations: Vec::new(),
                            confidence: Some(AnswerConfidence {
                                level: ConfidenceLevel::High,
                                score: 1.0,
                                reason: "Data retrieved directly from enterprise database".into(),
                            }),
                        })
                    }
                    Err(err) => {
                        warn!("Skill {skill_name} execution error: {err}");
                        Ok(BotChatResponse {
                            reply: format!("执行数据查询技能【{skill_name}】失败: {err}"),
                            action_taken: "skill".to_string(),
                            skill_used: Some(skill_name),
                            skill_data: None,
                            citations: Vec::new(),
                            confidence: Some(AnswerConfidence {
                                level: ConfidenceLevel::Low,
                                score: 0.0,
                                reason: format!("Database execution failed: {err}"),
                            }),
                        })
                    }
                }
            }
            BotAction::Rag { query } => {
                info!("Bot routing to RAG search: {query}");
                let rag_res = self.rag.answer_with_scopes(&query, top_k, access_scopes)?;
                Ok(BotChatResponse {
                    reply: rag_res.answer,
                    action_taken: "rag".to_string(),
                    skill_used: None,
                    skill_data: None,
                    citations: rag_res.citations,
                    confidence: Some(rag_res.confidence),
                })
            }
            BotAction::Direct { response } => {
                let reply = if response.is_empty() {
                    self.build_capabilities_overview()
                } else {
                    response
                };
                Ok(BotChatResponse {
                    reply,
                    action_taken: "direct".to_string(),
                    skill_used: None,
                    skill_data: None,
                    citations: Vec::new(),
                    confidence: Some(AnswerConfidence {
                        level: ConfidenceLevel::High,
                        score: 1.0,
                        reason: "Direct assistant response".into(),
                    }),
                })
            }
        }
    }

    pub fn chat_stream<F>(
        &self,
        message: &str,
        top_k: usize,
        access_scopes: &[String],
        mut on_event: F,
    ) -> Result<BotChatResponse, BotError>
    where
        F: FnMut(&str) -> Result<(), BotError>,
    {
        let available_skills = self.skills.list();
        let action = if let Some(planner) = &self.planner {
            match planner.plan(message, &available_skills) {
                Ok(act) => act,
                Err(err) => {
                    warn!("LLM bot planning failed in stream, falling back to rule-based: {err}");
                    rule_based_route(message, &available_skills)
                }
            }
        } else {
            rule_based_route(message, &available_skills)
        };

        match action {
            BotAction::Skill {
                skill_name,
                parameters,
            } => {
                let action_event = serde_json::json!({
                    "type": "action",
                    "action": "skill",
                    "skill_name": skill_name
                });
                on_event(&action_event.to_string())?;

                let res = self.test_skill(&skill_name, &parameters);
                match res {
                    Ok(result) => {
                        let formatted = format_query_result_markdown(&result);
                        let data_event = serde_json::json!({
                            "type": "data",
                            "columns": result.columns,
                            "rows": result.rows,
                            "total_rows": result.total_rows,
                            "execution_ms": result.execution_ms
                        });
                        on_event(&data_event.to_string())?;

                        let token_event = serde_json::json!({
                            "type": "token",
                            "content": formatted
                        });
                        on_event(&token_event.to_string())?;

                        Ok(BotChatResponse {
                            reply: formatted,
                            action_taken: "skill".to_string(),
                            skill_used: Some(skill_name),
                            skill_data: Some(result),
                            citations: Vec::new(),
                            confidence: Some(AnswerConfidence {
                                level: ConfidenceLevel::High,
                                score: 1.0,
                                reason: "Data retrieved directly from enterprise database".into(),
                            }),
                        })
                    }
                    Err(err) => {
                        let msg = format!("执行数据查询技能【{skill_name}】失败: {err}");
                        let token_event = serde_json::json!({
                            "type": "token",
                            "content": msg
                        });
                        on_event(&token_event.to_string())?;
                        Ok(BotChatResponse {
                            reply: msg,
                            action_taken: "skill".to_string(),
                            skill_used: Some(skill_name),
                            skill_data: None,
                            citations: Vec::new(),
                            confidence: Some(AnswerConfidence {
                                level: ConfidenceLevel::Low,
                                score: 0.0,
                                reason: format!("Database execution failed: {err}"),
                            }),
                        })
                    }
                }
            }
            BotAction::Rag { query } => {
                let action_event = serde_json::json!({
                    "type": "action",
                    "action": "rag",
                    "query": query
                });
                on_event(&action_event.to_string())?;

                let allowed_scopes = allowed_access_scopes(access_scopes);
                let rag_response = self.rag.answer_stream_with_scopes(
                    &query,
                    top_k,
                    &allowed_scopes,
                    |token| {
                        let chunk = serde_json::json!({
                            "type": "token",
                            "content": token
                        });
                        on_event(&chunk.to_string())
                            .map_err(|e| crate::generation::GenerateError::Provider(e.to_string()))
                    },
                )?;

                let meta_event = serde_json::json!({
                    "type": "metadata",
                    "citations": rag_response.citations,
                    "confidence": rag_response.confidence
                });
                on_event(&meta_event.to_string())?;

                Ok(BotChatResponse {
                    reply: rag_response.answer,
                    action_taken: "rag".to_string(),
                    skill_used: None,
                    skill_data: None,
                    citations: rag_response.citations,
                    confidence: Some(rag_response.confidence),
                })
            }
            BotAction::Direct { response } => {
                let reply = if response.is_empty() {
                    self.build_capabilities_overview()
                } else {
                    response
                };
                let action_event = serde_json::json!({
                    "type": "action",
                    "action": "direct"
                });
                on_event(&action_event.to_string())?;

                let token_event = serde_json::json!({
                    "type": "token",
                    "content": reply
                });
                on_event(&token_event.to_string())?;

                Ok(BotChatResponse {
                    reply,
                    action_taken: "direct".to_string(),
                    skill_used: None,
                    skill_data: None,
                    citations: Vec::new(),
                    confidence: Some(AnswerConfidence {
                        level: ConfidenceLevel::High,
                        score: 1.0,
                        reason: "Direct response".into(),
                    }),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bot_action_json() {
        let text = r#"```json
{
  "action": "skill",
  "skill_name": "query_user_info",
  "parameters": {
    "user_id": 1001
  }
}
```"#;
        let action = parse_bot_action(text).expect("parse bot action");
        match action {
            BotAction::Skill {
                skill_name,
                parameters,
            } => {
                assert_eq!(skill_name, "query_user_info");
                assert_eq!(parameters["user_id"], 1001);
            }
            _ => panic!("Expected Skill action"),
        }
    }

    #[test]
    fn test_rule_based_greeting() {
        let action = rule_based_route("你好", &[]);
        match action {
            BotAction::Direct { response: _ } => {}
            _ => panic!("Expected Direct action"),
        }
    }

    #[test]
    fn test_rule_based_capabilities_query() {
        let action = rule_based_route("你现在知道什么？", &[]);
        match action {
            BotAction::Direct { response: _ } => {}
            _ => panic!("Expected Direct action for capabilities query"),
        }
    }

    #[test]
    fn test_rule_based_skill_match() {
        let skill = SqlSkill {
            name: "query_user_orders".to_string(),
            description: "查询用户订单".to_string(),
            connection_ref: "PRIMARY".to_string(),
            sql_template: "SELECT * FROM orders WHERE user_id = :user_id".to_string(),
            parameters: vec![crate::skills::SqlSkillParameter {
                name: "user_id".to_string(),
                param_type: "integer".to_string(),
                description: "用户ID".to_string(),
                required: true,
                default: None,
            }],
            keywords: vec!["订单".to_string(), "orders".to_string()],
            max_rows: Some(10),
        };

        let action = rule_based_route("帮我查一下用户 2005 的订单记录", &[&skill]);
        match action {
            BotAction::Skill {
                skill_name,
                parameters,
            } => {
                assert_eq!(skill_name, "query_user_orders");
                assert_eq!(parameters["user_id"], 2005);
            }
            _ => panic!("Expected Skill action for matching keyword '订单'"),
        }
    }
}
