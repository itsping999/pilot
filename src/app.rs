use std::sync::Arc;

use log::{info, warn};

use crate::agent::{ChatCompletionsPlanner, ClaudeMessagesPlanner, QueryPlanner};
use crate::bot::{BotPlanner, BotService, ChatCompletionsBotPlanner, ClaudeMessagesBotPlanner};
use crate::config::{Config, EmbeddingProvider, LlmProvider, RagMode};
use crate::db::open_database;
use crate::embedding::{Embedder, HashEmbedder, OllamaEmbedder};
use crate::generation::{
    ChatCompletionsGenerator, ClaudeMessagesGenerator, ExtractiveGenerator, Generator,
};
use crate::rag::RagService;
use crate::retrieval::SqliteRetriever;
use crate::skills::{DataSourceManager, SqlSkillRegistry};
use crate::store::SqliteStore;

pub fn build_rag_service(config: &Config) -> Result<Arc<RagService>, Box<dyn std::error::Error>> {
    let db = open_database(&config.db_path)?;
    let store = Arc::new(SqliteStore::new(db));
    store.init()?;

    let embedder = build_embedder(config)?;
    let generator = build_generator(config)?;
    let planner = build_query_planner(config);

    Ok(Arc::new(RagService::new(
        store.clone(),
        embedder,
        Arc::new(SqliteRetriever::new(store.clone())),
        generator,
        planner,
        config.chunk_size,
        config.chunk_overlap,
    )))
}

pub fn build_bot_service(
    config: &Config,
    rag: Arc<RagService>,
) -> Result<Arc<BotService>, Box<dyn std::error::Error>> {
    let skills = Arc::new(SqlSkillRegistry::load_from_env_or_default(
        config.skills_path.as_deref(),
    ));
    let data_sources = Arc::new(DataSourceManager::new());
    let planner = build_bot_planner(config);

    Ok(Arc::new(BotService::new(
        rag,
        skills,
        data_sources,
        planner,
    )))
}

fn build_bot_planner(config: &Config) -> Option<Arc<dyn BotPlanner>> {
    match config.llm_provider {
        LlmProvider::OpenAiCompatible | LlmProvider::Custom
            if is_chat_completions_usable(config) =>
        {
            info!(
                "Bot planner enabled: provider={} model={}",
                config.chat_completions.name, config.chat_completions.model
            );
            Some(Arc::new(ChatCompletionsBotPlanner::new(
                config.chat_completions.clone(),
            )))
        }
        LlmProvider::Claude if is_claude_messages_usable(config) => {
            info!(
                "Bot planner enabled: provider={} model={}",
                config.claude.name, config.claude.model
            );
            Some(Arc::new(ClaudeMessagesBotPlanner::new(
                config.claude.clone(),
            )))
        }
        _ => {
            info!("Bot planner: rule-based router (no external LLM planner configured)");
            None
        }
    }
}

fn build_embedder(config: &Config) -> Result<Arc<dyn Embedder>, Box<dyn std::error::Error>> {
    match config.embedding.provider {
        EmbeddingProvider::Hash => {
            info!("Embedding provider enabled: hash model=hash-256");
            Ok(Arc::new(HashEmbedder::default()))
        }
        EmbeddingProvider::Ollama => {
            validate_ollama_embedding_model(config)?;
            info!(
                "Embedding provider enabled: ollama model={} endpoint={}",
                config.embedding.model,
                config.embedding.endpoint_url()
            );
            Ok(Arc::new(OllamaEmbedder::new(config.embedding.clone())))
        }
    }
}

fn validate_ollama_embedding_model(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if config.embedding.allow_unverified_model
        || is_probable_embedding_model(&config.embedding.model)
    {
        return Ok(());
    }

    Err(format!(
        "KNOWLEDGE_PILOT_EMBEDDING_MODEL={} does not look like an embedding model. Supported embedding model families: qwen3-embedding, bge-m3, nomic-embed-text, mxbai-embed-large, snowflake-arctic-embed, all-minilm, e5, gte, jina-embeddings, sentence-transformers. Keep chat/generation models under KNOWLEDGE_PILOT_LLM_MODEL. Set KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL=true only for a verified custom embedding model.",
        config.embedding.model
    )
    .into())
}

fn is_probable_embedding_model(model: &str) -> bool {
    let model = model.trim().to_lowercase();
    if model.is_empty() {
        return false;
    }

    [
        "embed",
        "embedding",
        "bge",
        "e5",
        "gte",
        "minilm",
        "sentence-transformer",
        "text-embedding",
        "nomic",
        "mxbai",
        "arctic",
        "jina",
        "snowflake",
        "paraphrase",
        "multi-qa",
        "msmarco",
        "contriever",
        "instructor",
        "e5-mistral",
        "uae",
        "voyage",
        "cohere",
        "openai",
        "ada",
        "embd",
    ]
    .iter()
    .any(|marker| model.contains(marker))
}

