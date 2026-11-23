use crate::cli::Cli;
use crate::mcp;
use crate::workspace::SUPPORTED_PROFILES;
use anyhow::Result;
use clap::{ArgAction, CommandFactory};
use editnovel_engine::contracts::{ARCHIVE_SCHEMA_VERSION, SUPPORTED_LANGUAGE_CODES};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

const SKILL_MD_RAW: &str = include_str!("../SKILL.md");
const SERVER_JSON_RAW: &str = include_str!("../server.json");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliArgDoc {
    pub id: String,
    pub long: Option<String>,
    pub short: Option<char>,
    pub required: bool,
    pub takes_value: bool,
    pub default_values: Vec<String>,
    pub help: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandDoc {
    pub path: Vec<String>,
    pub name: String,
    pub about: Option<String>,
    pub args: Vec<CliArgDoc>,
    pub subcommands: Vec<CliCommandDoc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDoc {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliManifest {
    pub schema_version: u32,
    pub name: String,
    pub version: String,
    pub engine_version: String,
    pub archive_schema_version: u32,
    pub description: String,
    pub repository: String,
    pub skills_install_command: String,
    pub mcp_server_name: String,
    pub mcp_config: Value,
    pub supported_profiles: Vec<String>,
    pub supported_langs: Vec<String>,
    pub commands: Vec<CliCommandDoc>,
    pub mcp_tools: Vec<McpToolDoc>,
    pub skill_markdown: String,
    pub server_json: Value,
}

fn strip_frontmatter(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n");
    if let Some(rest) = normalized.strip_prefix("---\n")
        && let Some(end_idx) = rest.find("\n---\n")
    {
        return rest[end_idx + 5..].trim().to_string();
    }
    normalized.trim().to_string()
}

fn collect_command_doc(cmd: &clap::Command, prefix: &[String]) -> CliCommandDoc {
    let name = cmd.get_name().to_string();
    let mut path = prefix.to_vec();
    if !prefix.is_empty() || name != "editnovel" {
        path.push(name.clone());
    }

    let args = cmd
        .get_arguments()
        .filter(|arg| {
            let id = arg.get_id().as_str();
            id != "help" && id != "version" && !arg.is_global_set()
        })
        .map(|arg| CliArgDoc {
            id: arg.get_id().to_string(),
            long: arg.get_long().map(str::to_string),
            short: arg.get_short(),
            required: arg.is_required_set(),
            takes_value: !matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count),
            default_values: arg
                .get_default_values()
                .iter()
                .map(|val| val.to_string_lossy().to_string())
                .collect(),
            help: arg.get_help().map(ToString::to_string),
        })
        .collect();

    let subcommands = cmd
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .map(|sub| collect_command_doc(sub, &path))
        .collect();

    CliCommandDoc {
        path,
        name,
        about: cmd.get_about().map(ToString::to_string),
        args,
        subcommands,
    }
}

pub fn build_cli_manifest() -> Result<CliManifest> {
    let server_json: Value = serde_json::from_str(SERVER_JSON_RAW)?;
    let root_cmd = Cli::command();

    let commands = root_cmd
        .get_subcommands()
        .filter(|sub| {
            !sub.is_hide_set()
                && sub.get_name() != "help"
                && sub.get_name() != "audit-bench"
                && sub.get_name() != "docs"
        })
        .map(|sub| collect_command_doc(sub, &[]))
        .collect();

    let mcp_tools = mcp::mcp_tool_definitions()
        .into_iter()
        .map(|tool| McpToolDoc {
            name: tool.name.to_string(),
            description: tool.description.map(|d| d.to_string()).unwrap_or_default(),
            input_schema: Value::Object((*tool.input_schema).clone()),
        })
        .collect();

    let repository = server_json
        .pointer("/repository/url")
        .and_then(Value::as_str)
        .unwrap_or("https://github.com/editnovel/editnovel")
        .to_string();

    let npm_pkg = server_json
        .pointer("/packages/0/identifier")
        .and_then(Value::as_str)
        .unwrap_or("editnovel");

    Ok(CliManifest {
        schema_version: 1,
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        engine_version: editnovel_engine::engine_version().to_string(),
        archive_schema_version: ARCHIVE_SCHEMA_VERSION,
        description: env!("CARGO_PKG_DESCRIPTION").to_string(),
        repository,
        skills_install_command: "npx skills add editnovel/editnovel".to_string(),
        mcp_server_name: npm_pkg.to_string(),
        mcp_config: serde_json::json!({
            "mcpServers": {
                npm_pkg: {
                    "command": "npx",
                    "args": ["-y", npm_pkg, "mcp"]
                }
            }
        }),
        supported_profiles: SUPPORTED_PROFILES.iter().map(|s| (*s).to_string()).collect(),
        supported_langs: SUPPORTED_LANGUAGE_CODES.iter().map(|s| (*s).to_string()).collect(),
        commands,
        mcp_tools,
        skill_markdown: strip_frontmatter(SKILL_MD_RAW),
        server_json,
    })
}

pub fn write_manifest_if_requested(out_path: Option<&Path>) -> Result<CliManifest> {
    let manifest = build_cli_manifest()?;
    if let Some(path) = out_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(&manifest)?;
        fs::write(path, format!("{}\n", body))?;
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_manifest_matches_clap_and_mcp() {
        let manifest = build_cli_manifest().expect("manifest should build");
        assert!(!manifest.version.is_empty());
        assert_eq!(manifest.supported_profiles, SUPPORTED_PROFILES);
        assert_eq!(manifest.supported_langs, SUPPORTED_LANGUAGE_CODES);

        let cmd_names: Vec<&str> = manifest.commands.iter().map(|c| c.name.as_str()).collect();
        for expected in ["ingest", "inspect", "structure", "media", "audit", "glossary", "rules", "build", "verify", "mcp"] {
            assert!(cmd_names.contains(&expected), "missing command {}", expected);
        }

        assert_eq!(manifest.mcp_tools.len(), mcp::mcp_tool_definitions().len());
        assert!(!manifest.skill_markdown.starts_with("---"));
    }
}
