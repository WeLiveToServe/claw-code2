#![allow(clippy::cast_possible_truncation)]
use std::future::Future;
use std::pin::Pin;

use runtime::{BackendTransport, RuntimeBackendConfig, RuntimeConfig};
use serde::Serialize;

use crate::error::ApiError;
use crate::types::{MessageRequest, MessageResponse};

pub mod anthropic;
pub mod openai_compat;

#[allow(dead_code)]
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

#[allow(dead_code)]
pub trait Provider {
    type Stream;

    fn send_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, MessageResponse>;

    fn stream_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, Self::Stream>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    Xai,
    OpenAi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider: ProviderKind,
    pub provider_label: &'static str,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
}

const ANTHROPIC_METADATA: ProviderMetadata = ProviderMetadata {
    provider: ProviderKind::Anthropic,
    provider_label: "anthropic",
    auth_env: "ANTHROPIC_API_KEY",
    base_url_env: "ANTHROPIC_BASE_URL",
    default_base_url: anthropic::DEFAULT_BASE_URL,
};

const XAI_METADATA: ProviderMetadata = ProviderMetadata {
    provider: ProviderKind::Xai,
    provider_label: "xai",
    auth_env: "XAI_API_KEY",
    base_url_env: "XAI_BASE_URL",
    default_base_url: openai_compat::DEFAULT_XAI_BASE_URL,
};

const OPENAI_COMPAT_METADATA: ProviderMetadata = ProviderMetadata {
    provider: ProviderKind::OpenAi,
    provider_label: "openai-compatible",
    auth_env: "OPENAI_API_KEY",
    base_url_env: "OPENAI_BASE_URL",
    default_base_url: openai_compat::DEFAULT_OPENAI_BASE_URL,
};

const DASHSCOPE_METADATA: ProviderMetadata = ProviderMetadata {
    provider: ProviderKind::OpenAi,
    provider_label: "dashscope",
    auth_env: "DASHSCOPE_API_KEY",
    base_url_env: "DASHSCOPE_BASE_URL",
    default_base_url: openai_compat::DEFAULT_DASHSCOPE_BASE_URL,
};