fn build_generator(config: &Config) -> Result<Arc<dyn Generator>, Box<dyn std::error::Error>> {
    match config.llm_provider {
        LlmProvider::Extractive => Ok(Arc::new(ExtractiveGenerator)),
        LlmProvider::OpenAiCompatible | LlmProvider::Custom => {
            if is_chat_completions_usable(config) {
                info!(
                    "LLM generator enabled: provider={} model={}",
                    config.chat_completions.name, config.chat_completions.model
                );
                Ok(Arc::new(ChatCompletionsGenerator::new(
                    config.chat_completions.clone(),
                )))
            } else {
                warn!(
                    "LLM provider={} is not usable with current configuration; falling back to extractive generator",
                    config.chat_completions.name
                );
                Ok(Arc::new(ExtractiveGenerator))
            }
        }
        LlmProvider::Claude => {
            if is_claude_messages_usable(config) {
                info!(
                    "LLM generator enabled: provider={} model={}",
                    config.claude.name, config.claude.model
                );
                Ok(Arc::new(ClaudeMessagesGenerator::new(
                    config.claude.clone(),
                )))
            } else {
                warn!(
                    "Claude provider is not usable with current configuration; falling back to extractive generator"
                );
                Ok(Arc::new(ExtractiveGenerator))
            }
        }
    }
}

fn build_query_planner(config: &Config) -> Option<Arc<dyn QueryPlanner>> {
    if config.rag_mode == RagMode::Traditional {
        info!("RAG mode: traditional");
        return None;
    }

    match config.llm_provider {
        LlmProvider::OpenAiCompatible | LlmProvider::Custom
            if is_chat_completions_usable(config) =>
        {
            info!(
                "RAG mode: agentic; planner provider={} model={}",
                config.chat_completions.name, config.chat_completions.model
            );
            Some(Arc::new(ChatCompletionsPlanner::new(
                config.chat_completions.clone(),
            )))
        }
        LlmProvider::Claude if is_claude_messages_usable(config) => {
            info!(
                "RAG mode: agentic; planner provider={} model={}",
                config.claude.name, config.claude.model
            );
            Some(Arc::new(ClaudeMessagesPlanner::new(config.claude.clone())))
        }
        _ => {
            if config.rag_mode == RagMode::Agentic {
                warn!(
                    "KNOWLEDGE_PILOT_RAG_MODE=agentic requested but no usable LLM is configured; falling back to traditional RAG"
                );
            } else {
                info!("RAG mode: traditional; no usable LLM planner configured");
            }
            None
        }
    }
}

fn is_chat_completions_usable(config: &Config) -> bool {
    match config.llm_provider {
        LlmProvider::Extractive => false,
        LlmProvider::OpenAiCompatible => config.chat_completions.api_key.is_some(),
        LlmProvider::Custom => config.chat_completions.base_url_configured,
        LlmProvider::Claude => false,
    }
}

