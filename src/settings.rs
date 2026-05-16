use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde_json::{Map, Value};

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub(crate) struct SettingsResolver {
    paths: Vec<PathBuf>,
}

impl SettingsResolver {
    pub(crate) fn for_project(project_root: &Path) -> Result<Self> {
        let user_settings = user_settings_path()?;
        let project_settings = project_root.join(".cawir").join("settings.json");
        let local_settings = project_root.join(".cawir").join("settings.local.json");

        Ok(Self::from_paths(vec![
            user_settings,
            project_settings,
            local_settings,
        ]))
    }

    pub(crate) fn from_paths(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    pub(crate) fn load(&self) -> Result<Value> {
        let mut merged = Value::Object(Map::new());

        for path in &self.paths {
            let settings = match std::fs::read_to_string(path) {
                Ok(contents) => contents,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(Error::Io(error)),
            };
            let settings = serde_json::from_str::<Value>(&settings).map_err(|error| {
                Error::Env(format!(
                    "failed to parse settings {}: {error}",
                    path.display()
                ))
            })?;

            deep_merge(&mut merged, settings);
        }

        Ok(merged)
    }
}

fn user_settings_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "cawir", "cawir")
        .ok_or_else(|| Error::Env("could not determine OS config directory".to_string()))?;

    Ok(dirs.config_dir().join("settings.json"))
}

fn deep_merge(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(&key) {
                    Some(existing) => deep_merge(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, source) => {
            *target = source;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn settings_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("cawir-settings-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}", prefix, std::process::id(), unique))
    }

    #[test]
    fn settings_resolver_deep_merges_user_project_and_local_files() {
        let root = settings_test_path("merge");
        let user = root.join("user.json");
        let project = root.join("project.json");
        let local = root.join("local.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            &user,
            r#"{
                "hooks": {
                    "pre_tool_use": [
                        { "type": "command", "command": "global-pre" }
                    ]
                },
                "nested": {
                    "keep": "user",
                    "override": "user"
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            &project,
            r#"{
                "nested": {
                    "project_only": true,
                    "override": "project"
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            &local,
            r#"{
                "hooks": {
                    "post_tool_use": [
                        { "type": "command", "command": "local-post" }
                    ]
                },
                "nested": {
                    "override": "local"
                }
            }"#,
        )
        .unwrap();

        let settings = SettingsResolver::from_paths(vec![user, project, local])
            .load()
            .unwrap();

        assert_eq!(
            settings,
            json!({
                "hooks": {
                    "pre_tool_use": [
                        { "type": "command", "command": "global-pre" }
                    ],
                    "post_tool_use": [
                        { "type": "command", "command": "local-post" }
                    ]
                },
                "nested": {
                    "keep": "user",
                    "project_only": true,
                    "override": "local"
                }
            })
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_settings_paths_are_cawir_owned() {
        let project = settings_test_path("project-paths");
        let resolver = SettingsResolver::for_project(&project).unwrap();

        assert!(resolver.paths[0].ends_with("settings.json"));
        assert!(!resolver.paths[0].to_string_lossy().contains(".claude"));
        assert_eq!(
            resolver.paths[1],
            project.join(".cawir").join("settings.json")
        );
        assert_eq!(
            resolver.paths[2],
            project.join(".cawir").join("settings.local.json")
        );

        if project.exists() {
            std::fs::remove_dir_all(project).unwrap();
        }
    }

    #[test]
    fn resolver_does_not_read_project_claude_settings() {
        let project = settings_test_path("ignore-claude");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::fs::create_dir_all(project.join(".cawir")).unwrap();
        std::fs::write(
            project.join(".claude").join("settings.json"),
            r#"{
                "hooks": {
                    "Stop": [
                        {
                            "matcher": "",
                            "hooks": [
                                { "type": "command", "command": "claude-code-hook" }
                            ]
                        }
                    ]
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            project.join(".cawir").join("settings.json"),
            r#"{
                "hooks": {
                    "pre_tool_use": [
                        { "type": "command", "command": "cawir-hook" }
                    ]
                }
            }"#,
        )
        .unwrap();

        let settings = SettingsResolver::for_project(&project)
            .unwrap()
            .load()
            .unwrap();

        assert_eq!(
            settings,
            json!({
                "hooks": {
                    "pre_tool_use": [
                        { "type": "command", "command": "cawir-hook" }
                    ]
                }
            })
        );

        std::fs::remove_dir_all(project).unwrap();
    }
}