impl ProviderMetadata {
    #[must_use]
    pub fn openai_compat_config(self) -> Option<openai_compat::OpenAiCompatConfig> {
        match self.provider {
            ProviderKind::Anthropic => None,
            ProviderKind::Xai => Some(openai_compat::OpenAiCompatConfig::xai()),
            ProviderKind::OpenAi if self.auth_env == "DASHSCOPE_API_KEY" => {
                Some(openai_compat::OpenAiCompatConfig::dashscope())
            }
            ProviderKind::OpenAi => Some(openai_compat::OpenAiCompatConfig::openai()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendBaseUrlSource {
    Environment(String),
    Config,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBackendBaseUrl {
    pub value: String,
    pub source: BackendBaseUrlSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBackend {
    pub id: String,
    pub provider: ProviderKind,
    pub transport: BackendTransport,
    pub provider_label: String,
    pub auth_env: Option<String>,
    pub auth_token_env: Option<String>,
    pub base_url_env: Option<String>,
    pub configured_base_url: Option<String>,
    pub default_base_url: Option<String>,
    pub default_model: Option<String>,
    pub request_stream_usage: bool,
}

impl ResolvedBackend {
    #[must_use]
    pub fn auth_env_vars(&self) -> Vec<String> {
        let mut envs = Vec::new();
        if let Some(auth_env) = &self.auth_env {
            envs.push(auth_env.clone());
        }
        if let Some(auth_token_env) = &self.auth_token_env {
            envs.push(auth_token_env.clone());
        }
        envs
    }

    #[must_use]
    pub fn openai_compat_config(&self) -> Option<openai_compat::OpenAiCompatConfig> {
        (self.transport == BackendTransport::OpenAiCompatible).then(|| {
            openai_compat::OpenAiCompatConfig::custom(
                self.provider_label.clone(),
                self.auth_env.clone(),
                self.base_url_env.clone(),
                self.default_base_url.clone(),
                self.configured_base_url.clone(),
                self.request_stream_usage,
            )
        })
    }

    #[must_use]
    pub fn resolved_base_url(&self) -> Option<ResolvedBackendBaseUrl> {
        match self.transport {
            BackendTransport::OpenAiCompatible => {
                let config = self.openai_compat_config()?;
                let resolution = openai_compat::resolve_base_url(&config);
                Some(ResolvedBackendBaseUrl {
                    value: resolution.value,
                    source: match resolution.source {
                        openai_compat::BaseUrlSource::Environment(env_key) => {
                            BackendBaseUrlSource::Environment(env_key)
                        }
                        openai_compat::BaseUrlSource::Config => BackendBaseUrlSource::Config,
                        openai_compat::BaseUrlSource::Default => BackendBaseUrlSource::Default,
                    },
                })
            }
            BackendTransport::Anthropic => {
                if let Some(env_key) = self.base_url_env.as_deref() {
                    if let Some(value) = env_or_dotenv_value(env_key) {
                        return Some(ResolvedBackendBaseUrl {
                            value,
                            source: BackendBaseUrlSource::Environment(env_key.to_string()),
                        });
                    }
                }
                if let Some(configured_base_url) = self.configured_base_url.as_ref() {
                    return Some(ResolvedBackendBaseUrl {
                        value: configured_base_url.clone(),
                        source: BackendBaseUrlSource::Config,
                    });
                }
                self.default_base_url
                    .as_ref()
                    .map(|value| ResolvedBackendBaseUrl {
                        value: value.clone(),
                        source: BackendBaseUrlSource::Default,
                    })
            }
        }
    }
}

fn resolved_backend_from_metadata(
    id: impl Into<String>,
    metadata: ProviderMetadata,
) -> ResolvedBackend {
    ResolvedBackend {
        id: id.into(),
        provider: metadata.provider,
        transport: match metadata.provider {
            ProviderKind::Anthropic => BackendTransport::Anthropic,
            ProviderKind::Xai | ProviderKind::OpenAi => BackendTransport::OpenAiCompatible,
        },
        provider_label: metadata.provider_label.to_string(),
        auth_env: Some(metadata.auth_env.to_string()),
        auth_token_env: (metadata.provider == ProviderKind::Anthropic)
            .then_some("ANTHROPIC_AUTH_TOKEN".to_string()),
        base_url_env: Some(metadata.base_url_env.to_string()),
        configured_base_url: None,
        default_base_url: Some(metadata.default_base_url.to_string()),
        default_model: None,
        request_stream_usage: matches!(metadata.auth_env, "OPENAI_API_KEY"),
    }
}

fn builtin_backend(backend_id: &str) -> Option<ResolvedBackend> {
    match backend_id {
        "anthropic" => Some(ResolvedBackend {
            provider_label: "anthropic".to_string(),
            ..resolved_backend_from_metadata("anthropic", ANTHROPIC_METADATA)
        }),
        "openai" => Some(ResolvedBackend {
            id: "openai".to_string(),
            provider: ProviderKind::OpenAi,
            transport: BackendTransport::OpenAiCompatible,
            provider_label: "openai".to_string(),
            auth_env: Some("OPENAI_API_KEY".to_string()),
            auth_token_env: None,
            base_url_env: Some("OPENAI_BASE_URL".to_string()),
            configured_base_url: None,
            default_base_url: Some(openai_compat::DEFAULT_OPENAI_BASE_URL.to_string()),
            default_model: None,
            request_stream_usage: true,
        }),
        "xai" => Some(ResolvedBackend {
            provider_label: "xai".to_string(),
            ..resolved_backend_from_metadata("xai", XAI_METADATA)
        }),
        "dashscope" => Some(ResolvedBackend {
            provider_label: "dashscope".to_string(),
            ..resolved_backend_from_metadata("dashscope", DASHSCOPE_METADATA)
        }),
        "openrouter" => Some(ResolvedBackend {
            id: "openrouter".to_string(),
            provider: ProviderKind::OpenAi,
            transport: BackendTransport::OpenAiCompatible,
            provider_label: "openrouter".to_string(),
            auth_env: Some("OPENROUTER_API_KEY".to_string()),
            auth_token_env: None,
            base_url_env: Some("OPENROUTER_BASE_URL".to_string()),
            configured_base_url: None,
            default_base_url: Some("https://openrouter.ai/api/v1".to_string()),
            default_model: None,
            request_stream_usage: true,
        }),
        _ => None,
    }
}

fn resolved_backend_from_runtime_config(
    backend_id: &str,
    backend: &RuntimeBackendConfig,
) -> ResolvedBackend {
    let provider = match backend.transport {
        BackendTransport::Anthropic => ProviderKind::Anthropic,
        BackendTransport::OpenAiCompatible => ProviderKind::OpenAi,
    };
    ResolvedBackend {
        id: backend_id.to_string(),
        provider,
        transport: backend.transport,
        provider_label: backend.provider_label.clone(),
        auth_env: backend.api_key_env.clone().or_else(|| {
            (backend.transport == BackendTransport::Anthropic)
                .then_some("ANTHROPIC_API_KEY".to_string())
        }),
        auth_token_env: (backend.transport == BackendTransport::Anthropic)
            .then_some("ANTHROPIC_AUTH_TOKEN".to_string()),
        base_url_env: backend.base_url_env.clone().or_else(|| match backend.transport {
            BackendTransport::Anthropic => Some("ANTHROPIC_BASE_URL".to_string()),
            BackendTransport::OpenAiCompatible => None,
        }),
        configured_base_url: backend.base_url.clone(),
        default_base_url: Some(match backend.transport {
            BackendTransport::Anthropic => anthropic::DEFAULT_BASE_URL.to_string(),
            BackendTransport::OpenAiCompatible => openai_compat::DEFAULT_OPENAI_BASE_URL.to_string(),
        }),
        default_model: backend.default_model.clone(),
        request_stream_usage: backend.transport == BackendTransport::OpenAiCompatible,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelTokenLimit {
    pub max_output_tokens: u32,
    pub context_window_tokens: u32,
}

const MODEL_REGISTRY: &[(&str, ProviderMetadata)] = &[
    ("opus", ANTHROPIC_METADATA),
    ("sonnet", ANTHROPIC_METADATA),
    ("haiku", ANTHROPIC_METADATA),
    ("grok", XAI_METADATA),
    ("grok-3", XAI_METADATA),
    ("grok-mini", XAI_METADATA),
    ("grok-3-mini", XAI_METADATA),
    ("grok-2", XAI_METADATA),
];

#[must_use]
pub fn resolve_model_alias(model: &str) -> String {
    let trimmed = model.trim();
    let lower = trimmed.to_ascii_lowercase();
    MODEL_REGISTRY
        .iter()
        .find_map(|(alias, metadata)| {
            (*alias == lower).then_some(match metadata.provider {
                ProviderKind::Anthropic => match *alias {
                    "opus" => "claude-opus-4-6",
                    "sonnet" => "claude-sonnet-4-6",
                    "haiku" => "claude-haiku-4-5-20251213",
                    _ => trimmed,
                },
                ProviderKind::Xai => match *alias {
                    "grok" | "grok-3" => "grok-3",
                    "grok-mini" | "grok-3-mini" => "grok-3-mini",
                    "grok-2" => "grok-2",
                    _ => trimmed,
                },
                ProviderKind::OpenAi => trimmed,
            })
        })
        .map_or_else(|| trimmed.to_string(), ToOwned::to_owned)
}

#[must_use]
pub fn metadata_for_model(model: &str) -> Option<ProviderMetadata> {
    let canonical = resolve_model_alias(model);
    if canonical.starts_with("claude") {
        return Some(ANTHROPIC_METADATA);
    }
    if canonical.starts_with("grok") {
        return Some(XAI_METADATA);
    }
    // Explicit provider-namespaced models (e.g. "openai/gpt-4.1-mini") must
    // route to the correct provider regardless of which auth env vars are set.
    // Without this, detect_provider_kind falls through to the auth-sniffer
    // order and misroutes to Anthropic if ANTHROPIC_API_KEY is present.
    if canonical.starts_with("openai/")
        || canonical.starts_with("gpt-")
        || canonical.starts_with("gemini-")
    {
        return Some(OPENAI_COMPAT_METADATA);
    }
    // Alibaba DashScope compatible-mode endpoint. Routes qwen/* and bare
    // qwen-* model names (qwen-max, qwen-plus, qwen-turbo, qwen-qwq, etc.)
    // to the OpenAI-compat client pointed at DashScope's /compatible-mode/v1.
    // Uses the OpenAi provider kind because DashScope speaks the OpenAI REST
    // shape — only the base URL and auth env var differ.
    if canonical.starts_with("qwen/") || canonical.starts_with("qwen-") {
        return Some(DASHSCOPE_METADATA);
    }
    None
}

#[must_use]
pub const fn fallback_metadata_for_provider(kind: ProviderKind) -> ProviderMetadata {
    match kind {
        ProviderKind::Anthropic => ANTHROPIC_METADATA,
        ProviderKind::Xai => XAI_METADATA,
        ProviderKind::OpenAi => OPENAI_COMPAT_METADATA,
    }
}

#[must_use]
pub fn resolved_metadata_for_model(model: &str) -> ProviderMetadata {
    metadata_for_model(model)
        .unwrap_or_else(|| fallback_metadata_for_provider(detect_provider_kind(model)))
}

fn configured_backend_override() -> Option<String> {
    std::env::var("CLAW_BACKEND")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_named_backend(
    backend_id: &str,
    runtime_config: Option<&RuntimeConfig>,
) -> Result<ResolvedBackend, ApiError> {
    if let Some(runtime_backend) = runtime_config.and_then(|config| config.backends().get(backend_id))
    {
        return Ok(resolved_backend_from_runtime_config(
            backend_id,
            runtime_backend,
        ));
    }
    if let Some(builtin) = builtin_backend(backend_id) {
        return Ok(builtin);
    }
    Err(ApiError::Auth(format!(
        "unknown backend `{backend_id}`; configure it under settings.backends or choose one of anthropic, openai, xai, dashscope, openrouter"
    )))
}

fn legacy_resolved_backend_for_model(model: &str) -> ResolvedBackend {
    let metadata = resolved_metadata_for_model(model);
    resolved_backend_from_metadata(metadata.provider_label, metadata)
}

pub fn resolve_backend(
    model: &str,
    runtime_config: Option<&RuntimeConfig>,
    explicit_backend: Option<&str>,
) -> Result<ResolvedBackend, ApiError> {
    let backend_id = explicit_backend
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(configured_backend_override)
        .or_else(|| runtime_config.and_then(RuntimeConfig::backend).map(ToOwned::to_owned));

    match backend_id {
        Some(backend_id) => resolve_named_backend(&backend_id, runtime_config),
        None => Ok(legacy_resolved_backend_for_model(model)),
    }
}

#[must_use]
pub fn detect_provider_kind(model: &str) -> ProviderKind {
    if let Some(metadata) = metadata_for_model(model) {
        return metadata.provider;
    }
    if anthropic::has_auth_from_env_or_saved().unwrap_or(false) {
        return ProviderKind::Anthropic;
    }
    if openai_compat::has_api_key("OPENAI_API_KEY") {
        return ProviderKind::OpenAi;
    }
    if openai_compat::has_api_key("XAI_API_KEY") {
        return ProviderKind::Xai;
    }
    ProviderKind::Anthropic
}

#[must_use]
pub fn max_tokens_for_model(model: &str) -> u32 {
    model_token_limit(model).map_or_else(
        || {
            let canonical = resolve_model_alias(model);
            if canonical.contains("opus") {
                32_000
            } else {
                64_000
            }
        },
        |limit| limit.max_output_tokens,
    )
}

/// Returns the effective max output tokens for a model, preferring a plugin
/// override when present. Falls back to [`max_tokens_for_model`] when the
/// override is `None`.
#[must_use]
pub fn max_tokens_for_model_with_override(model: &str, plugin_override: Option<u32>) -> u32 {
    plugin_override.unwrap_or_else(|| max_tokens_for_model(model))
}

#[must_use]
pub fn model_token_limit(model: &str) -> Option<ModelTokenLimit> {
    let canonical = resolve_model_alias(model);
    let normalized = canonical
        .strip_prefix("openai/")
        .unwrap_or(canonical.as_str());
    match normalized {
        "claude-opus-4-6" => Some(ModelTokenLimit {
            max_output_tokens: 32_000,
            context_window_tokens: 200_000,
        }),
        "claude-sonnet-4-6" | "claude-haiku-4-5-20251213" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 200_000,
        }),
        "gpt-4.1" | "gpt-4.1-mini" | "gpt-4.1-nano" => Some(ModelTokenLimit {
            max_output_tokens: 32_768,
            context_window_tokens: 1_047_576,
        }),
        "grok-3" | "grok-3-mini" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 131_072,
        }),
        _ => None,
    }
}

pub fn preflight_message_request(request: &MessageRequest) -> Result<(), ApiError> {
    let Some(limit) = model_token_limit(&request.model) else {
        return Ok(());
    };

    let estimated_input_tokens = estimate_message_request_input_tokens(request);
    let estimated_total_tokens = estimated_input_tokens.saturating_add(request.max_tokens);
    if estimated_total_tokens > limit.context_window_tokens {
        return Err(ApiError::ContextWindowExceeded {
            model: resolve_model_alias(&request.model),
            estimated_input_tokens,
            requested_output_tokens: request.max_tokens,
            estimated_total_tokens,
            context_window_tokens: limit.context_window_tokens,
        });
    }

    Ok(())
}

fn estimate_message_request_input_tokens(request: &MessageRequest) -> u32 {
    let mut estimate = estimate_serialized_tokens(&request.messages);
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.system));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tools));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tool_choice));
    estimate
}