fn is_claude_messages_usable(config: &Config) -> bool {
    config.llm_provider == LlmProvider::Claude
        && (config.claude.api_key.is_some() || config.claude.base_url_configured)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use tempfile::tempdir;

    use super::{build_rag_service, is_probable_embedding_model};
    use crate::config::{
        ChatCompletionsProviderConfig, ClaudeMessagesProviderConfig, Config,
        EmbeddingProviderConfig, LlmProvider, RagMode,
    };

    #[test]
    fn openai_provider_without_key_falls_back_at_startup() {
        let dir = tempdir().expect("create temp dir");
        let config = Config {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            db_path: dir.path().join("knowledge-pilot.db"),
            api_token: None,
            ui_enabled: true,
            request_body_limit_bytes: 1024 * 1024,
            chunk_size: 800,
            chunk_overlap: 120,
            embedding: test_embedding_config(),
            rag_mode: RagMode::Agentic,
            rate_limit_rpm: 300,
            llm_provider: LlmProvider::OpenAiCompatible,
            chat_completions: ChatCompletionsProviderConfig {
                name: "openai-compatible".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                base_url_configured: false,
                path: "/chat/completions".to_string(),
                api_key: None,
                model: "gpt-4.1-mini".to_string(),
                auth_header: "Authorization".to_string(),
                auth_scheme: Some("Bearer".to_string()),
                extra_headers: Vec::new(),
            },
            claude: test_claude_config(None, false),
            skills_path: None,
        };

        build_rag_service(&config).expect("service should fall back to traditional RAG");
    }

    #[test]
    fn custom_provider_without_key_can_start_when_base_url_is_set() {
        let dir = tempdir().expect("create temp dir");
        let config = Config {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            db_path: dir.path().join("knowledge-pilot.db"),
            api_token: None,
            ui_enabled: true,
            request_body_limit_bytes: 1024 * 1024,
            chunk_size: 800,
            chunk_overlap: 120,
            embedding: test_embedding_config(),
            rag_mode: RagMode::Agentic,
            rate_limit_rpm: 300,
            llm_provider: LlmProvider::Custom,
            chat_completions: ChatCompletionsProviderConfig {
                name: "local-custom".to_string(),
                base_url: "http://127.0.0.1:9999/v1".to_string(),
                base_url_configured: true,
                path: "/chat/completions".to_string(),
                api_key: None,
                model: "local-model".to_string(),
                auth_header: "Authorization".to_string(),
                auth_scheme: Some("Bearer".to_string()),
                extra_headers: Vec::new(),
            },
            claude: test_claude_config(None, false),
            skills_path: None,
        };

        build_rag_service(&config).expect("custom local provider should be accepted");
    }

    #[test]
    fn claude_provider_without_key_falls_back_at_startup() {
        let dir = tempdir().expect("create temp dir");
        let config = Config {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            db_path: dir.path().join("knowledge-pilot.db"),
            api_token: None,
            ui_enabled: true,
            request_body_limit_bytes: 1024 * 1024,
            chunk_size: 800,
            chunk_overlap: 120,
            embedding: test_embedding_config(),
            rag_mode: RagMode::Agentic,
            rate_limit_rpm: 300,
            llm_provider: LlmProvider::Claude,
            chat_completions: test_chat_config(None, false),
            claude: test_claude_config(None, false),
            skills_path: None,
        };

        build_rag_service(&config).expect("service should fall back to traditional RAG");
    }

    #[test]
    fn ollama_embedding_rejects_generic_chat_model() {
        let dir = tempdir().expect("create temp dir");
        let mut config = test_config(dir.path().join("knowledge-pilot.db"));
        config.embedding = test_ollama_embedding_config("qwen3:8b", false);

        let error = match build_rag_service(&config) {
            Ok(_) => panic!("chat model should be rejected"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("does not look like an embedding model")
        );
    }

    #[test]
    fn ollama_embedding_allows_verified_custom_model_escape_hatch() {
        let dir = tempdir().expect("create temp dir");
        let mut config = test_config(dir.path().join("knowledge-pilot.db"));
        config.embedding = test_ollama_embedding_config("internal-vector-model", true);

        build_rag_service(&config).expect("verified custom embedding model should be accepted");
    }

    #[test]
    fn embedding_model_name_detection_covers_common_ollama_models() {
        assert!(is_probable_embedding_model("qwen3-embedding:0.6b"));
        assert!(is_probable_embedding_model("bge-m3"));
        assert!(is_probable_embedding_model("nomic-embed-text"));
        assert!(is_probable_embedding_model("mxbai-embed-large"));
        assert!(!is_probable_embedding_model("qwen3:8b"));
        assert!(!is_probable_embedding_model("llama3.1:8b"));
    }

    fn test_chat_config(
        api_key: Option<String>,
        base_url_configured: bool,
    ) -> ChatCompletionsProviderConfig {
        ChatCompletionsProviderConfig {
            name: "openai-compatible".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            base_url_configured,
            path: "/chat/completions".to_string(),
            api_key,
            model: "gpt-4.1-mini".to_string(),
            auth_header: "Authorization".to_string(),
            auth_scheme: Some("Bearer".to_string()),
            extra_headers: Vec::new(),
        }
    }

    fn test_embedding_config() -> EmbeddingProviderConfig {
        EmbeddingProviderConfig {
            provider: crate::config::EmbeddingProvider::Hash,
            base_url: "http://127.0.0.1:11434".to_string(),
            path: "/api/embed".to_string(),
            model: "hash-256".to_string(),
            allow_unverified_model: false,
        }
    }

    fn test_ollama_embedding_config(
        model: &str,
        allow_unverified_model: bool,
    ) -> EmbeddingProviderConfig {
        EmbeddingProviderConfig {
            provider: crate::config::EmbeddingProvider::Ollama,
            base_url: "http://127.0.0.1:11434".to_string(),
            path: "/api/embed".to_string(),
            model: model.to_string(),
            allow_unverified_model,
        }
    }

    fn test_config(db_path: std::path::PathBuf) -> Config {
        Config {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            db_path,
            api_token: None,
            ui_enabled: true,
            request_body_limit_bytes: 1024 * 1024,
            chunk_size: 800,
            chunk_overlap: 120,
            embedding: test_embedding_config(),
            rag_mode: RagMode::Auto,
            rate_limit_rpm: 300,
            llm_provider: LlmProvider::Extractive,
            chat_completions: test_chat_config(None, false),
            claude: test_claude_config(None, false),
            skills_path: None,
        }
    }

    fn test_claude_config(
        api_key: Option<String>,
        base_url_configured: bool,
    ) -> ClaudeMessagesProviderConfig {
        ClaudeMessagesProviderConfig {
            name: "claude".to_string(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            base_url_configured,
            path: "/messages".to_string(),
            api_key,
            model: "claude-sonnet-4-20250514".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            max_tokens: 1024,
            extra_headers: Vec::new(),
        }
    }
}
