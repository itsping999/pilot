use std::collections::HashMap;
use std::sync::Arc;

use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentError, extract_json_object};
use crate::config::{
    ChatCompletionsProviderConfig, ClaudeMessagesProviderConfig, MODEL_NOT_CONFIGURED_MESSAGE,
};
use crate::contracts::BotChatResponse;
use crate::models::{AnswerConfidence, ConfidenceLevel, allowed_access_scopes};
use crate::rag::{RagError, RagService};
use crate::skills::{
    DataSourceManager, QueryResult, SkillError, SqlSkill, SqlSkillRegistry,
    format_query_result_markdown,
};

#[derive(Debug)]
pub enum BotError {
    ModelNotConfigured,
    Planning(AgentError),
    Rag(RagError),
    Skill(SkillError),
    Internal(String),
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotConfigured => write!(f, "{MODEL_NOT_CONFIGURED_MESSAGE}"),
            Self::Planning(err) => write!(f, "Bot planning error: {err}"),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotProfile {
    #[serde(default = "default_bot_name")]
    pub bot_name: String,
    #[serde(default = "default_greeting_reply")]
    pub greeting_reply: String,
    #[serde(default)]
    pub extra_capabilities: Vec<String>,
    #[serde(default = "default_example_queries")]
    pub example_queries: Vec<String>,
}

fn default_bot_name() -> String {
    "企业平台智能助手".to_string()
}

fn default_greeting_reply() -> String {
    "您好！我一直在线，随时为您服务。\n您可以向我咨询平台使用方法与规范，或查询系统实时运行数据。请告诉我您需要什么帮助！".to_string()
}

fn default_example_queries() -> Vec<String> {
    vec![
        "查一下微服务 auth-service 最近的指标".to_string(),
        "查询用户 1001 的基本信息".to_string(),
        "企业 API 安全管理规范是什么？".to_string(),
    ]
}

impl Default for BotProfile {
    fn default() -> Self {
        Self {
            bot_name: default_bot_name(),
            greeting_reply: default_greeting_reply(),
            extra_capabilities: Vec::new(),
            example_queries: default_example_queries(),
        }
    }
}

impl BotProfile {
    pub fn load_from_path_or_default(path: Option<&std::path::Path>) -> Self {
        if let Some(p) = path {
            if p.exists() {
                match std::fs::read_to_string(p) {
                    Ok(content) => match serde_json::from_str::<BotProfile>(&content) {
                        Ok(profile) => {
                            info!("Loaded bot profile from {}", p.display());
                            return profile;
                        }
                        Err(e) => {
                            warn!("Failed to parse bot profile from {}: {e}", p.display());
                        }
                    },
                    Err(e) => {
                        warn!("Failed to read bot profile from {}: {e}", p.display());
                    }
                }
            }
        } else {
            let default_path = std::path::Path::new("./config/bot.json");
            if default_path.exists()
                && let Ok(content) = std::fs::read_to_string(default_path)
                && let Ok(profile) = serde_json::from_str::<BotProfile>(&content)
            {
                info!("Loaded default bot profile from ./config/bot.json");
                return profile;
            }
        }

        BotProfile::default()
    }
}

pub trait BotPlanner: Send + Sync {
    fn plan(&self, message: &str, skills: &[&SqlSkill]) -> Result<BotAction, AgentError>;
}

pub struct ChatCompletionsBotPlanner {
    config: ChatCompletionsProviderConfig,
    client: reqwest::blocking::Client,
    profile: Arc<BotProfile>,
}

impl ChatCompletionsBotPlanner {
    pub fn new(config: ChatCompletionsProviderConfig, profile: Arc<BotProfile>) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
            profile,
        }
    }
}

impl BotPlanner for ChatCompletionsBotPlanner {
    fn plan(&self, message: &str, skills: &[&SqlSkill]) -> Result<BotAction, AgentError> {
        let system_prompt = build_bot_system_prompt(skills, &self.profile);
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
    profile: Arc<BotProfile>,
}

impl ClaudeMessagesBotPlanner {
    pub fn new(config: ClaudeMessagesProviderConfig, profile: Arc<BotProfile>) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
            profile,
        }
    }
}