fn estimate_serialized_tokens<T: Serialize>(value: &T) -> u32 {
    serde_json::to_vec(value)
        .ok()
        .map_or(0, |bytes| (bytes.len() / 4 + 1) as u32)
}

/// Env var names used by other provider backends. When Anthropic auth
/// resolution fails we sniff these so we can hint the user that their
/// credentials probably belong to a different provider and suggest the
/// model-prefix routing fix that would select it.
const FOREIGN_PROVIDER_ENV_VARS: &[(&str, &str, &str)] = &[
    (
        "OPENAI_API_KEY",
        "OpenAI-compat",
        "prefix your model name with `openai/` (e.g. `--model openai/gpt-4.1-mini`) so prefix routing selects the OpenAI-compatible provider, and set `OPENAI_BASE_URL` if you are pointing at OpenRouter/Ollama/a local server",
    ),
    (
        "XAI_API_KEY",
        "xAI",
        "use an xAI model alias (e.g. `--model grok` or `--model grok-mini`) so the prefix router selects the xAI backend",
    ),
    (
        "DASHSCOPE_API_KEY",
        "Alibaba DashScope",
        "prefix your model name with `qwen/` or `qwen-` (e.g. `--model qwen-plus`) so prefix routing selects the DashScope backend",
    ),
];

