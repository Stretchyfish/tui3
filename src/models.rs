use ratatui::widgets::ListState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub instance_id: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
}

impl ModelSelection {
    pub fn option_value(&self, id: &str) -> Option<&str> {
        let options = self.options.as_ref()?;
        if let Some(array) = options.as_array() {
            array.iter().find(|option| option["id"] == id)?["value"].as_str()
        } else {
            options.get(id)?.as_str()
        }
    }
}

#[derive(Clone)]
pub struct ModelChoice {
    pub selection: ModelSelection,
    pub label: String,
    pub reasoning: Option<ReasoningOption>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningOption {
    pub id: String,
    pub label: String,
    pub options: Vec<OptionChoice>,
    pub current_value: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionChoice {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Provider {
    instance_id: String,
    display_name: Option<String>,
    driver: String,
    enabled: bool,
    installed: bool,
    status: String,
    availability: Option<String>,
    auth: Auth,
    models: Vec<ProviderModel>,
}
#[derive(Deserialize)]
struct Auth {
    status: String,
}
#[derive(Deserialize)]
struct ProviderModel {
    slug: String,
    name: String,
    #[serde(default)]
    capabilities: ModelCapabilities,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelCapabilities {
    #[serde(default)]
    option_descriptors: Vec<Value>,
}

pub fn choices(value: Value) -> Result<Vec<ModelChoice>, String> {
    #[derive(Deserialize)]
    struct Config {
        providers: Vec<Provider>,
    }
    let config: Config =
        serde_json::from_value(value).map_err(|_| "Unexpected T3 model catalog format.")?;
    let mut choices = Vec::new();
    for provider in config.providers {
        if !provider.enabled
            || !provider.installed
            || provider.status == "error"
            || provider.status == "disabled"
            || provider.availability.as_deref() == Some("unavailable")
            || provider.auth.status == "unauthenticated"
        {
            continue;
        }
        let name = provider.display_name.as_deref().unwrap_or(&provider.driver);
        for model in provider.models {
            let reasoning: Option<ReasoningOption> = model
                .capabilities
                .option_descriptors
                .into_iter()
                .find_map(|value| {
                    let role = value.get("role").and_then(Value::as_str);
                    let id = value.get("id").and_then(Value::as_str);
                    if value.get("type").and_then(Value::as_str) != Some("select")
                        || (role != Some("reasoning") && id != Some("reasoningEffort"))
                    {
                        return None;
                    }
                    serde_json::from_value(value).ok()
                });
            let default = reasoning.as_ref().and_then(|descriptor| {
                descriptor.current_value.clone().or_else(|| {
                    descriptor
                        .options
                        .iter()
                        .find(|option| option.is_default)
                        .map(|option| option.id.clone())
                })
            });
            let options = reasoning
                .as_ref()
                .zip(default)
                .map(|(descriptor, value)| json!([{"id": descriptor.id, "value": value}]));
            choices.push(ModelChoice {
                label: format!("{name} · {}", model.name),
                selection: ModelSelection {
                    instance_id: provider.instance_id.clone(),
                    model: model.slug,
                    options,
                },
                reasoning,
            });
        }
    }
    choices.sort_by_key(|choice| choice.label.to_lowercase());
    Ok(choices)
}

#[derive(Clone)]
pub struct ModelChange {
    pub thread_id: String,
    pub command_id: String,
    pub selection: ModelSelection,
}
impl ModelChange {
    pub fn payload(&self) -> Value {
        json!({"type":"thread.model-selection.set", "commandId":self.command_id,
            "threadId":self.thread_id,"modelSelection":self.selection})
    }
}

impl ModelChoice {
    pub fn reasoning_value(&self) -> Option<&str> {
        let id = &self.reasoning.as_ref()?.id;
        self.selection.option_value(id)
    }

    pub fn cycle_reasoning(&mut self, forward: bool) {
        let Some(descriptor) = &self.reasoning else {
            return;
        };
        if descriptor.options.is_empty() {
            return;
        }
        let current = self
            .reasoning_value()
            .and_then(|value| {
                descriptor
                    .options
                    .iter()
                    .position(|option| option.id == value)
            })
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % descriptor.options.len()
        } else {
            (current + descriptor.options.len() - 1) % descriptor.options.len()
        };
        let mut selections = self
            .selection
            .options
            .as_ref()
            .map_or_else(Vec::new, |options| {
                if let Some(array) = options.as_array() {
                    array.clone()
                } else {
                    options.as_object().map_or_else(Vec::new, |object| {
                        object
                            .iter()
                            .map(|(id, value)| json!({"id": id, "value": value}))
                            .collect()
                    })
                }
            });
        selections.retain(|option| option["id"] != descriptor.id);
        selections.push(json!({"id": descriptor.id, "value": descriptor.options[next].id}));
        self.selection.options = Some(Value::Array(selections));
    }
}

pub enum ModelCommand {
    Load,
    Set(ModelChange),
}

#[derive(Default)]
pub struct ModelPicker {
    pub choices: Vec<ModelChoice>,
    pub open: bool,
    pub loading: bool,
    pub load_requested: bool,
    pub thread_id: Option<String>,
    pub list_state: ListState,
    pub reasoning_focus: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub notice_thread: Option<String>,
    pub change_requested: bool,
    pub pending: Option<ModelChange>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn catalog() -> Value {
        json!({"providers":[
            {"instanceId":"codex","driver":"codex","displayName":"Codex","enabled":true,"installed":true,"status":"ready","auth":{"status":"authenticated"},"models":[{"slug":"model-a","name":"Model A"},{"slug":"model-b","name":"Model B"}]},
            {"instanceId":"disabled","driver":"claude","enabled":false,"installed":true,"status":"disabled","auth":{"status":"authenticated"},"models":[{"slug":"hidden","name":"Hidden"}]},
            {"instanceId":"signed-out","driver":"other","enabled":true,"installed":true,"status":"ready","auth":{"status":"unauthenticated"},"models":[{"slug":"unavailable","name":"Unavailable"}]}
        ]})
    }

    #[test]
    fn lists_available_models_and_serializes_the_real_thread_setting_command() {
        let models = choices(catalog()).unwrap();
        assert_eq!(models.len(), 2);
        let request = ModelChange {
            thread_id: "target-thread".into(),
            command_id: "unique-command".into(),
            selection: models[1].selection.clone(),
        };
        assert_eq!(
            request.payload(),
            json!({"type":"thread.model-selection.set","commandId":"unique-command","threadId":"target-thread","modelSelection":{"instanceId":"codex","model":"model-b"}})
        );
    }
}
