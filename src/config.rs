use std::collections::BTreeMap;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

pub const MODEL_NOT_CONFIGURED_MESSAGE: &str = "未配置可用模型，请设置 KNOWLEDGE_PILOT_LLM_PROVIDER、KNOWLEDGE_PILOT_LLM_MODEL 和对应的服务地址或 API Key（Claude 也支持 KNOWLEDGE_PILOT_CLAUDE_MODEL）。";

#[derive(Clone, Debug)]
pub struct Config {
    pub addr: SocketAddr,
    pub db_path: PathBuf,
    pub api_token: Option<String>,
    pub ui_enabled: bool,
    pub request_body_limit_bytes: usize,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub embedding: EmbeddingProviderConfig,
    pub rag_mode: RagMode,
    pub llm_provider: LlmProvider,
    pub chat_completions: ChatCompletionsProviderConfig,
    pub claude: ClaudeMessagesProviderConfig,
    pub rate_limit_rpm: u32,
    pub skills_path: Option<PathBuf>,
    pub bot_config_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbeddingProvider {
    Hash,
    Ollama,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingProviderConfig {
    pub provider: EmbeddingProvider,
    pub base_url: String,
    pub path: String,
    pub model: String,
    pub allow_unverified_model: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmProvider {
    Unconfigured,
    Extractive,
    OpenAiCompatible,
    Custom,
    Claude,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RagMode {
    Auto,
    Traditional,
    Agentic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCompletionsProviderConfig {
    pub name: String,
    pub base_url: String,
    pub base_url_configured: bool,
    pub path: String,
    pub api_key: Option<String>,
    pub model: String,
    pub auth_header: String,
    pub auth_scheme: Option<String>,
    pub extra_headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeMessagesProviderConfig {
    pub name: String,
    pub base_url: String,
    pub base_url_configured: bool,
    pub path: String,
    pub api_key: Option<String>,
    pub model: String,
    pub anthropic_version: String,
    pub max_tokens: u32,
    pub extra_headers: Vec<(String, String)>,
}

impl Config {
    pub fn from_env() -> Self {
        let addr = env::var("KNOWLEDGE_PILOT_ADDR")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080));

        let db_path = env::var("KNOWLEDGE_PILOT_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/knowledge-pilot.db"));

        let api_token = env::var("KNOWLEDGE_PILOT_API_TOKEN")
            .ok()
            .and_then(clean_env_string);

        let ui_enabled = env::var("KNOWLEDGE_PILOT_UI_ENABLED")
            .map(|value| value != "false" && value != "0")
            .unwrap_or(true);

        let request_body_limit_bytes = env::var("KNOWLEDGE_PILOT_REQUEST_BODY_LIMIT_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(2 * 1024 * 1024);

        let chunk_size = env::var("KNOWLEDGE_PILOT_CHUNK_SIZE")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(600);

        let chunk_overlap = env::var("KNOWLEDGE_PILOT_CHUNK_OVERLAP")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value < chunk_size)
            .unwrap_or(100);

        let embedding = EmbeddingProviderConfig::from_env();

        let rag_mode = RagMode::from_value(
            &env::var("KNOWLEDGE_PILOT_RAG_MODE").unwrap_or_else(|_| "auto".to_string()),
        );

        let llm_provider =
            LlmProvider::from_value(&env::var("KNOWLEDGE_PILOT_LLM_PROVIDER").unwrap_or_default());

        let chat_completions = ChatCompletionsProviderConfig::from_env(&llm_provider);
        let claude = ClaudeMessagesProviderConfig::from_env(&llm_provider);

        let rate_limit_rpm = env::var("KNOWLEDGE_PILOT_RATE_LIMIT_RPM")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(300);

        let skills_path = env::var("KNOWLEDGE_PILOT_SKILLS_PATH")
            .ok()
            .and_then(clean_env_string)
            .map(PathBuf::from);

        let bot_config_path = env::var("KNOWLEDGE_PILOT_BOT_CONFIG_PATH")
            .ok()
            .and_then(clean_env_string)
            .map(PathBuf::from);

        Self {
            addr,
            db_path,
            api_token,
            ui_enabled,
            request_body_limit_bytes,
            chunk_size,
            chunk_overlap,
            embedding,
            rag_mode,
            llm_provider,
            chat_completions,
            claude,
            rate_limit_rpm,
            skills_path,
            bot_config_path,
        }
    }
}

impl LlmProvider {
    fn from_value(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "extractive" => Self::Extractive,
            "openai" | "openai-compatible" | "openai_compatible" => Self::OpenAiCompatible,
            "custom" | "chat-completions" | "chat_completions" => Self::Custom,
            "claude" | "anthropic" => Self::Claude,
            _ => Self::Unconfigured,
        }
    }
}

impl RagMode {
    fn from_value(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "agentic" | "agent" => Self::Agentic,
            "traditional" | "classic" | "rag" => Self::Traditional,
            _ => Self::Auto,
        }
    }
}

impl EmbeddingProviderConfig {
    fn from_env() -> Self {
        let provider = match env::var("KNOWLEDGE_PILOT_EMBEDDING_PROVIDER")
            .unwrap_or_else(|_| "hash".to_string())
            .to_lowercase()
            .as_str()
        {
            "ollama" => EmbeddingProvider::Ollama,
            _ => EmbeddingProvider::Hash,
        };

        let default_model = match provider {
            EmbeddingProvider::Hash => "hash-256",
            EmbeddingProvider::Ollama => "qwen3-embedding:0.6b",
        };

        Self {
            provider,
            base_url: first_env(&[
                "KNOWLEDGE_PILOT_EMBEDDING_BASE_URL",
                "KNOWLEDGE_PILOT_OLLAMA_BASE_URL",
            ])
            .unwrap_or_else(|| "http://127.0.0.1:11434".to_string()),
            path: first_env(&["KNOWLEDGE_PILOT_EMBEDDING_PATH"])
                .unwrap_or_else(|| "/api/embed".to_string()),
            model: first_env(&["KNOWLEDGE_PILOT_EMBEDDING_MODEL"])
                .unwrap_or_else(|| default_model.to_string()),
            allow_unverified_model: bool_env("KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL"),
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            self.path.trim_start_matches('/')
        )
    }
}

impl ChatCompletionsProviderConfig {
    fn from_env(provider: &LlmProvider) -> Self {
        let base_url = first_env(&[
            "KNOWLEDGE_PILOT_LLM_BASE_URL",
            "KNOWLEDGE_PILOT_OPENAI_BASE_URL",
        ]);
        let base_url_configured = base_url.is_some();
        let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let default_name = match provider {
            LlmProvider::Unconfigured => "unconfigured",
            LlmProvider::Extractive => "extractive",
            LlmProvider::OpenAiCompatible => "openai-compatible",
            LlmProvider::Custom => "custom",
            LlmProvider::Claude => "claude",
        };

        Self {
            name: first_env(&["KNOWLEDGE_PILOT_LLM_PROVIDER_NAME"])
                .unwrap_or_else(|| default_name.to_string()),
            base_url,
            base_url_configured,
            path: first_env(&["KNOWLEDGE_PILOT_LLM_CHAT_COMPLETIONS_PATH"])
                .unwrap_or_else(|| "/chat/completions".to_string()),
            api_key: first_env(&[
                "KNOWLEDGE_PILOT_LLM_API_KEY",
                "KNOWLEDGE_PILOT_OPENAI_API_KEY",
            ]),
            model: first_env(&["KNOWLEDGE_PILOT_LLM_MODEL", "KNOWLEDGE_PILOT_OPENAI_MODEL"])
                .unwrap_or_default(),
            auth_header: first_env(&["KNOWLEDGE_PILOT_LLM_AUTH_HEADER"])
                .unwrap_or_else(|| "Authorization".to_string()),
            auth_scheme: auth_scheme_from_env(),
            extra_headers: parse_headers_json(first_env(&["KNOWLEDGE_PILOT_LLM_HEADERS_JSON"])),
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            self.path.trim_start_matches('/')
        )
    }
}

impl ClaudeMessagesProviderConfig {
    fn from_env(provider: &LlmProvider) -> Self {
        let base_url = first_env(&[
            "KNOWLEDGE_PILOT_CLAUDE_BASE_URL",
            "KNOWLEDGE_PILOT_LLM_BASE_URL",
        ]);
        let base_url_configured = base_url.is_some();
        let default_name = match provider {
            LlmProvider::Claude => "claude",
            _ => "anthropic-compatible",
        };

        Self {
            name: first_env(&["KNOWLEDGE_PILOT_LLM_PROVIDER_NAME"])
                .unwrap_or_else(|| default_name.to_string()),
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            base_url_configured,
            path: first_env(&["KNOWLEDGE_PILOT_CLAUDE_MESSAGES_PATH"])
                .unwrap_or_else(|| "/messages".to_string()),
            api_key: first_env(&[
                "KNOWLEDGE_PILOT_CLAUDE_API_KEY",
                "KNOWLEDGE_PILOT_LLM_API_KEY",
                "ANTHROPIC_API_KEY",
            ]),
            model: first_env(&["KNOWLEDGE_PILOT_CLAUDE_MODEL", "KNOWLEDGE_PILOT_LLM_MODEL"])
                .unwrap_or_default(),
            anthropic_version: first_env(&[
                "KNOWLEDGE_PILOT_CLAUDE_ANTHROPIC_VERSION",
                "KNOWLEDGE_PILOT_LLM_ANTHROPIC_VERSION",
            ])
            .unwrap_or_else(|| "2023-06-01".to_string()),
            max_tokens: first_env(&[
                "KNOWLEDGE_PILOT_CLAUDE_MAX_TOKENS",
                "KNOWLEDGE_PILOT_LLM_MAX_TOKENS",
            ])
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(1024),
            extra_headers: parse_headers_json(first_env(&["KNOWLEDGE_PILOT_LLM_HEADERS_JSON"])),
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            self.path.trim_start_matches('/')
        )
    }
}

fn first_env(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| env::var(key).ok().and_then(clean_env_string))
}