/// Check whether an env var is set to a non-empty value either in the real
/// process environment or in the working-directory `.env` file. Mirrors the
/// credential discovery path used by `read_env_non_empty` so the hint text
/// stays truthful when users rely on `.env` instead of a real export.
fn env_or_dotenv_value(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => {
            dotenv_value(key).filter(|value| !value.is_empty())
        }
        Err(_) => None,
    }
}

fn env_or_dotenv_present(key: &str) -> bool {
    env_or_dotenv_value(key).is_some()
}

/// Produce a hint string describing the first foreign provider credential
/// that is present in the environment when Anthropic auth resolution has
/// just failed. Returns `None` when no foreign credential is set, in which
/// case the caller should fall back to the plain `missing_credentials`
/// error without a hint.
pub(crate) fn anthropic_missing_credentials_hint() -> Option<String> {
    for (env_var, provider_label, fix_hint) in FOREIGN_PROVIDER_ENV_VARS {
        if env_or_dotenv_present(env_var) {
            return Some(format!(
                "I see {env_var} is set — if you meant to use the {provider_label} provider, {fix_hint}."
            ));
        }
    }
    None
}

/// Build an Anthropic-specific `MissingCredentials` error, attaching a
/// hint suggesting the probable fix whenever a different provider's
/// credentials are already present in the environment. Anthropic call
/// sites should prefer this helper over `ApiError::missing_credentials`
/// so users who mistyped a model name or forgot the prefix get a useful
/// signal instead of a generic "missing Anthropic credentials" wall.
pub(crate) fn anthropic_missing_credentials() -> ApiError {
    const PROVIDER: &str = "Anthropic";
    const ENV_VARS: &[&str] = &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"];
    match anthropic_missing_credentials_hint() {
        Some(hint) => ApiError::missing_credentials_with_hint(PROVIDER, ENV_VARS, hint),
        None => ApiError::missing_credentials(PROVIDER, ENV_VARS),
    }
}

