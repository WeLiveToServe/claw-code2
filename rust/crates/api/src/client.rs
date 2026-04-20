use crate::error::ApiError;
use crate::prompt_cache::{PromptCache, PromptCacheRecord, PromptCacheStats};
use crate::providers::anthropic::{self, AnthropicClient, AuthSource};
use crate::providers::gemini::{self, GeminiClient};
use crate::providers::openai_compat::{self, OpenAiCompatClient, OpenAiCompatConfig};
use crate::providers::{self, ProviderKind, ResolvedBackend};
use crate::types::{MessageRequest, MessageResponse, StreamEvent};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ProviderClient {
    Anthropic(AnthropicClient),
    Xai(OpenAiCompatClient),
    OpenAi(OpenAiCompatClient),
    Gemini(GeminiClient),
}

impl ProviderClient {
    pub fn from_model(model: &str) -> Result<Self, ApiError> {
        Self::from_model_with_backend_and_anthropic_auth(model, None, None, None)
    }

    pub fn from_model_with_runtime_config(
        model: &str,
        runtime_config: Option<&runtime::RuntimeConfig>,
        explicit_backend: Option<&str>,
    ) -> Result<Self, ApiError> {
        Self::from_model_with_backend_and_anthropic_auth(
            model,
            runtime_config,
            explicit_backend,
            None,
        )
    }

    pub fn from_model_with_anthropic_auth(
        model: &str,
        anthropic_auth: Option<AuthSource>,
    ) -> Result<Self, ApiError> {
        Self::from_model_with_backend_and_anthropic_auth(model, None, None, anthropic_auth)
    }

    pub fn from_model_with_backend_and_anthropic_auth(
        model: &str,
        runtime_config: Option<&runtime::RuntimeConfig>,
        explicit_backend: Option<&str>,
        anthropic_auth: Option<AuthSource>,
    ) -> Result<Self, ApiError> {
        let resolved_model = providers::resolve_model_alias(model);
        let backend =
            providers::resolve_backend(&resolved_model, runtime_config, explicit_backend)?;
        match backend.provider {
            ProviderKind::Anthropic => {
                let auth = resolve_anthropic_auth_for_backend(&backend, anthropic_auth)?;
                let base_url = backend
                    .resolved_base_url()
                    .map_or_else(anthropic::read_base_url, |resolved| resolved.value);
                Ok(Self::Anthropic(
                    AnthropicClient::from_auth(auth).with_base_url(base_url),
                ))
            }
            ProviderKind::Gemini => {
                let primary_env = backend.auth_env.as_deref().unwrap_or("GEMINI_API_KEY");
                let api_key = read_env_non_empty(primary_env)?
                    .or_else(|| read_env_non_empty("GOOGLE_API_KEY").ok().flatten())
                    .ok_or_else(|| {
                        ApiError::Auth(format!(
                            "missing Gemini credentials; export {primary_env} (or GOOGLE_API_KEY) before calling the Gemini API",
                        ))
                    })?;
                let base_url = backend
                    .resolved_base_url()
                    .map_or_else(|| gemini::DEFAULT_BASE_URL.to_string(), |resolved| {
                        resolved.value
                    });
                Ok(Self::Gemini(GeminiClient::new(api_key).with_base_url(base_url)))
            }
            ProviderKind::Xai | ProviderKind::OpenAi => {
                let config = backend
                    .openai_compat_config()
                    .expect("non-Anthropic metadata should produce an OpenAI-compatible config");
                let client = OpenAiCompatClient::from_env(config)?;
                Ok(match backend.provider {
                    ProviderKind::Xai => Self::Xai(client),
                    ProviderKind::OpenAi => Self::OpenAi(client),
                    ProviderKind::Anthropic => unreachable!(),
                    ProviderKind::Gemini => unreachable!(),
                })
            }
        }
    }

    #[must_use]
    pub const fn provider_kind(&self) -> ProviderKind {
        match self {
            Self::Anthropic(_) => ProviderKind::Anthropic,
            Self::Xai(_) => ProviderKind::Xai,
            Self::OpenAi(_) => ProviderKind::OpenAi,
            Self::Gemini(_) => ProviderKind::Gemini,
        }
    }

    #[must_use]
    pub fn with_prompt_cache(self, prompt_cache: PromptCache) -> Self {
        match self {
            Self::Anthropic(client) => Self::Anthropic(client.with_prompt_cache(prompt_cache)),
            other => other,
        }
    }

    #[must_use]
    pub fn prompt_cache_stats(&self) -> Option<PromptCacheStats> {
        match self {
            Self::Anthropic(client) => client.prompt_cache_stats(),
            Self::Xai(_) | Self::OpenAi(_) | Self::Gemini(_) => None,
        }
    }

