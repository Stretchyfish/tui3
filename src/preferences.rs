//! Local UI preferences, separate from authentication credentials.
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TypingStyle {
    #[default]
    Regular,
    Vim,
    Helix,
}

impl TypingStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Regular => "Regular",
            Self::Vim => "Vim",
            Self::Helix => "Helix",
        }
    }

    pub fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Regular, true) | (Self::Helix, false) => Self::Vim,
            (Self::Vim, true) | (Self::Regular, false) => Self::Helix,
            _ => Self::Regular,
        }
    }
}

#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutPreset {
    #[default]
    Balanced,
    Default,
    EditorFocused,
}

impl LayoutPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Default => "Chat-focused",
            Self::EditorFocused => "Editor-focused",
        }
    }

    pub fn workspace_width(self) -> u16 {
        match self {
            Self::Balanced => 40,
            Self::Default => 32,
            Self::EditorFocused => 50,
        }
    }

    pub fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Balanced, true) | (Self::EditorFocused, false) => Self::Default,
            (Self::Default, true) | (Self::Balanced, false) => Self::EditorFocused,
            _ => Self::Balanced,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Preferences {
    #[serde(default = "enabled")]
    auto_start_t3: bool,
    #[serde(default)]
    typing_style: TypingStyle,
    #[serde(default)]
    layout_preset: LayoutPreset,
    #[serde(default)]
    locations: BTreeMap<String, Location>,
}
fn enabled() -> bool {
    true
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            auto_start_t3: true,
            typing_style: Default::default(),
            layout_preset: Default::default(),
            locations: Default::default(),
        }
    }
}

pub struct ResumeStore {
    path: Option<PathBuf>,
    preferences: Preferences,
    environment: Option<String>,
    dirty: bool,
    pub warning: Option<String>,
}

impl ResumeStore {
    pub fn auto_start_t3(&self) -> bool {
        self.preferences.auto_start_t3
    }
    pub fn typing_style(&self) -> TypingStyle {
        self.preferences.typing_style
    }
    pub fn layout_preset(&self) -> LayoutPreset {
        self.preferences.layout_preset
    }
    pub fn load() -> Self {
        let path = crate::pairing::session_path()
            .ok()
            .map(|path| path.with_file_name("ui-state.json"));
        Self::at(path)
    }

    fn at(path: Option<PathBuf>) -> Self {
        let preferences = path
            .as_ref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        Self {
            path,
            preferences,
            environment: None,
            dirty: false,
            warning: None,
        }
    }

    pub fn activate(&mut self, environment: &str, app: &mut crate::app::App) {
        if self.environment.as_deref() == Some(environment) {
            return;
        }
        self.remember(app);
        app.clear_catalog();
        app.restore_location = self.preferences.locations.get(environment).cloned();
        self.environment = Some(environment.into());
    }

    pub fn remember(&mut self, app: &crate::app::App) {
        if self.preferences.auto_start_t3 != app.auto_start_t3 {
            self.preferences.auto_start_t3 = app.auto_start_t3;
            self.dirty = true;
            self.flush();
        }
        if self.preferences.typing_style != app.typing_style {
            self.preferences.typing_style = app.typing_style;
            self.dirty = true;
            self.flush();
        }
        if self.preferences.layout_preset != app.layout_preset {
            self.preferences.layout_preset = app.layout_preset;
            self.dirty = true;
            self.flush();
        }
        let (Some(environment), Some(location)) = (&self.environment, app.selected_location())
        else {
            return;
        };
        if self.preferences.locations.get(environment) == Some(&location) {
            return;
        }
        self.preferences
            .locations
            .insert(environment.clone(), location);
        self.dirty = true;
        self.flush();
    }

