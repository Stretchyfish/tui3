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

#[derive(Clone)]
pub struct ModelChoice {
    pub selection: ModelSelection,
    pub label: String,
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
            choices.push(ModelChoice {
                label: format!("{name} · {}", model.name),
                selection: ModelSelection {
                    instance_id: provider.instance_id.clone(),
                    model: model.slug,
                    options: None,
                },
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