    #[must_use]
    pub fn take_last_prompt_cache_record(&self) -> Option<PromptCacheRecord> {
        match self {
            Self::Anthropic(client) => client.take_last_prompt_cache_record(),
            Self::Xai(_) | Self::OpenAi(_) | Self::Gemini(_) => None,
        }
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        match self {
            Self::Anthropic(client) => client.send_message(request).await,
            Self::Xai(client) | Self::OpenAi(client) => client.send_message(request).await,
            Self::Gemini(client) => client.send_message(request).await,
        }
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        match self {
            Self::Anthropic(client) => client
                .stream_message(request)
                .await
                .map(MessageStream::Anthropic),
            Self::Xai(client) | Self::OpenAi(client) => client
                .stream_message(request)
                .await
                .map(MessageStream::OpenAiCompat),
            Self::Gemini(client) => client
                .stream_message(request)
                .await
                .map(MessageStream::Gemini),
        }
    }
}

fn read_env_non_empty(key: &str) -> Result<Option<String>, ApiError> {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(providers::dotenv_value(key)),
        Err(error) => Err(ApiError::from(error)),
    }
}

fn resolve_anthropic_auth_for_backend(
    backend: &ResolvedBackend,
    anthropic_auth: Option<AuthSource>,
) -> Result<AuthSource, ApiError> {
    if let Some(auth) = anthropic_auth {
        return Ok(auth);
    }

    let uses_default_anthropic_env = backend.auth_env.as_deref() == Some("ANTHROPIC_API_KEY")
        && backend.auth_token_env.as_deref() == Some("ANTHROPIC_AUTH_TOKEN");
    if uses_default_anthropic_env {
        return AuthSource::from_env_or_saved();
    }

    let api_key = match backend.auth_env.as_deref() {
        Some(env_key) => read_env_non_empty(env_key)?,
        None => None,
    };
    let bearer_token = match backend.auth_token_env.as_deref() {
        Some(env_key) => read_env_non_empty(env_key)?,
        None => None,
    };

    match (api_key, bearer_token) {
        (Some(api_key), Some(bearer_token)) => Ok(AuthSource::ApiKeyAndBearer {
            api_key,
            bearer_token,
        }),
        (Some(api_key), None) => Ok(AuthSource::ApiKey(api_key)),
        (None, Some(bearer_token)) => Ok(AuthSource::BearerToken(bearer_token)),
        (None, None) => Err(ApiError::Auth(format!(
            "missing {} credentials; export {} before calling the {} API",
            backend.provider_label,
            backend.auth_env_vars().join(" or "),
            backend.provider_label
        ))),
    }
}

#[derive(Debug)]
pub enum MessageStream {
    Anthropic(anthropic::MessageStream),
    OpenAiCompat(openai_compat::MessageStream),
    Gemini(gemini::MessageStream),
}

impl MessageStream {
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Anthropic(stream) => stream.request_id(),
            Self::OpenAiCompat(stream) => stream.request_id(),
            Self::Gemini(stream) => stream.request_id(),
        }
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        match self {
            Self::Anthropic(stream) => stream.next_event().await,
            Self::OpenAiCompat(stream) => stream.next_event().await,
            Self::Gemini(stream) => stream.next_event().await,
        }
    }
}

pub use anthropic::{
    oauth_token_is_expired, resolve_saved_oauth_token, resolve_startup_auth_source, OAuthTokenSet,
};
#[must_use]
pub fn read_base_url() -> String {
    anthropic::read_base_url()
}

#[must_use]
pub fn read_xai_base_url() -> String {
    openai_compat::read_base_url(&OpenAiCompatConfig::xai())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::ProviderClient;
    use crate::providers::{detect_provider_kind, resolve_model_alias, ProviderKind};

    /// Serializes every test in this module that mutates process-wide
    /// environment variables so concurrent test threads cannot observe
    /// each other's partially-applied state.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn resolves_existing_and_grok_aliases() {
        assert_eq!(resolve_model_alias("opus"), "claude-opus-4-6");
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
    }

    #[test]
    fn provider_detection_prefers_model_family() {
        assert_eq!(detect_provider_kind("grok-3"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::Anthropic
        );
    }

    /// Snapshot-restore guard for a single environment variable. Mirrors
    /// the pattern used in `providers/mod.rs` tests: captures the original
    /// value on construction, applies the override, and restores on drop so
    /// tests leave the process env untouched even when they panic.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let original = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn dashscope_model_uses_dashscope_config_not_openai() {
        // Regression: qwen-plus was being routed to OpenAiCompatConfig::openai()
        // which reads OPENAI_API_KEY and points at api.openai.com, when it should
        // use OpenAiCompatConfig::dashscope() which reads DASHSCOPE_API_KEY and
        // points at dashscope.aliyuncs.com.
        let _lock = env_lock();
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("test-dashscope-key"));
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);

        let client = ProviderClient::from_model("qwen-plus");

        // Must succeed (not fail with "missing OPENAI_API_KEY")
        assert!(
            client.is_ok(),
            "qwen-plus with DASHSCOPE_API_KEY set should build successfully, got: {:?}",
            client.err()
        );

        // Verify it's the OpenAi variant pointed at the DashScope base URL.
        match client.unwrap() {
            ProviderClient::OpenAi(openai_client) => {
                assert!(
                    openai_client.base_url().contains("dashscope.aliyuncs.com"),
                    "qwen-plus should route to DashScope base URL (contains 'dashscope.aliyuncs.com'), got: {}",
                    openai_client.base_url()
                );
            }
            other => panic!("Expected ProviderClient::OpenAi for qwen-plus, got: {other:?}"),
        }
    }
}
