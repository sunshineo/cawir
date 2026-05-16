use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result};

pub(crate) const SKILL_MANIFEST_FILE: &str = "cawir-skill.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Skill {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) trigger_guidance: Vec<String>,
    pub(crate) instructions: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkillMetadata {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) trigger_guidance: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SkillEntry {
    metadata: SkillMetadata,
    manifest_path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SkillCatalog {
    skills: Vec<SkillEntry>,
}

impl SkillCatalog {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn from_settings(
        settings: &Value,
        project_root: &Path,
        plugin_skill_directories: Vec<PathBuf>,
    ) -> Result<Self> {
        let mut skill_directories = configured_skill_directories(settings, project_root)?;
        skill_directories.extend(plugin_skill_directories);

        let mut skills = Vec::new();
        for directory in deduplicate_paths(skill_directories) {
            skills.extend(discover_skills_in_directory(&directory)?);
        }

        skills.sort_by(|left, right| {
            left.metadata
                .name
                .cmp(&right.metadata.name)
                .then_with(|| left.manifest_path.cmp(&right.manifest_path))
        });
        reject_duplicate_names(&skills)?;

        Ok(Self { skills })
    }

    #[cfg(test)]
    pub(crate) fn metadata(&self) -> Vec<&SkillMetadata> {
        self.skills.iter().map(|skill| &skill.metadata).collect()
    }

    pub(crate) fn activate_for_prompt(&self, prompt: &str) -> Result<Vec<Skill>> {
        let prompt_lower = prompt.to_ascii_lowercase();
        let prompt_tokens = prompt
            .split(|character: char| {
                !(character.is_ascii_alphanumeric() || character == '_' || character == '-')
            })
            .filter(|token| !token.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<BTreeSet<_>>();

        self.skills
            .iter()
            .filter(|skill| skill_matches_prompt(&skill.metadata, &prompt_lower, &prompt_tokens))
            .map(load_active_skill)
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct RawSkillMetadata {
    name: String,
    description: String,
    #[serde(default, alias = "triggers")]
    trigger_guidance: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawSkillInstructions {
    instructions: String,
}

fn configured_skill_directories(settings: &Value, project_root: &Path) -> Result<Vec<PathBuf>> {
    let mut directories = Vec::new();
    if let Some(value) = settings
        .get("skill_dirs")
        .or_else(|| settings.get("skillDirs"))
    {
        append_configured_directories(value, project_root, &mut directories)?;
    }
    if let Some(value) = settings
        .get("skills")
        .and_then(|skills| skills.get("directories").or_else(|| skills.get("dirs")))
    {
        append_configured_directories(value, project_root, &mut directories)?;
    }

    Ok(directories)
}

fn append_configured_directories(
    value: &Value,
    project_root: &Path,
    directories: &mut Vec<PathBuf>,
) -> Result<()> {
    let array = value.as_array().ok_or_else(|| {
        Error::Env("settings.skills.directories must be an array of paths".to_string())
    })?;

    for value in array {
        let path = value.as_str().ok_or_else(|| {
            Error::Env("settings.skills.directories entries must be strings".to_string())
        })?;
        let path = resolve_configured_path(path, project_root);
        let path = path.canonicalize().map_err(|error| {
            Error::Env(format!(
                "failed to resolve skill directory {}: {error}",
                path.display()
            ))
        })?;
        directories.push(path);
    }

    Ok(())
}

fn resolve_configured_path(path: &str, project_root: &Path) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }

    unique
}

fn discover_skills_in_directory(directory: &Path) -> Result<Vec<SkillEntry>> {
    if directory.join(SKILL_MANIFEST_FILE).is_file() {
        return Ok(vec![load_skill(directory)?]);
    }

    let mut children = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join(SKILL_MANIFEST_FILE).is_file() {
            children.push(entry.path());
        }
    }
    children.sort();

    children
        .iter()
        .map(|child| load_skill(child))
        .collect::<Result<Vec<_>>>()
}

fn load_skill(root: &Path) -> Result<SkillEntry> {
    let manifest_path = root.canonicalize()?.join(SKILL_MANIFEST_FILE);
    let contents = std::fs::read_to_string(&manifest_path)?;
    let raw = serde_json::from_str::<RawSkillMetadata>(&contents).map_err(|error| {
        Error::Env(format!(
            "failed to parse skill metadata {}: {error}",
            manifest_path.display()
        ))
    })?;

    let mut metadata = SkillMetadata {
        name: raw.name,
        description: raw.description,
        trigger_guidance: raw.trigger_guidance,
    };
    normalize_metadata(&mut metadata);
    validate_metadata(&metadata)?;

    Ok(SkillEntry {
        metadata,
        manifest_path,
    })
}

fn load_active_skill(entry: &SkillEntry) -> Result<Skill> {
    let contents = std::fs::read_to_string(&entry.manifest_path)?;
    let raw = serde_json::from_str::<RawSkillInstructions>(&contents).map_err(|error| {
        Error::Env(format!(
            "failed to parse skill instructions {}: {error}",
            entry.manifest_path.display()
        ))
    })?;
    let instructions = raw.instructions.trim().to_string();
    validate_instructions(&entry.metadata.name, &instructions)?;

    Ok(Skill {
        name: entry.metadata.name.clone(),
        description: entry.metadata.description.clone(),
        trigger_guidance: entry.metadata.trigger_guidance.clone(),
        instructions,
    })
}

fn normalize_metadata(metadata: &mut SkillMetadata) {
    metadata.name = metadata.name.trim().to_string();
    metadata.description = metadata.description.trim().to_string();
    metadata.trigger_guidance = metadata
        .trigger_guidance
        .iter()
        .map(|trigger| trigger.trim().to_string())
        .filter(|trigger| !trigger.is_empty())
        .collect();
}

fn validate_metadata(metadata: &SkillMetadata) -> Result<()> {
    if metadata.name.is_empty() || !is_valid_skill_name(&metadata.name) {
        return Err(Error::Env(format!(
            "skill has an unusable name: {}",
            metadata.name
        )));
    }
    if metadata.description.is_empty() {
        return Err(Error::Env(format!(
            "skill {} must include a description",
            metadata.name
        )));
    }

    Ok(())
}

fn validate_instructions(skill_name: &str, instructions: &str) -> Result<()> {
    if instructions.is_empty() {
        return Err(Error::Env(format!(
            "skill {skill_name} must include instructions"
        )));
    }

    Ok(())
}

fn is_valid_skill_name(name: &str) -> bool {
    name.chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

fn reject_duplicate_names(skills: &[SkillEntry]) -> Result<()> {
    for pair in skills.windows(2) {
        if pair[0].metadata.name == pair[1].metadata.name {
            return Err(Error::Env(format!(
                "duplicate skill name configured: {}",
                pair[0].metadata.name
            )));
        }
    }

    Ok(())
}

fn skill_matches_prompt(
    skill: &SkillMetadata,
    prompt_lower: &str,
    prompt_tokens: &BTreeSet<String>,
) -> bool {
    let name = skill.name.to_ascii_lowercase();
    prompt_tokens.contains(&name)
        || prompt_lower.contains(&format!("${name}"))
        || skill
            .trigger_guidance
            .iter()
            .map(|trigger| trigger.to_ascii_lowercase())
            .any(|trigger| prompt_lower.contains(&trigger))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn skill_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("cawir-skill-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}-{}", prefix, std::process::id(), unique))
    }

    fn write_skill(root: &Path, name: &str, triggers: &[&str], instructions: &str) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join(SKILL_MANIFEST_FILE),
            json!({
                "name": name,
                "description": format!("Guidance for {name}"),
                "trigger_guidance": triggers,
                "instructions": instructions
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn loads_skills_from_configured_directories() {
        let project = skill_test_path("load");
        let skill_root = project.join("skills").join("rust-tutor");
        write_skill(
            &skill_root,
            "rust-tutor",
            &["ownership", "borrowing"],
            "Explain Rust ownership in small steps.",
        );
        let settings = json!({
            "skills": {
                "directories": ["skills"]
            }
        });

        let catalog = SkillCatalog::from_settings(&settings, &project, Vec::new()).unwrap();

        assert_eq!(catalog.metadata().len(), 1);
        assert_eq!(catalog.metadata()[0].name, "rust-tutor");

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn activates_skills_by_name_or_trigger_phrase() {
        let project = skill_test_path("activate");
        write_skill(
            &project.join("skills").join("rust-tutor"),
            "rust-tutor",
            &["ownership"],
            "Explain ownership.",
        );
        write_skill(
            &project.join("skills").join("release"),
            "release",
            &["publish a release"],
            "Check versioning.",
        );
        let settings = json!({
            "skills": {
                "directories": ["skills"]
            }
        });
        let catalog = SkillCatalog::from_settings(&settings, &project, Vec::new()).unwrap();

        let named = catalog
            .activate_for_prompt("Use $rust-tutor for this change")
            .unwrap();
        let triggered = catalog
            .activate_for_prompt("Can you explain ownership here?")
            .unwrap();

        assert_eq!(
            named
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["rust-tutor"]
        );
        assert_eq!(
            triggered
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["rust-tutor"]
        );

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn catalog_loads_metadata_without_validating_inactive_instructions() {
        let project = skill_test_path("lazy-invalid");
        let skill_root = project.join("skills").join("rust-tutor");
        std::fs::create_dir_all(&skill_root).unwrap();
        std::fs::write(
            skill_root.join(SKILL_MANIFEST_FILE),
            r#"{
                "name": "rust-tutor",
                "description": "Guidance for Rust learning",
                "trigger_guidance": ["ownership"],
                "instructions": 42
            }"#,
        )
        .unwrap();
        let settings = json!({
            "skills": {
                "directories": ["skills"]
            }
        });

        let catalog = SkillCatalog::from_settings(&settings, &project, Vec::new()).unwrap();

        assert_eq!(catalog.metadata()[0].name, "rust-tutor");
        assert!(
            catalog
                .activate_for_prompt("no matching task")
                .unwrap()
                .is_empty()
        );
        let error = catalog.activate_for_prompt("ownership help").unwrap_err();
        assert!(error.to_string().contains("instructions"));

        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn activation_reads_instruction_body_from_manifest_after_catalog_load() {
        let project = skill_test_path("lazy-reload");
        let skill_root = project.join("skills").join("rust-tutor");
        write_skill(
            &skill_root,
            "rust-tutor",
            &["ownership"],
            "old instructions",
        );
        let settings = json!({
            "skills": {
                "directories": ["skills"]
            }
        });
        let catalog = SkillCatalog::from_settings(&settings, &project, Vec::new()).unwrap();
        write_skill(
            &skill_root,
            "rust-tutor",
            &["ownership"],
            "new instructions",
        );

        let active = catalog.activate_for_prompt("ownership help").unwrap();

        assert_eq!(active[0].instructions, "new instructions");

        std::fs::remove_dir_all(project).unwrap();
    }
}
