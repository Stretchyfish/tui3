use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub instance_id: String,
    enabled: bool,
    installed: bool,
    availability: Option<String>,
    auth: Auth,
    usage_limits: Option<Limits>,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Auth {
    status: String,
    email: Option<String>,
    profile_id: Option<String>,
    workspace_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limits {
    credential_fingerprint: Option<String>,
    #[serde(default)]
    windows: Vec<Window>,
    unavailable: Option<Unavailable>,
}
#[derive(Deserialize)]
struct Unavailable {
    reason: String,
    #[cfg_attr(not(test), allow(dead_code))]
    message: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    id: String,
    kind: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
}

#[derive(Default)]
pub struct UsageState {
    providers: HashMap<String, ProviderUsage>,
    pub loaded: bool,
    pub error: Option<String>,
}

pub struct Remaining {
    pub percent: f64,
    pub stale: bool,
    pub resets_at: Option<String>,
}

impl UsageState {
    pub fn apply(&mut self, providers: Vec<ProviderUsage>) {
        self.providers = providers
            .into_iter()
            .map(|mut provider| {
                if provider.enabled
                    && provider.installed
                    && provider.auth.status != "unauthenticated"
                    && provider.availability.as_deref() != Some("unavailable")
                    && let Some(limits) = &mut provider.usage_limits
                    && limits.windows.is_empty()
                    && limits
                        .unavailable
                        .as_ref()
                        .is_some_and(|unavailable| unavailable.reason == "probeFailed")
                    && let Some(previous) = self.providers.get(&provider.instance_id)
                    && let Some(old_limits) = &previous.usage_limits
                {
                    let same_credentials =
                        limits
                            .credential_fingerprint
                            .as_ref()
                            .is_some_and(|fingerprint| {
                                old_limits.credential_fingerprint.as_ref() == Some(fingerprint)
                            });
                    let same_account = limits.credential_fingerprint.is_none()
                        && old_limits.credential_fingerprint.is_none()
                        && (provider.auth.email.is_some() || provider.auth.profile_id.is_some())
                        && provider.auth == previous.auth;
                    if same_credentials || same_account {
                        limits.windows.clone_from(&old_limits.windows);
                    }
                }
                (provider.instance_id.clone(), provider)
            })
            .collect();
        self.loaded = true;
        self.error = None;
    }

    /// All server-reported windows, including allowances for other models.
    #[cfg(test)]
    pub fn windows(&self, instance: &str) -> Vec<&Window> {
        self.providers
            .get(instance)
            .filter(|p| {
                p.enabled
                    && p.installed
                    && p.auth.status != "unauthenticated"
                    && p.availability.as_deref() != Some("unavailable")
            })
            .and_then(|p| p.usage_limits.as_ref())
            .filter(|limits| {
                !limits
                    .unavailable
                    .as_ref()
                    .is_some_and(|u| u.reason == "unsupported")
            })
            .map(|limits| {
                limits
                    .windows
                    .iter()
                    .filter(|w| {
                        w.used_percent.is_finite() && (0.0..=100.0).contains(&w.used_percent)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn notice(&self, instance: &str) -> Option<&str> {
        if let Some(error) = self.error.as_deref() {
            return Some(error);
        }
        let Some(provider) = self.providers.get(instance) else {
            return Some(if self.loaded {
                "No usage reported by T3."
            } else {
                "Loading usage…"
            });
        };
        if !provider.enabled
            || !provider.installed
            || provider.auth.status == "unauthenticated"
            || provider.availability.as_deref() == Some("unavailable")
        {
            return Some("Provider unavailable or signed out.");
        }
        if let Some(unavailable) = provider
            .usage_limits
            .as_ref()
            .and_then(|l| l.unavailable.as_ref())
        {
            return Some(unavailable.message.as_deref().unwrap_or(
                match unavailable.reason.as_str() {
                    "probeFailed" => "Usage refresh failed; displayed values may be stale.",
                    "unsupported" => "Usage reporting is unavailable for this provider.",
                    _ => "Usage is currently unavailable.",
                },
            ));
        }
        None
    }

    pub fn remaining(&self, instance: &str, model: &str, kind: &str) -> Option<Remaining> {
        let provider = self.providers.get(instance)?;
        if !provider.enabled
            || !provider.installed
            || provider.auth.status == "unauthenticated"
            || provider.availability.as_deref() == Some("unavailable")
        {
            return None;
        }
        let limits = provider.usage_limits.as_ref()?;
        let reason = limits
            .unavailable
            .as_ref()
            .map(|unavailable| unavailable.reason.as_str());
        if reason == Some("unsupported") {
            return None;
        }
        let candidates: Vec<_> = limits
            .windows
            .iter()
            .filter(|window| {
                window.kind == kind
                    && window.used_percent.is_finite()
                    && (0.0..=100.0).contains(&window.used_percent)
            })
            .collect();
        let family = ["opus", "sonnet", "haiku", "gemini", "gpt"]
            .into_iter()
            .find(|family| model.to_lowercase().contains(family));
        let applicable: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|window| {
                let general = matches!(
                    window.id.as_str(),
                    "primary" | "secondary" | "five_hour" | "seven_day" | "session" | "weekly"
                );
                let scoped = family.is_some_and(|family| {
                    window.label.to_lowercase().contains(family)
                        || window.id.to_lowercase().contains(family)
                });
                general || scoped || candidates.len() == 1
            })
            .collect();
        // Where a provider reports both a general and a model-specific allowance,
        // the more restrictive applicable window determines how much remains.
        let selected = applicable
            .into_iter()
            .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent))?;
        Some(Remaining {
            percent: 100.0 - selected.used_percent,
            stale: self.error.is_some() || reason == Some("probeFailed"),
            resets_at: selected.resets_at.clone(),
        })
    }
}

pub fn from_config_event(value: Value) -> Result<Option<Vec<ProviderUsage>>, String> {
    let providers = match value.get("type").and_then(Value::as_str) {
        Some("snapshot") => &value["config"]["providers"],
        Some("providerStatuses") => &value["payload"]["providers"],
        _ => return Ok(None),
    };
    serde_json::from_value(providers.clone())
        .map(Some)
        .map_err(|_| "Invalid T3 usage snapshot.".into())
}

pub fn circle(percent: f64) -> &'static str {
    if percent <= 0.0 {
        "○"
    } else if percent < 37.5 {
        "◔"
    } else if percent < 62.5 {
        "◑"
    } else if percent < 87.5 {
        "◕"
    } else {
        "●"
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub fn snapshot() -> Value {
        json!({"type":"snapshot","config":{"providers":[
            {"instanceId":"codex","enabled":true,"installed":true,"auth":{"status":"authenticated"},"usageLimits":{"windows":[
                {"id":"primary","kind":"session","label":"Session","usedPercent":25.0},
                {"id":"secondary","kind":"weekly","label":"Weekly","usedPercent":100.0}
            ]}},
            {"instanceId":"claude","enabled":true,"installed":true,"auth":{"status":"authenticated"},"usageLimits":{"windows":[
                {"id":"five_hour","kind":"session","label":"Session","usedPercent":0.0},
                {"id":"seven_day","kind":"weekly","label":"Weekly","usedPercent":30.0},
                {"id":"seven_day_sonnet","kind":"weekly","label":"Weekly · Sonnet","usedPercent":95.0}
            ]}}
        ]}})
    }

    #[test]
    fn unsupported_usage_preserves_server_explanation() {
        let mut value = snapshot();
        value["config"]["providers"][0]["usageLimits"] = json!({
            "windows": [], "unavailable": {"reason":"unsupported",
            "message":"Sign in with Google to see Antigravity limits."}
        });
        let mut state = UsageState::default();
        state.apply(from_config_event(value).unwrap().unwrap());
        assert!(state.windows("codex").is_empty());
        assert_eq!(
            state.notice("codex"),
            Some("Sign in with Google to see Antigravity limits.")
        );
    }

    #[test]
    fn converts_used_to_remaining_and_keeps_accounts_and_scoped_budgets_separate() {
        let mut state = UsageState::default();
        state.apply(from_config_event(snapshot()).unwrap().unwrap());
        assert_eq!(
            state
                .remaining("codex", "gpt-test", "session")
                .unwrap()
                .percent,
            75.0
        );
        assert_eq!(
            state
                .remaining("codex", "gpt-test", "weekly")
                .unwrap()
                .percent,
            0.0
        );
        assert_eq!(
            state
                .remaining("claude", "claude-opus", "weekly")
                .unwrap()
                .percent,
            70.0
        );
        assert_eq!(
            state
                .remaining("claude", "claude-sonnet", "weekly")
                .unwrap()
                .percent,
            5.0
        );
        assert!(
            state
                .remaining("other-account", "gpt-test", "session")
                .is_none()
        );
        assert_eq!(circle(0.0), "○");
        assert_eq!(circle(100.0), "●");
        assert_eq!(circle(75.0), "◕");
    }

    #[test]
    fn unavailable_and_invalid_data_are_not_shown_as_full_or_empty_allowances() {
        for replacement in [
            json!({"windows":[]}),
            json!({"windows":[{"id":"primary","kind":"session","label":"Session","usedPercent":101.0}]}),
            json!({"unavailable":{"reason":"unsupported"},"windows":[]}),
        ] {
            let mut value = snapshot();
            value["config"]["providers"][0]["usageLimits"] = replacement;
            let mut state = UsageState::default();
            state.apply(from_config_event(value).unwrap().unwrap());
            assert!(state.remaining("codex", "gpt-test", "session").is_none());
        }
        let mut value = snapshot();
        value["config"]["providers"][0]["auth"]["status"] = json!("unauthenticated");
        let mut state = UsageState::default();
        state.apply(from_config_event(value).unwrap().unwrap());
        assert!(state.remaining("codex", "gpt-test", "session").is_none());
    }

    #[test]
    fn failed_probes_keep_known_account_data_but_never_reuse_it_for_new_credentials() {
        let mut initial = snapshot();
        initial["config"]["providers"][0]["usageLimits"]["credentialFingerprint"] =
            json!("account-a");
        let mut state = UsageState::default();
        state.apply(from_config_event(initial.clone()).unwrap().unwrap());
        initial["config"]["providers"][0]["usageLimits"]["windows"] = json!([]);
        initial["config"]["providers"][0]["usageLimits"]["unavailable"] =
            json!({"reason":"probeFailed"});
        state.apply(from_config_event(initial.clone()).unwrap().unwrap());
        let remaining = state.remaining("codex", "gpt-test", "session").unwrap();
        assert_eq!(remaining.percent, 75.0);
        assert!(remaining.stale);
        initial["config"]["providers"][0]["usageLimits"]["credentialFingerprint"] =
            json!("account-b");
        state.apply(from_config_event(initial).unwrap().unwrap());
        assert!(state.remaining("codex", "gpt-test", "session").is_none());
    }

    #[test]
    fn retains_but_marks_stale_data_and_replaces_it_on_new_provider_snapshots() {
        let mut state = UsageState::default();
        state.apply(from_config_event(snapshot()).unwrap().unwrap());
        state.error = Some("connection lost".into());
        assert!(
            state
                .remaining("codex", "gpt-test", "session")
                .unwrap()
                .stale
        );
        let mut value = snapshot();
        value["config"]["providers"][0]["usageLimits"]["unavailable"] =
            json!({"reason":"probeFailed"});
        state.apply(from_config_event(value.clone()).unwrap().unwrap());
        assert!(
            state
                .remaining("codex", "gpt-test", "session")
                .unwrap()
                .stale
        );
        value["config"]["providers"][0]["usageLimits"]
            .as_object_mut()
            .unwrap()
            .remove("unavailable");
        let providers = value["config"]["providers"].clone();
        state.apply(
            from_config_event(json!({"type":"providerStatuses","payload":{"providers":providers}}))
                .unwrap()
                .unwrap(),
        );
        assert!(
            !state
                .remaining("codex", "gpt-test", "session")
                .unwrap()
                .stale
        );
        assert!(
            from_config_event(json!({"type":"settingsUpdated","payload":{}}))
                .unwrap()
                .is_none()
        );
    }
}