fn parse_headers_json(value: Option<String>) -> Vec<(String, String)> {
    let Some(value) = value else {
        return Vec::new();
    };

    serde_json::from_str::<BTreeMap<String, String>>(&value)
        .map(|headers| headers.into_iter().collect())
        .unwrap_or_default()
}

fn auth_scheme_from_env() -> Option<String> {
    match env::var("KNOWLEDGE_PILOT_LLM_AUTH_SCHEME") {
        Ok(value) => clean_env_string(value),
        Err(_) => Some("Bearer".to_string()),
    }
}

fn bool_env(key: &str) -> bool {
    env::var(key)
        .ok()
        .and_then(clean_env_string)
        .is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
}

fn clean_env_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::{
        ChatCompletionsProviderConfig, ClaudeMessagesProviderConfig, EmbeddingProvider,
        EmbeddingProviderConfig, LlmProvider, RagMode,
    };

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn llm_provider_requires_explicit_selection() {
        for value in ["", "   ", "unexpected"] {
            assert_eq!(LlmProvider::from_value(value), LlmProvider::Unconfigured);
        }
        assert_eq!(
            LlmProvider::from_value("extractive"),
            LlmProvider::Extractive
        );
        assert_eq!(LlmProvider::from_value(" Custom "), LlmProvider::Custom);
        assert_eq!(
            LlmProvider::from_value("openai"),
            LlmProvider::OpenAiCompatible
        );
        assert_eq!(LlmProvider::from_value("anthropic"), LlmProvider::Claude);
    }

    #[test]
    fn chat_models_require_configuration_and_preserve_legacy_aliases() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let keys = [
            "KNOWLEDGE_PILOT_LLM_MODEL",
            "KNOWLEDGE_PILOT_OPENAI_MODEL",
            "KNOWLEDGE_PILOT_CLAUDE_MODEL",
        ];
        let previous: Vec<_> = keys.iter().map(std::env::var_os).collect();
        unsafe {
            for key in keys {
                std::env::remove_var(key);
            }
        }
        let chat = ChatCompletionsProviderConfig::from_env(&LlmProvider::Custom);
        let claude = ClaudeMessagesProviderConfig::from_env(&LlmProvider::Claude);
        assert!(chat.model.is_empty());
        assert!(claude.model.is_empty());

        unsafe {
            std::env::set_var("KNOWLEDGE_PILOT_LLM_MODEL", "  ");
        }
        assert!(
            ChatCompletionsProviderConfig::from_env(&LlmProvider::Custom)
                .model
                .is_empty()
        );
        assert!(
            ClaudeMessagesProviderConfig::from_env(&LlmProvider::Claude)
                .model
                .is_empty()
        );

        unsafe {
            std::env::set_var("KNOWLEDGE_PILOT_OPENAI_MODEL", " legacy-model ");
            std::env::set_var("KNOWLEDGE_PILOT_CLAUDE_MODEL", " claude-model ");
        }
        assert_eq!(
            ChatCompletionsProviderConfig::from_env(&LlmProvider::OpenAiCompatible).model,
            "legacy-model"
        );
        assert_eq!(
            ClaudeMessagesProviderConfig::from_env(&LlmProvider::Claude).model,
            "claude-model"
        );

        unsafe {
            for (key, value) in keys.iter().zip(previous) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn rag_mode_parses_known_values() {
        assert_eq!(RagMode::from_value("agentic"), RagMode::Agentic);
        assert_eq!(RagMode::from_value("traditional"), RagMode::Traditional);
        assert_eq!(RagMode::from_value("unexpected"), RagMode::Auto);
    }

    #[test]
    fn embedding_provider_reads_ollama_config() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        unsafe {
            std::env::set_var("KNOWLEDGE_PILOT_EMBEDDING_PROVIDER", "ollama");
            std::env::set_var(
                "KNOWLEDGE_PILOT_EMBEDDING_BASE_URL",
                "http://localhost:11434",
            );
            std::env::set_var("KNOWLEDGE_PILOT_EMBEDDING_PATH", "/api/embed");
            std::env::set_var("KNOWLEDGE_PILOT_EMBEDDING_MODEL", "qwen3-embedding:0.6b");
            std::env::set_var("KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL", "true");
        }

        let config = EmbeddingProviderConfig::from_env();

        assert_eq!(config.provider, EmbeddingProvider::Ollama);
        assert_eq!(config.endpoint_url(), "http://localhost:11434/api/embed");
        assert_eq!(config.model, "qwen3-embedding:0.6b");
        assert!(config.allow_unverified_model);

        unsafe {
            std::env::remove_var("KNOWLEDGE_PILOT_EMBEDDING_PROVIDER");
            std::env::remove_var("KNOWLEDGE_PILOT_EMBEDDING_BASE_URL");
            std::env::remove_var("KNOWLEDGE_PILOT_EMBEDDING_PATH");
            std::env::remove_var("KNOWLEDGE_PILOT_EMBEDDING_MODEL");
            std::env::remove_var("KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL");
        }
    }

    #[test]
    fn custom_provider_reads_generic_chat_config() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        unsafe {
            std::env::set_var("KNOWLEDGE_PILOT_LLM_BASE_URL", "http://localhost:9999/v1");
            std::env::set_var("KNOWLEDGE_PILOT_LLM_CHAT_COMPLETIONS_PATH", "/custom/chat");
            std::env::set_var("KNOWLEDGE_PILOT_LLM_MODEL", "custom-model");
            std::env::set_var("KNOWLEDGE_PILOT_LLM_API_KEY", "test-key");
            std::env::set_var("KNOWLEDGE_PILOT_LLM_AUTH_HEADER", "X-API-Key");
            std::env::set_var("KNOWLEDGE_PILOT_LLM_AUTH_SCHEME", "");
            std::env::set_var(
                "KNOWLEDGE_PILOT_LLM_HEADERS_JSON",
                "{\"X-Tenant\":\"demo\"}",
            );
        }

        let config = ChatCompletionsProviderConfig::from_env(&LlmProvider::Custom);

        assert_eq!(
            config.endpoint_url(),
            "http://localhost:9999/v1/custom/chat"
        );
        assert_eq!(config.model, "custom-model");
        assert_eq!(config.api_key.as_deref(), Some("test-key"));
        assert_eq!(config.auth_header, "X-API-Key");
        assert_eq!(config.auth_scheme, None);
        assert_eq!(
            config.extra_headers,
            vec![("X-Tenant".to_string(), "demo".to_string())]
        );

        unsafe {
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_BASE_URL");
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_CHAT_COMPLETIONS_PATH");
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_MODEL");
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_API_KEY");
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_AUTH_HEADER");
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_AUTH_SCHEME");
            std::env::remove_var("KNOWLEDGE_PILOT_LLM_HEADERS_JSON");
        }
    }

    #[test]
    fn claude_provider_reads_anthropic_messages_config() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        unsafe {
            std::env::set_var(
                "KNOWLEDGE_PILOT_CLAUDE_BASE_URL",
                "http://localhost:9998/v1",
            );
            std::env::set_var("KNOWLEDGE_PILOT_CLAUDE_MESSAGES_PATH", "/messages");
            std::env::set_var("KNOWLEDGE_PILOT_CLAUDE_MODEL", "claude-test");
            std::env::set_var("KNOWLEDGE_PILOT_CLAUDE_API_KEY", "claude-key");
            std::env::set_var("KNOWLEDGE_PILOT_CLAUDE_ANTHROPIC_VERSION", "2023-06-01");
            std::env::set_var("KNOWLEDGE_PILOT_CLAUDE_MAX_TOKENS", "2048");
        }

        let config = ClaudeMessagesProviderConfig::from_env(&LlmProvider::Claude);

        assert_eq!(config.endpoint_url(), "http://localhost:9998/v1/messages");
        assert_eq!(config.model, "claude-test");
        assert_eq!(config.api_key.as_deref(), Some("claude-key"));
        assert_eq!(config.anthropic_version, "2023-06-01");
        assert_eq!(config.max_tokens, 2048);

        unsafe {
            std::env::remove_var("KNOWLEDGE_PILOT_CLAUDE_BASE_URL");
            std::env::remove_var("KNOWLEDGE_PILOT_CLAUDE_MESSAGES_PATH");
            std::env::remove_var("KNOWLEDGE_PILOT_CLAUDE_MODEL");
            std::env::remove_var("KNOWLEDGE_PILOT_CLAUDE_API_KEY");
            std::env::remove_var("KNOWLEDGE_PILOT_CLAUDE_ANTHROPIC_VERSION");
            std::env::remove_var("KNOWLEDGE_PILOT_CLAUDE_MAX_TOKENS");
        }
    }
}
