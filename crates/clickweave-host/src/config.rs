use clickweave_llm::LlmConfig;

/// Build an [`LlmConfig`] from the five caller-facing parameters.
///
/// Normalizes `Some("")` api keys to `None` so no-auth endpoints do not
/// receive a spurious bearer header. This matches the behaviour of
/// `EndpointConfig::into_llm_config` in the Tauri shell.
pub fn llm_config(
    base_url: String,
    model: String,
    api_key: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
) -> LlmConfig {
    LlmConfig {
        base_url,
        model,
        api_key: api_key.filter(|k| !k.is_empty()),
        temperature,
        max_tokens,
        ..LlmConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_fields_set_correctly() {
        let cfg = llm_config(
            "http://localhost:1234/v1".to_string(),
            "my-model".to_string(),
            Some("my-key".to_string()),
            Some(0.5),
            Some(2048),
        );
        assert_eq!(cfg.base_url, "http://localhost:1234/v1");
        assert_eq!(cfg.model, "my-model");
        assert_eq!(cfg.api_key, Some("my-key".to_string()));
        assert_eq!(cfg.temperature, Some(0.5));
        assert_eq!(cfg.max_tokens, Some(2048));
    }

    #[test]
    fn empty_api_key_normalized_to_none() {
        let cfg = llm_config(
            "http://localhost:1234/v1".to_string(),
            "my-model".to_string(),
            Some(String::new()),
            None,
            None,
        );
        assert_eq!(
            cfg.api_key, None,
            "Some(\"\") api key must become None to avoid spurious bearer header"
        );
    }

    #[test]
    fn non_empty_api_key_preserved() {
        let cfg = llm_config(
            "http://localhost:1234/v1".to_string(),
            "my-model".to_string(),
            Some("sk-abc123".to_string()),
            None,
            None,
        );
        assert_eq!(cfg.api_key, Some("sk-abc123".to_string()));
    }

    #[test]
    fn none_api_key_stays_none() {
        let cfg = llm_config(
            "http://localhost:1234/v1".to_string(),
            "my-model".to_string(),
            None,
            None,
            None,
        );
        assert_eq!(cfg.api_key, None);
    }
}
