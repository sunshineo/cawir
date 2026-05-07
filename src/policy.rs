use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PermissionMode {
    Default,
    Plan,
    AcceptEdits,
    Bypass,
}

impl PermissionMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Plan => "plan",
            Self::AcceptEdits => "accept-edits",
            Self::Bypass => "bypass",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::Default),
            "plan" => Some(Self::Plan),
            "accept-edits" => Some(Self::AcceptEdits),
            "bypass" => Some(Self::Bypass),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
    Allow,
    AskUser,
    Deny(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolKind {
    ReadOnly,
    WriteFile,
    Shell,
    ExitPlanMode,
}

pub(crate) fn permission_decision(mode: PermissionMode, tool: ToolKind) -> PermissionDecision {
    match mode {
        PermissionMode::Default => match tool {
            ToolKind::ReadOnly | ToolKind::ExitPlanMode => PermissionDecision::Allow,
            ToolKind::WriteFile | ToolKind::Shell => PermissionDecision::AskUser,
        },
        PermissionMode::Plan => match tool {
            ToolKind::ReadOnly | ToolKind::ExitPlanMode => PermissionDecision::Allow,
            ToolKind::WriteFile | ToolKind::Shell => {
                PermissionDecision::Deny("plan mode does not allow mutating tools")
            }
        },
        PermissionMode::AcceptEdits => match tool {
            ToolKind::ReadOnly | ToolKind::WriteFile | ToolKind::ExitPlanMode => {
                PermissionDecision::Allow
            }
            ToolKind::Shell => PermissionDecision::AskUser,
        },
        PermissionMode::Bypass => PermissionDecision::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_names_parse_back_to_modes() {
        for mode in [
            PermissionMode::Default,
            PermissionMode::Plan,
            PermissionMode::AcceptEdits,
            PermissionMode::Bypass,
        ] {
            assert_eq!(PermissionMode::parse(mode.name()), Some(mode));
        }
    }

    #[test]
    fn plan_mode_denies_mutating_tools() {
        assert_eq!(
            permission_decision(PermissionMode::Plan, ToolKind::WriteFile),
            PermissionDecision::Deny("plan mode does not allow mutating tools")
        );
        assert_eq!(
            permission_decision(PermissionMode::Plan, ToolKind::Shell),
            PermissionDecision::Deny("plan mode does not allow mutating tools")
        );
    }

    #[test]
    fn accept_edits_allows_writes_but_still_asks_for_shell() {
        assert_eq!(
            permission_decision(PermissionMode::AcceptEdits, ToolKind::WriteFile),
            PermissionDecision::Allow
        );
        assert_eq!(
            permission_decision(PermissionMode::AcceptEdits, ToolKind::Shell),
            PermissionDecision::AskUser
        );
    }
}