impl BotPlanner for ClaudeMessagesBotPlanner {
    fn plan(&self, message: &str, skills: &[&SqlSkill]) -> Result<BotAction, AgentError> {
        let system_prompt = build_bot_system_prompt(skills, &self.profile);
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

fn build_bot_system_prompt(skills: &[&SqlSkill], profile: &BotProfile) -> String {
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

    let mut extra_caps = String::new();
    for extra in &profile.extra_capabilities {
        extra_caps.push_str(&format!("- 扩展能力/支持渠道: {}\n", extra));
    }

    format!(
        r#"你是{bot_name}。你拥有以下核心能力：
1. 平台文档与知识库问答 (RAG)：回答平台使用方法、架构规范、开发指南、FAQ、流程与政策。
2. 预定义业务数据查询技能 (SQL Skills)：调用预设模板查询实时平台数据（如用户状态、服务指标、告警通知等）。
{extra_caps}
可用技能列表：
{skills_text}

根据用户的输入，分析其真实意图并做出路由判断：
- 如果用户意图是查询实时业务数据且有匹配的技能，返回 JSON:
  {{"action":"skill","skill_name":"<技能名>","parameters":{{"<参数名>":<提取值>}}}}
- 如果用户意图是询问平台使用方法、概念、流程、常见问题或未匹配到数据技能，返回 JSON:
  {{"action":"rag","query":"<提取并优化后的检索关键词>"}}
- 如果用户输入为简单打招呼、在吗、在线确认或寒暄（如“在？”、“你好”），返回 JSON:
  {{"action":"direct","response":"{greeting}"}}
- 如果用户输入为功能咨询（“你能做什么”、“你现在知道什么”）、能力介绍，返回 JSON:
  {{"action":"direct","response":""}}

必须严格仅返回一个合法 JSON 对象，不要输出 markdown 代码块标记，不要包含其他文字。"#,
        bot_name = profile.bot_name,
        extra_caps = extra_caps,
        skills_text = skills_text,
        greeting = profile.greeting_reply.replace('\n', " ")
    )
}

fn parse_bot_action(content: &str) -> Result<BotAction, AgentError> {
    let json_str = extract_json_object(content).ok_or_else(|| {
        AgentError::InvalidPlan("Bot planner response did not contain JSON".into())
    })?;
    serde_json::from_str(json_str).map_err(|e| AgentError::InvalidPlan(format!("{e}: {json_str}")))
}

pub struct BotService {
    rag: Arc<RagService>,
    skills: Arc<SqlSkillRegistry>,
    data_sources: Arc<DataSourceManager>,
    planner: Option<Arc<dyn BotPlanner>>,
    profile: Arc<BotProfile>,
}

impl BotService {
    pub fn new(
        rag: Arc<RagService>,
        skills: Arc<SqlSkillRegistry>,
        data_sources: Arc<DataSourceManager>,
        planner: Option<Arc<dyn BotPlanner>>,
        profile: Arc<BotProfile>,
    ) -> Self {
        Self {
            rag,
            skills,
            data_sources,
            planner,
            profile,
        }
    }

    pub fn list_skills(&self) -> Vec<SqlSkill> {
        self.skills.list().into_iter().cloned().collect()
    }

    pub fn ensure_model_configured(&self) -> Result<(), BotError> {
        self.planner
            .as_ref()
            .ok_or(BotError::ModelNotConfigured)
            .map(|_| ())
    }

    fn plan_action(&self, message: &str) -> Result<BotAction, BotError> {
        let planner = self.planner.as_ref().ok_or(BotError::ModelNotConfigured)?;
        planner
            .plan(message, &self.skills.list())
            .map_err(BotError::Planning)
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
        text.push_str(&format!(
            "您好！我是{}。目前我掌握以下核心能力：\n\n",
            self.profile.bot_name
        ));

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

        if !self.profile.extra_capabilities.is_empty() {
            text.push_str("\n🛠️ **其他扩展能力与支持**：\n");
            for extra in &self.profile.extra_capabilities {
                text.push_str(&format!("- {}\n", extra));
            }
        }

        text.push_str("\n💡 **您可以直接向我提问，例如**：\n");
        for eg in &self.profile.example_queries {
            text.push_str(&format!("- “{}”\n", eg));
        }

        text
    }

    pub fn chat(
        &self,
        message: &str,
        top_k: usize,
        access_scopes: &[String],
    ) -> Result<BotChatResponse, BotError> {
        let action = self.plan_action(message)?;

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
        let action = self.plan_action(message)?;

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

                        for line in formatted.split_inclusive('\n') {
                            let token_event = serde_json::json!({
                                "type": "token",
                                "content": line
                            });
                            on_event(&token_event.to_string())?;
                        }

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

                for line in reply.split_inclusive('\n') {
                    let token_event = serde_json::json!({
                        "type": "token",
                        "content": line
                    });
                    on_event(&token_event.to_string())?;
                }

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

    fn test_bot(planner: Option<Arc<dyn BotPlanner>>) -> BotService {
        let store = Arc::new(crate::store::SqliteStore::new(
            rusqlite::Connection::open_in_memory().expect("open test database"),
        ));
        store.init().expect("initialize test database");
        let rag = Arc::new(RagService::new(
            store.clone(),
            Arc::new(crate::embedding::HashEmbedder::default()),
            Arc::new(crate::retrieval::SqliteRetriever::new(store)),
            Arc::new(crate::generation::ExtractiveGenerator),
            None,
            600,
            100,
        ));
        let mut skills = SqlSkillRegistry::new();
        skills.register(SqlSkill {
            name: "query_user_orders".to_string(),
            description: "查询用户订单".to_string(),
            connection_ref: "UNCONFIGURED_TEST_DB".to_string(),
            sql_template: "SELECT * FROM orders WHERE user_id = :user_id".to_string(),
            parameters: vec![crate::skills::SqlSkillParameter {
                name: "user_id".to_string(),
                param_type: "integer".to_string(),
                description: "用户ID".to_string(),
                required: true,
                default: None,
            }],
            keywords: vec!["订单".to_string()],
            max_rows: Some(10),
        });
        BotService::new(
            rag,
            Arc::new(skills),
            Arc::new(DataSourceManager::new()),
            planner,
            Arc::new(BotProfile::default()),
        )
    }

    #[test]
    fn chat_without_model_rejects_all_intents() {
        let bot = test_bot(None);
        for message in [
            "你好",
            "在？",
            "？？？",
            "你能做什么",
            "用户 2005 的订单",
            "平台使用方法",
        ] {
            assert!(matches!(
                bot.chat(message, 5, &[]),
                Err(BotError::ModelNotConfigured)
            ));
        }
        assert!(bot.rag.list_query_history(100).unwrap().is_empty());
    }

    #[test]
    fn stream_without_model_emits_no_actions_or_tokens() {
        let bot = test_bot(None);
        for message in ["你好", "用户 2005 的订单", "平台使用方法"] {
            let mut events = Vec::new();
            let result = bot.chat_stream(message, 5, &[], |event| {
                events.push(event.to_string());
                Ok(())
            });
            assert!(matches!(result, Err(BotError::ModelNotConfigured)));
            assert!(events.is_empty());
        }
        assert!(bot.rag.list_query_history(100).unwrap().is_empty());
    }

    struct FailingPlanner;

    impl BotPlanner for FailingPlanner {
        fn plan(&self, _message: &str, _skills: &[&SqlSkill]) -> Result<BotAction, AgentError> {
            Err(AgentError::Provider("model unavailable".to_string()))
        }
    }

    #[test]
    fn planning_failure_does_not_execute_keyword_skill() {
        let bot = test_bot(Some(Arc::new(FailingPlanner)));
        assert!(matches!(
            bot.chat("用户 2005 的订单", 5, &[]),
            Err(BotError::Planning(AgentError::Provider(_)))
        ));
        let mut events = Vec::new();
        let result = bot.chat_stream("用户 2005 的订单", 5, &[], |event| {
            events.push(event.to_string());
            Ok(())
        });
        assert!(matches!(
            result,
            Err(BotError::Planning(AgentError::Provider(_)))
        ));
        assert!(events.is_empty());
        assert!(bot.rag.list_query_history(100).unwrap().is_empty());
    }

    struct DirectPlanner;

    impl BotPlanner for DirectPlanner {
        fn plan(&self, _message: &str, _skills: &[&SqlSkill]) -> Result<BotAction, AgentError> {
            Ok(BotAction::Direct {
                response: "模型生成的回复".to_string(),
            })
        }
    }

    #[test]
    fn configured_planner_controls_normal_and_streaming_chat() {
        let bot = test_bot(Some(Arc::new(DirectPlanner)));
        let response = bot.chat("你好", 5, &[]).expect("chat succeeds");
        assert_eq!(response.reply, "模型生成的回复");
        assert_eq!(response.action_taken, "direct");
        let mut events = Vec::new();
        let response = bot
            .chat_stream("你好", 5, &[], |event| {
                events.push(serde_json::from_str::<serde_json::Value>(event).unwrap());
                Ok(())
            })
            .expect("stream succeeds");
        assert_eq!(response.reply, "模型生成的回复");
        assert_eq!(events[0]["action"], "direct");
        assert_eq!(events[1]["content"], "模型生成的回复");
    }
}