/// Parse a `.env` file body into key/value pairs using a minimal `KEY=VALUE`
/// grammar. Lines that are blank, start with `#`, or do not contain `=` are
/// ignored. Surrounding double or single quotes are stripped from the value.
/// An optional leading `export ` prefix on the key is also stripped so files
/// shared with shell `source` workflows still parse cleanly.
pub(crate) fn parse_dotenv(content: &str) -> std::collections::HashMap<String, String> {
    let mut values = std::collections::HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let trimmed_key = raw_key.trim();
        let key = trimmed_key
            .strip_prefix("export ")
            .map_or(trimmed_key, str::trim)
            .to_string();
        if key.is_empty() {
            continue;
        }
        let trimmed_value = raw_value.trim();
        let unquoted = if (trimmed_value.starts_with('"') && trimmed_value.ends_with('"')
            || trimmed_value.starts_with('\'') && trimmed_value.ends_with('\''))
            && trimmed_value.len() >= 2
        {
            &trimmed_value[1..trimmed_value.len() - 1]
        } else {
            trimmed_value
        };
        values.insert(key, unquoted.to_string());
    }
    values
}

/// Load and parse a `.env` file from the given path. Missing files yield
/// `None` instead of an error so callers can use this as a soft fallback.
pub(crate) fn load_dotenv_file(
    path: &std::path::Path,
) -> Option<std::collections::HashMap<String, String>> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(parse_dotenv(&content))
}