    pub fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        match self.write() {
            Ok(()) => {
                self.dirty = false;
                self.warning = None;
            }
            Err(error) => self.warning = Some(error),
        }
    }

    fn write(&self) -> Result<(), String> {
        let path = self
            .path
            .as_ref()
            .ok_or("Cannot locate the TUI3 config directory.")?;
        let directory = path.parent().ok_or("Invalid TUI3 preferences path.")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(|_| "Could not create the TUI3 preferences directory.")?;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Could not protect the TUI3 preferences directory.")?;
        }
        #[cfg(not(unix))]
        fs::create_dir_all(directory)
            .map_err(|_| "Could not create the TUI3 preferences directory.")?;
        let data = serde_json::to_vec(&self.preferences)
            .map_err(|_| "Could not encode TUI3 preferences.")?;
        let mut file = tempfile::NamedTempFile::new_in(directory)
            .map_err(|_| "Could not create a TUI3 preferences file.")?;
        file.write_all(&data)
            .map_err(|_| "Could not write TUI3 preferences.")?;
        file.persist(path)
            .map_err(|_| "Could not save TUI3 preferences.")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::App, catalog::Selection};

    #[test]
    fn balanced_is_default_and_saved_older_layouts_remain_valid() {
        let defaults: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.layout_preset, LayoutPreset::Balanced);
        for (saved, expected) in [
            ("default", LayoutPreset::Default),
            ("editor-focused", LayoutPreset::EditorFocused),
            ("balanced", LayoutPreset::Balanced),
        ] {
            let preferences: Preferences =
                serde_json::from_value(serde_json::json!({"layout_preset": saved})).unwrap();
            assert_eq!(preferences.layout_preset, expected);
        }
    }

    #[test]
    fn auto_start_defaults_on_for_new_and_old_preferences_and_saves_off_without_server() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ui-state.json");
        let mut store = ResumeStore::at(Some(path.clone()));
        assert!(store.auto_start_t3());
        std::fs::write(&path, r#"{"locations":{}}"#).unwrap();
        assert!(ResumeStore::at(Some(path.clone())).auto_start_t3());
        let app = App {
            auto_start_t3: false,
            ..Default::default()
        };
        store.remember(&app);
        assert!(!ResumeStore::at(Some(path)).auto_start_t3());
    }

    #[test]
    fn typing_style_is_saved_without_a_server_connection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ui-state.json");
        let mut store = ResumeStore::at(Some(path.clone()));
        assert_eq!(store.typing_style(), TypingStyle::Regular);
        let app = App {
            typing_style: TypingStyle::Helix,
            ..App::default()
        };
        store.remember(&app);
        assert!(store.warning.is_none());
        assert_eq!(
            ResumeStore::at(Some(path)).typing_style(),
            TypingStyle::Helix
        );
    }

    fn catalog() -> crate::catalog::Catalog {
        let mut catalog = crate::catalog::tests::snapshot();
        catalog.normalize();
        catalog
    }

    #[test]
    fn restores_thread_by_id_after_restart_and_catalog_reordering() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tui3/ui-state.json");
        let mut store = ResumeStore::at(Some(path.clone()));
        let mut app = App {
            authenticated: true,
            ..Default::default()
        };
        store.activate("environment-a", &mut app);
        app.update_catalog(catalog());
        app.move_selection(true);
        app.move_selection(true);
        store.remember(&app);
        let mut restarted = App {
            authenticated: true,
            ..Default::default()
        };
        let mut store = ResumeStore::at(Some(path.clone()));
        store.activate("environment-a", &mut restarted);
        let mut reordered = catalog();
        reordered.projects.reverse();
        restarted.update_catalog(reordered);
        restarted.sync_history_selection();
        assert_eq!(restarted.history_thread.as_deref(), Some("t2"));
        restarted.move_selection(false);
        restarted.update_catalog(catalog());
        assert!(matches!(restarted.selection(), Some(Selection::Thread(id)) if id == "t1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn preserves_separate_environments_and_does_not_overwrite_on_empty_startup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tui3/ui-state.json");
        let mut store = ResumeStore::at(Some(path.clone()));
        let mut app = App::default();
        store.activate("environment-a", &mut app);
        app.update_catalog(catalog());
        for _ in 0..3 {
            app.move_selection(true);
        }
        store.remember(&app);
        store.activate("environment-b", &mut app);
        app.update_catalog(catalog());
        app.move_selection(true);
        store.remember(&app);
        let mut restarted = App::default();
        let mut store = ResumeStore::at(Some(path));
        store.activate("environment-a", &mut restarted);
        let mut empty = catalog();
        empty.projects.clear();
        restarted.update_catalog(empty);
        store.remember(&restarted);
        assert!(restarted.selection().is_none());
        restarted.update_catalog(catalog());
        assert!(matches!(restarted.selection(), Some(Selection::Project(id)) if id == "p2"));
        store.activate("environment-b", &mut restarted);
        restarted.update_catalog(catalog());
        assert!(matches!(restarted.selection(), Some(Selection::Thread(id)) if id == "t1"));
    }

    #[test]
    fn deleted_thread_falls_back_to_its_project_then_first_available_item() {
        let mut app = App {
            restore_location: Some(Location {
                project_id: "p2".into(),
                thread_id: Some("deleted".into()),
            }),
            ..Default::default()
        };
        app.update_catalog(catalog());
        assert!(matches!(app.selection(), Some(Selection::Project(id)) if id == "p2"));
        app.clear_catalog();
        app.restore_location = Some(Location {
            project_id: "deleted-project".into(),
            thread_id: None,
        });
        app.update_catalog(catalog());
        assert!(matches!(app.selection(), Some(Selection::Project(id)) if id == "p1"));
    }

    #[test]
    fn missing_corrupt_and_unwritable_preferences_do_not_break_browsing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("broken.json");
        fs::write(&path, "not-json").unwrap();
        let mut store = ResumeStore::at(Some(path));
        let mut app = App::default();
        store.activate("environment-a", &mut app);
        app.update_catalog(catalog());
        assert!(app.selection().is_some());
        let blocker = directory.path().join("not-a-directory");
        fs::write(&blocker, "file").unwrap();
        let mut store = ResumeStore::at(Some(blocker.join("ui-state.json")));
        store.activate("environment-a", &mut app);
        app.update_catalog(catalog());
        store.remember(&app);
        assert!(store.warning.is_some());
        assert!(app.selection().is_some());
    }
}
