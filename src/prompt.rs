use std::{collections::BTreeSet, fs, path::Path};

use crate::{Result, skills::Skill};

const IDENTITY: &str = "You are cawir, a minimal coding agent written in Rust.";
const BEHAVIOR: &str =
    "Answer plainly, use available tools when needed, and keep the implementation readable.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SystemPrompt {
    pub(crate) sections: Vec<PromptSection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PromptSection {
    pub(crate) name: String,
    pub(crate) content: String,
}

impl SystemPrompt {
    pub(crate) fn render_text(&self) -> String {
        self.sections
            .iter()
            .map(|section| {
                format!(
                    "<{}>\n{}\n</{}>",
                    section.name, section.content, section.name
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
pub(crate) fn assemble(project_path: &Path) -> Result<SystemPrompt> {
    assemble_with_skills(project_path, &[])
}

pub(crate) fn assemble_with_skills(
    project_path: &Path,
    active_skills: &[Skill],
) -> Result<SystemPrompt> {
    let project_path = project_path.canonicalize()?;
    let mut sections = vec![
        section("identity", IDENTITY),
        section("behavior", BEHAVIOR),
        section("environment", format!("cwd: {}", project_path.display())),
    ];

    let guidance = load_project_guidance(&project_path)?;
    if !guidance.is_empty() {
        sections.push(section("project_guidance", guidance.join("\n\n")));
    }

    if !active_skills.is_empty() {
        sections.push(section(
            "active_skills",
            render_active_skills(active_skills),
        ));
    }

    Ok(SystemPrompt { sections })
}

fn section(name: &str, content: impl Into<String>) -> PromptSection {
    PromptSection {
        name: name.to_string(),
        content: content.into(),
    }
}

fn load_project_guidance(project_path: &Path) -> Result<Vec<String>> {
    let mut directories = project_path
        .ancestors()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    directories.reverse();

    let mut seen_files = BTreeSet::new();
    let mut guidance = Vec::new();

    for directory in directories {
        for file_name in ["AGENTS.md", "CLAUDE.md"] {
            let path = directory.join(file_name);
            if !path.is_file() {
                continue;
            }

            let canonical_path = path.canonicalize()?;
            if !seen_files.insert(canonical_path) {
                continue;
            }

            guidance.push(format!(
                "## {}\n{}",
                display_guidance_path(project_path, &path),
                fs::read_to_string(path)?
            ));
        }
    }

    Ok(guidance)
}

fn render_active_skills(active_skills: &[Skill]) -> String {
    active_skills
        .iter()
        .map(|skill| {
            let mut parts = vec![
                format!("## {}", skill.name),
                format!("Description: {}", skill.description),
            ];
            if !skill.trigger_guidance.is_empty() {
                parts.push(format!(
                    "Trigger guidance:\n{}",
                    skill
                        .trigger_guidance
                        .iter()
                        .map(|trigger| format!("- {trigger}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            parts.push(format!("Instructions:\n{}", skill.instructions));
            parts.join("\n\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn display_guidance_path(project_path: &Path, path: &Path) -> String {
    path.strip_prefix(project_path)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn assembles_named_sections_with_project_guidance() {
        let workspace = TestWorkspace::new("prompt_sections");
        workspace.write("CLAUDE.md", "project rule");

        let prompt = assemble(&workspace.root).unwrap();

        assert_eq!(
            section_names(&prompt),
            vec!["identity", "behavior", "environment", "project_guidance"]
        );
        assert!(prompt.render_text().contains("<identity>\n"));
        assert!(prompt.render_text().contains("project rule"));
    }

    #[test]
    fn loads_project_guidance_from_ancestors_before_children() {
        let workspace = TestWorkspace::new("prompt_hierarchy");
        workspace.write("CLAUDE.md", "outer rule");
        workspace.create_dir("crate/src");
        workspace.write("crate/AGENTS.md", "inner rule");

        let prompt = assemble(&workspace.root.join("crate/src")).unwrap();
        let rendered = prompt.render_text();

        assert!(rendered.contains("CLAUDE.md"));
        assert!(rendered.contains("AGENTS.md"));
        assert!(rendered.find("outer rule").unwrap() < rendered.find("inner rule").unwrap());
    }

    #[test]
    fn skips_duplicate_guidance_files_that_resolve_to_same_path() {
        let workspace = TestWorkspace::new("prompt_duplicates");
        workspace.write("CLAUDE.md", "same rule");
        #[cfg(unix)]
        std::os::unix::fs::symlink("CLAUDE.md", workspace.root.join("AGENTS.md")).unwrap();
        #[cfg(not(unix))]
        workspace.write("AGENTS.md", "same rule");

        let prompt = assemble(&workspace.root).unwrap();
        let rendered = prompt.render_text();

        assert_eq!(rendered.matches("same rule").count(), 1);
    }

    #[test]
    fn active_skills_render_as_prompt_section() {
        let workspace = TestWorkspace::new("prompt_active_skills");
        let skills = vec![crate::skills::Skill {
            name: "rust-tutor".to_string(),
            description: "Teach Rust concepts while coding".to_string(),
            trigger_guidance: vec!["ownership".to_string()],
            instructions: "Explain ownership and borrowing when they appear.".to_string(),
        }];

        let prompt = assemble_with_skills(&workspace.root, &skills).unwrap();
        let rendered = prompt.render_text();

        assert_eq!(
            section_names(&prompt),
            vec!["identity", "behavior", "environment", "active_skills"]
        );
        assert!(rendered.contains("## rust-tutor"));
        assert!(rendered.contains("Teach Rust concepts while coding"));
        assert!(rendered.contains("Explain ownership and borrowing when they appear."));
    }

    fn section_names(prompt: &SystemPrompt) -> Vec<&str> {
        prompt
            .sections
            .iter()
            .map(|section| section.name.as_str())
            .collect()
    }

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("cawir_{name}_{unique}"));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn create_dir(&self, path: &str) {
            fs::create_dir_all(self.root.join(path)).unwrap();
        }

        fn write(&self, path: &str, contents: &str) {
            if let Some(parent) = Path::new(path).parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(self.root.join(parent)).unwrap();
            }
            fs::write(self.root.join(path), contents).unwrap();
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
