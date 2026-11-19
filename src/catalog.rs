use std::collections::HashSet;

use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub title: String,
    pub workspace_root: String,
    pub archived_at: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub worktree_path: Option<String>,
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub status: String,
    pub model_selection: Option<crate::models::ModelSelection>,
    #[serde(default)]
    pub runtime_mode: Option<String>,
    pub archived_at: Option<String>,
    pub deleted_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    #[serde(default)]
    pub archived_threads: Vec<Thread>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Selection {
    Project(String),
    Thread(String),
}

pub struct Row {
    pub selection: Selection,
    pub label: String,
}

impl Catalog {
    pub fn normalize(&mut self) {
        self.projects.retain(|project| project.deleted_at.is_none());
        self.threads.append(&mut self.archived_threads);
        let mut seen = HashSet::new();
        self.threads
            .retain(|thread| thread.deleted_at.is_none() && seen.insert(thread.id.clone()));
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for project in &self.projects {
            rows.push(Row {
                selection: Selection::Project(project.id.clone()),
                label: format!("{}{}", project.title, archived_label(&project.archived_at)),
            });
            for thread in self
                .threads
                .iter()
                .filter(|thread| thread.project_id == project.id)
            {
                rows.push(Row {
                    selection: Selection::Thread(thread.id.clone()),
                    label: format!(
                        "  · {}{}",
                        thread.title,
                        archived_label(&thread.archived_at)
                    ),
                });
            }
        }
        rows
    }
}

impl Thread {
    pub fn is_running(&self) -> bool {
        matches!(
            self.status.as_str(),
            "preparing" | "queued" | "starting" | "running" | "waiting"
        )
    }
}

fn archived_label(value: &Option<String>) -> &'static str {
    if value.is_some() { " [archived]" } else { "" }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn snapshot() -> Catalog {
        serde_json::from_str(r#"{
            "projects":[{"id":"p1","title":"First","workspaceRoot":"/first"},{"id":"p2","title":"Second","workspaceRoot":"/second"}],
            "threads":[{"id":"t1","projectId":"p1","title":"Conversation","status":"idle"}],
            "archivedThreads":[{"id":"t2","projectId":"p1","title":"Older conversation","status":"idle","archivedAt":"2026-01-01"}]
        }"#).unwrap()
    }

    #[test]
    fn groups_archived_threads_and_filters_deleted_and_duplicate_items() {
        let mut catalog = snapshot();
        let mut deleted = catalog.threads[0].clone();
        deleted.id = "deleted".into();
        deleted.deleted_at = Some("2026-01-01".into());
        catalog.threads.push(deleted);
        catalog.archived_threads.push(catalog.threads[0].clone());
        catalog.normalize();
        let rows = catalog.rows();
        assert_eq!(rows.len(), 4);
        assert!(rows[2].label.contains("[archived]"));
        assert!(matches!(&rows[3].selection, Selection::Project(id) if id == "p2"));
    }
}
