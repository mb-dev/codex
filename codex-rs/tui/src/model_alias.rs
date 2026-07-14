use codex_protocol::openai_models::ModelPreset;

const OPENAI_MODEL_PREFIX: &str = "openai-";
const CODEX_AUTO_PREFIX: &str = "codex-auto-";

pub(crate) fn canonical_picker_model(model: &str) -> &str {
    model.strip_prefix(OPENAI_MODEL_PREFIX).unwrap_or(model)
}

pub(crate) fn same_picker_model(lhs: &str, rhs: &str) -> bool {
    canonical_picker_model(lhs) == canonical_picker_model(rhs)
}

pub(crate) fn matches_picker_preset(model: &str, preset: &ModelPreset) -> bool {
    same_picker_model(model, preset.model.as_str())
}

pub(crate) fn find_matching_picker_preset<'a>(
    presets: &'a [ModelPreset],
    model: &str,
) -> Option<&'a ModelPreset> {
    presets
        .iter()
        .find(|preset| matches_picker_preset(model, preset))
}

pub(crate) fn persisted_picker_model(model: &str, prefix_openai_alias: bool) -> String {
    if prefix_openai_alias
        && !model.starts_with(OPENAI_MODEL_PREFIX)
        && !model.starts_with(CODEX_AUTO_PREFIX)
    {
        format!("{OPENAI_MODEL_PREFIX}{model}")
    } else {
        model.to_string()
    }
}

/// Whether a picker selection should be persisted with the `openai-` alias prefix.
///
/// True for the native OpenAI provider, and for OpenAI-compatible providers
/// reached through a custom `model_provider` (e.g. Snowflake/snowhouse) where the
/// configured model already carries the `openai-` alias the backend routes on.
/// Mirroring the user's existing alias style keeps a fresh pick from dropping a
/// prefix the provider requires.
pub(crate) fn should_prefix_openai_alias(is_openai_provider: bool, current_model: &str) -> bool {
    is_openai_provider || current_model.starts_with(OPENAI_MODEL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::openai_models::InputModality;
    use codex_protocol::openai_models::ReasoningEffort;
    use codex_protocol::openai_models::ReasoningEffortPreset;

    fn preset(model: &str) -> ModelPreset {
        ModelPreset {
            id: model.to_string(),
            model: model.to_string(),
            display_name: model.to_string(),
            description: String::new(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description: "medium".to_string(),
            }],
            supports_personality: false,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            availability_nux: None,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text],
        }
    }

    #[test]
    fn canonical_picker_model_strips_openai_prefix() {
        assert_eq!(canonical_picker_model("openai-gpt-5.4"), "gpt-5.4");
        assert_eq!(canonical_picker_model("gpt-5.4"), "gpt-5.4");
    }

    #[test]
    fn matching_picker_preset_accepts_openai_aliases() {
        let models = vec![preset("gpt-5.4")];

        assert!(matches_picker_preset("openai-gpt-5.4", &models[0]));
        assert_eq!(
            find_matching_picker_preset(&models, "openai-gpt-5.4")
                .map(|matched| matched.model.as_str()),
            Some("gpt-5.4")
        );
    }

    #[test]
    fn persisted_picker_model_prefixes_openai_models() {
        assert_eq!(
            persisted_picker_model("gpt-5.4", /*prefix_openai_alias*/ true),
            "openai-gpt-5.4"
        );
        assert_eq!(
            persisted_picker_model("codex-auto-fast", /*prefix_openai_alias*/ true),
            "codex-auto-fast"
        );
        assert_eq!(
            persisted_picker_model("gpt-5.4", /*prefix_openai_alias*/ false),
            "gpt-5.4"
        );
    }

    #[test]
    fn should_prefix_openai_alias_follows_provider_or_configured_alias() {
        // Native OpenAI provider always prefixes.
        assert!(should_prefix_openai_alias(
            /*is_openai_provider*/ true, "gpt-5.4"
        ));
        // Custom provider (e.g. snowhouse) whose configured model is already aliased.
        assert!(should_prefix_openai_alias(
            /*is_openai_provider*/ false,
            "openai-gpt-5.4"
        ));
        // Custom provider on a bare (non-OpenAI) model leaves the slug untouched.
        assert!(!should_prefix_openai_alias(
            /*is_openai_provider*/ false,
            "qwen3.6-35b-a3b"
        ));
    }
}