/// Look up `key` in a `.env` file located in the current working directory.
/// Returns `None` when the file is missing, the key is absent, or the value
/// is empty.
pub(crate) fn dotenv_value(key: &str) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let values = load_dotenv_file(&cwd.join(".env"))?;
    values.get(key).filter(|value| !value.is_empty()).cloned()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use runtime::ConfigLoader;

    use crate::error::ApiError;
    use crate::types::{
        InputContentBlock, InputMessage, MessageRequest, ToolChoice, ToolDefinition,
    };

    use super::{
        anthropic_missing_credentials, anthropic_missing_credentials_hint, detect_provider_kind,
        load_dotenv_file, max_tokens_for_model, max_tokens_for_model_with_override,
        model_token_limit, parse_dotenv, preflight_message_request, resolve_backend,
        resolve_model_alias, ProviderKind,
    };

    /// Serializes every test in this module that mutates process-wide
    /// environment variables so concurrent test threads cannot observe
    /// each other's partially-applied state while probing the foreign
    /// provider credential sniffer.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Snapshot-restore guard for a single environment variable. Captures
    /// the original value on construction, applies the requested override
    /// (set or remove), and restores the original on drop so tests leave
    /// the process env untouched even when they panic mid-assertion.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
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

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("api-providers-{nanos}"))
    }

    #[test]
    fn resolves_grok_aliases() {
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
        assert_eq!(resolve_model_alias("grok-2"), "grok-2");
    }

    #[test]
    fn detects_provider_from_model_name_first() {
        assert_eq!(detect_provider_kind("grok"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn openai_namespaced_model_routes_to_openai_not_anthropic() {
        // Regression: "openai/gpt-4.1-mini" was misrouted to Anthropic when
        // ANTHROPIC_API_KEY was set because metadata_for_model returned None
        // and detect_provider_kind fell through to auth-sniffer order.
        // The model prefix must win over env-var presence.
        let kind = super::metadata_for_model("openai/gpt-4.1-mini")
            .map(|m| m.provider)
            .unwrap_or_else(|| detect_provider_kind("openai/gpt-4.1-mini"));
        assert_eq!(
            kind,
            ProviderKind::OpenAi,
            "openai/ prefix must route to OpenAi regardless of ANTHROPIC_API_KEY"
        );

        // Also cover bare gpt- prefix
        let kind2 = super::metadata_for_model("gpt-4o")
            .map(|m| m.provider)
            .unwrap_or_else(|| detect_provider_kind("gpt-4o"));
        assert_eq!(kind2, ProviderKind::OpenAi);
    }

    #[test]
    fn gemini_prefix_routes_to_openai_not_anthropic() {
        let meta = super::metadata_for_model("gemini-2.5-flash")
            .expect("gemini-* prefix must resolve to OpenAI-compatible metadata");
        assert_eq!(meta.provider, ProviderKind::OpenAi);
        assert_eq!(meta.provider_label, "openai-compatible");
        assert_eq!(meta.auth_env, "OPENAI_API_KEY");
        assert_eq!(meta.base_url_env, "OPENAI_BASE_URL");

        let kind = detect_provider_kind("gemini-2.5-flash");
        assert_eq!(
            kind,
            ProviderKind::OpenAi,
            "gemini-* prefix must win over auth-sniffer order"
        );
    }

    #[test]
    fn qwen_prefix_routes_to_dashscope_not_anthropic() {
        // User request from Discord #clawcode-get-help: web3g wants to use
        // Qwen 3.6 Plus via native Alibaba DashScope API (not OpenRouter,
        // which has lower rate limits). metadata_for_model must route
        // qwen/* and bare qwen-* to the OpenAi provider kind pointed at
        // the DashScope compatible-mode endpoint, regardless of whether
        // ANTHROPIC_API_KEY is present in the environment.
        let meta = super::metadata_for_model("qwen/qwen-max")
            .expect("qwen/ prefix must resolve to DashScope metadata");
        assert_eq!(meta.provider, ProviderKind::OpenAi);
        assert_eq!(meta.provider_label, "dashscope");
        assert_eq!(meta.auth_env, "DASHSCOPE_API_KEY");
        assert_eq!(meta.base_url_env, "DASHSCOPE_BASE_URL");
        assert!(meta.default_base_url.contains("dashscope.aliyuncs.com"));

        // Bare qwen- prefix also routes
        let meta2 = super::metadata_for_model("qwen-plus")
            .expect("qwen- prefix must resolve to DashScope metadata");
        assert_eq!(meta2.provider, ProviderKind::OpenAi);
        assert_eq!(meta2.auth_env, "DASHSCOPE_API_KEY");

        // detect_provider_kind must agree even if ANTHROPIC_API_KEY is set
        let kind = detect_provider_kind("qwen/qwen3-coder");
        assert_eq!(
            kind,
            ProviderKind::OpenAi,
            "qwen/ prefix must win over auth-sniffer order"
        );
    }

    #[test]
    fn metadata_builds_matching_openai_compatible_transport_configs() {
        let openai_meta = super::metadata_for_model("gemini-2.5-flash")
            .expect("gemini-* prefix must resolve to OpenAI-compatible metadata");
        let openai_config = openai_meta
            .openai_compat_config()
            .expect("OpenAI-compatible metadata should yield a transport config");
        assert_eq!(openai_config.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert_eq!(openai_config.base_url_env.as_deref(), Some("OPENAI_BASE_URL"));

        let dashscope_meta = super::metadata_for_model("qwen-plus")
            .expect("qwen-* prefix must resolve to DashScope metadata");
        let dashscope_config = dashscope_meta
            .openai_compat_config()
            .expect("DashScope metadata should yield a transport config");
        assert_eq!(dashscope_config.api_key_env.as_deref(), Some("DASHSCOPE_API_KEY"));
        assert_eq!(
            dashscope_config.base_url_env.as_deref(),
            Some("DASHSCOPE_BASE_URL")
        );
    }

    #[test]
    fn explicit_openrouter_backend_overrides_qwen_prefix_routing() {
        let backend = resolve_backend("qwen/qwen3.5-27b", None, Some("openrouter"))
            .expect("explicit openrouter backend should resolve");
        assert_eq!(backend.id, "openrouter");
        assert_eq!(backend.provider, ProviderKind::OpenAi);
        assert_eq!(backend.provider_label, "openrouter");
        assert_eq!(backend.auth_env.as_deref(), Some("OPENROUTER_API_KEY"));
        assert_eq!(
            backend.resolved_base_url().map(|value| value.value),
            Some("https://openrouter.ai/api/v1".to_string())
        );
    }

    #[test]
    fn custom_openai_compatible_backend_resolves_from_runtime_config() {
        let root = temp_dir();
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        fs::create_dir_all(cwd.join(".claw")).expect("project config dir");
        fs::create_dir_all(&home).expect("home config dir");
        fs::write(
            cwd.join(".claw").join("settings.json"),
            r#"{
              "backend": "droplet-qwen",
              "backends": {
                "droplet-qwen": {
                  "transport": "openai-compatible",
                  "providerLabel": "droplet-qwen",
                  "apiKeyEnv": "DROPLET_QWEN_API_KEY",
                  "baseUrl": "http://127.0.0.1:8000/v1",
                  "defaultModel": "Qwen/Qwen2.5-VL-72B-Instruct-AWQ"
                }
              }
            }"#,
        )
        .expect("write backend config");

        let runtime_config = ConfigLoader::new(&cwd, &home)
            .load()
            .expect("config should load");
        let backend = resolve_backend("qwen-plus", Some(&runtime_config), None)
            .expect("custom backend should resolve");

        assert_eq!(backend.id, "droplet-qwen");
        assert_eq!(backend.provider, ProviderKind::OpenAi);
        assert_eq!(backend.provider_label, "droplet-qwen");
        assert_eq!(backend.auth_env.as_deref(), Some("DROPLET_QWEN_API_KEY"));
        assert_eq!(
            backend.default_model.as_deref(),
            Some("Qwen/Qwen2.5-VL-72B-Instruct-AWQ")
        );
        assert_eq!(
            backend.resolved_base_url().map(|value| value.value),
            Some("http://127.0.0.1:8000/v1".to_string())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn keeps_existing_max_token_heuristic() {
        assert_eq!(max_tokens_for_model("opus"), 32_000);
        assert_eq!(max_tokens_for_model("grok-3"), 64_000);
    }

    #[test]
    fn gpt_4_1_family_uses_openai_token_limits_even_with_prefix() {
        assert_eq!(max_tokens_for_model("gpt-4.1-mini"), 32_768);
        assert_eq!(max_tokens_for_model("openai/gpt-4.1-mini"), 32_768);

        let limits = model_token_limit("openai/gpt-4.1-mini")
            .expect("gpt-4.1-mini should expose token limits");
        assert_eq!(limits.max_output_tokens, 32_768);
        assert_eq!(limits.context_window_tokens, 1_047_576);
    }

    #[test]
    fn plugin_config_max_output_tokens_overrides_model_default() {
        // given
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("api-plugin-max-tokens-{nanos}"));
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        std::fs::create_dir_all(cwd.join(".claw")).expect("project config dir");
        std::fs::create_dir_all(&home).expect("home config dir");
        std::fs::write(
            home.join("settings.json"),
            r#"{
              "plugins": {
                "maxOutputTokens": 12345
              }
            }"#,
        )
        .expect("write plugin settings");

        // when
        let loaded = runtime::ConfigLoader::new(&cwd, &home)
            .load()
            .expect("config should load");
        let plugin_override = loaded.plugins().max_output_tokens();
        let effective = max_tokens_for_model_with_override("claude-opus-4-6", plugin_override);

        // then
        assert_eq!(plugin_override, Some(12345));
        assert_eq!(effective, 12345);
        assert_ne!(effective, max_tokens_for_model("claude-opus-4-6"));

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn max_tokens_for_model_with_override_falls_back_when_plugin_unset() {
        // given
        let plugin_override: Option<u32> = None;

        // when
        let effective = max_tokens_for_model_with_override("claude-opus-4-6", plugin_override);

        // then
        assert_eq!(effective, max_tokens_for_model("claude-opus-4-6"));
        assert_eq!(effective, 32_000);
    }

    #[test]
    fn returns_context_window_metadata_for_supported_models() {
        assert_eq!(
            model_token_limit("claude-sonnet-4-6")
                .expect("claude-sonnet-4-6 should be registered")
                .context_window_tokens,
            200_000
        );
        assert_eq!(
            model_token_limit("grok-mini")
                .expect("grok-mini should resolve to a registered model")
                .context_window_tokens,
            131_072
        );
    }

    #[test]
    fn preflight_blocks_requests_that_exceed_the_model_context_window() {
        let request = MessageRequest {
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(600_000),
                }],
            }],
            system: Some("Keep the answer short.".to_string()),
            tools: Some(vec![ToolDefinition {
                name: "weather".to_string(),
                description: Some("Fetches weather".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": { "city": { "type": "string" } },
                }),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
            ..Default::default()
        };

        let error = preflight_message_request(&request)
            .expect_err("oversized request should be rejected before the provider call");

        match error {
            ApiError::ContextWindowExceeded {
                model,
                estimated_input_tokens,
                requested_output_tokens,
                estimated_total_tokens,
                context_window_tokens,
            } => {
                assert_eq!(model, "claude-sonnet-4-6");
                assert!(estimated_input_tokens > 136_000);
                assert_eq!(requested_output_tokens, 64_000);
                assert!(estimated_total_tokens > context_window_tokens);
                assert_eq!(context_window_tokens, 200_000);
            }
            other => panic!("expected context-window preflight failure, got {other:?}"),
        }
    }

    #[test]
    fn preflight_skips_unknown_models() {
        let request = MessageRequest {
            model: "unknown-model".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(600_000),
                }],
            }],
            system: None,
            tools: None,
            tool_choice: None,
            stream: false,
            ..Default::default()
        };

        preflight_message_request(&request)
            .expect("models without context metadata should skip the guarded preflight");
    }

    #[test]
    fn parse_dotenv_extracts_keys_handles_comments_quotes_and_export_prefix() {
        // given
        let body = "\
# this is a comment

ANTHROPIC_API_KEY=plain-value
XAI_API_KEY=\"quoted-value\"
OPENAI_API_KEY='single-quoted'
export GROK_API_KEY=exported-value
   PADDED_KEY  =  padded-value  
EMPTY_VALUE=
NO_EQUALS_LINE
";

        // when
        let values = parse_dotenv(body);

        // then
        assert_eq!(
            values.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("plain-value")
        );
        assert_eq!(
            values.get("XAI_API_KEY").map(String::as_str),
            Some("quoted-value")
        );
        assert_eq!(
            values.get("OPENAI_API_KEY").map(String::as_str),
            Some("single-quoted")
        );
        assert_eq!(
            values.get("GROK_API_KEY").map(String::as_str),
            Some("exported-value")
        );
        assert_eq!(
            values.get("PADDED_KEY").map(String::as_str),
            Some("padded-value")
        );
        assert_eq!(values.get("EMPTY_VALUE").map(String::as_str), Some(""));
        assert!(!values.contains_key("NO_EQUALS_LINE"));
        assert!(!values.contains_key("# this is a comment"));
    }

    #[test]
    fn load_dotenv_file_reads_keys_from_disk_and_returns_none_when_missing() {
        // given
        let temp_root = std::env::temp_dir().join(format!(
            "api-dotenv-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&temp_root).expect("create temp dir");
        let env_path = temp_root.join(".env");
        std::fs::write(
            &env_path,
            "ANTHROPIC_API_KEY=secret-from-file\n# comment\nXAI_API_KEY=\"xai-secret\"\n",
        )
        .expect("write .env");
        let missing_path = temp_root.join("does-not-exist.env");

        // when
        let loaded = load_dotenv_file(&env_path).expect("file should load");
        let missing = load_dotenv_file(&missing_path);

        // then
        assert_eq!(
            loaded.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("secret-from-file")
        );
        assert_eq!(
            loaded.get("XAI_API_KEY").map(String::as_str),
            Some("xai-secret")
        );
        assert!(missing.is_none());

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn anthropic_missing_credentials_hint_is_none_when_no_foreign_creds_present() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint();

        // then
        assert!(
            hint.is_none(),
            "no hint should be produced when every foreign provider env var is absent, got {hint:?}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_detects_openai_api_key_and_recommends_openai_prefix() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("sk-openrouter-varleg"));
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("OPENAI_API_KEY presence should produce a hint");

        // then
        assert!(
            hint.contains("OPENAI_API_KEY is set"),
            "hint should name the detected env var so users recognize it: {hint}"
        );
        assert!(
            hint.contains("OpenAI-compat"),
            "hint should identify the target provider: {hint}"
        );
        assert!(
            hint.contains("openai/"),
            "hint should mention the `openai/` prefix routing fix: {hint}"
        );
        assert!(
            hint.contains("OPENAI_BASE_URL"),
            "hint should mention OPENAI_BASE_URL so OpenRouter users see the full picture: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_detects_xai_api_key() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", Some("xai-test-key"));
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("XAI_API_KEY presence should produce a hint");

        // then
        assert!(
            hint.contains("XAI_API_KEY is set"),
            "hint should name XAI_API_KEY: {hint}"
        );
        assert!(
            hint.contains("xAI"),
            "hint should identify the xAI provider: {hint}"
        );
        assert!(
            hint.contains("grok"),
            "hint should suggest a grok-prefixed model alias: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_detects_dashscope_api_key() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("sk-dashscope-test"));

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("DASHSCOPE_API_KEY presence should produce a hint");

        // then
        assert!(
            hint.contains("DASHSCOPE_API_KEY is set"),
            "hint should name DASHSCOPE_API_KEY: {hint}"
        );
        assert!(
            hint.contains("DashScope"),
            "hint should identify the DashScope provider: {hint}"
        );
        assert!(
            hint.contains("qwen"),
            "hint should suggest a qwen-prefixed model alias: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_prefers_openai_when_multiple_foreign_creds_set() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("sk-openrouter-varleg"));
        let _xai = EnvVarGuard::set("XAI_API_KEY", Some("xai-test-key"));
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("sk-dashscope-test"));

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("multiple foreign creds should still produce a hint");

        // then
        assert!(
            hint.contains("OPENAI_API_KEY"),
            "OpenAI should be prioritized because it is the most common misrouting pattern (OpenRouter users), got: {hint}"
        );
        assert!(
            !hint.contains("XAI_API_KEY"),
            "only the first detected provider should be named to keep the hint focused, got: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_builds_error_with_canonical_env_vars_and_no_hint_when_clean() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let error = anthropic_missing_credentials();

        // then
        match &error {
            ApiError::MissingCredentials {
                provider,
                env_vars,
                hint,
            } => {
                assert_eq!(*provider, "Anthropic");
                assert_eq!(*env_vars, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]);
                assert!(
                    hint.is_none(),
                    "clean environment should not generate a hint, got {hint:?}"
                );
            }
            other => panic!("expected MissingCredentials variant, got {other:?}"),
        }
        let rendered = error.to_string();
        assert!(
            !rendered.contains(" — hint: "),
            "rendered error should be a plain missing-creds message: {rendered}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_builds_error_with_hint_when_openai_key_is_set() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("sk-openrouter-varleg"));
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let error = anthropic_missing_credentials();

        // then
        match &error {
            ApiError::MissingCredentials {
                provider,
                env_vars,
                hint,
            } => {
                assert_eq!(*provider, "Anthropic");
                assert_eq!(*env_vars, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]);
                let hint_value = hint.as_deref().expect("hint should be populated");
                assert!(
                    hint_value.contains("OPENAI_API_KEY is set"),
                    "hint should name the detected env var: {hint_value}"
                );
            }
            other => panic!("expected MissingCredentials variant, got {other:?}"),
        }
        let rendered = error.to_string();
        assert!(
            rendered.starts_with("missing Anthropic credentials;"),
            "canonical base message should still lead the rendered error: {rendered}"
        );
        assert!(
            rendered.contains(" — hint: I see OPENAI_API_KEY is set"),
            "rendered error should carry the env-driven hint: {rendered}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_ignores_empty_string_values() {
        // given
        let _lock = env_lock();
        // An empty value is semantically equivalent to "not set" for the
        // credential discovery path, so the sniffer must treat it that way
        // to avoid false-positive hints for users who intentionally cleared
        // a stale export with `OPENAI_API_KEY=`.
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some(""));
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint();

        // then
        assert!(
            hint.is_none(),
            "empty env var should not trigger the hint sniffer, got {hint:?}"
        );
    }
}
