use crate::workspace;
use anyhow::{Result, anyhow, bail};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, object,
    },
    service::RequestContext,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Default)]
pub struct EditNovelMcpServer;

#[derive(Debug, Deserialize)]
struct IngestArgs {
    files: Vec<String>,
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default = "default_lang")]
    lang: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceOnlyArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
}

#[derive(Debug, Deserialize)]
struct InspectSectionArgs {
    id: String,
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default = "default_head_tail")]
    head: usize,
    #[serde(default = "default_head_tail")]
    tail: usize,
}

#[derive(Debug, Deserialize)]
struct PatchStructureArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default)]
    merge: Vec<String>,
    #[serde(default)]
    rename: Vec<String>,
    #[serde(default)]
    retype: Vec<String>,
    #[serde(default)]
    reorder: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct InspectMediaArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default = "default_filter")]
    filter: String,
}

#[derive(Debug, Deserialize)]
struct UpdateMediaArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default)]
    keep: Vec<String>,
    #[serde(default)]
    discard: Vec<String>,
    #[serde(default)]
    alt: Vec<String>,
    #[serde(default)]
    decorative: Vec<String>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct AuditTextArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default = "default_audit_conf")]
    min_conf: f64,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct ApplyRulesArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
    #[serde(default)]
    ignore_words: Vec<String>,
    #[serde(default)]
    apply_spell: bool,
    #[serde(default = "default_spell_conf")]
    min_conf: f64,
    #[serde(default)]
    find: Option<String>,
    #[serde(default)]
    replace: Option<String>,
    #[serde(default = "default_flags")]
    flags: String,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct BuildEpubArgs {
    #[serde(default = "default_workspace")]
    workspace: String,
    out: String,
    #[serde(default)]
    export_project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerifyEpubArgs {
    epub_path: String,
}

fn default_workspace() -> String {
    "./work".to_string()
}

fn default_profile() -> String {
    "en-light-novel".to_string()
}

fn default_lang() -> String {
    "auto".to_string()
}

fn default_head_tail() -> usize {
    10
}

fn default_filter() -> String {
    "all".to_string()
}

fn default_audit_conf() -> f64 {
    0.75
}

fn default_spell_conf() -> f64 {
    0.90
}

fn default_limit() -> usize {
    50
}

fn default_flags() -> String {
    "g".to_string()
}

pub fn mcp_tool_definitions() -> Vec<Tool> {
    vec![
        Tool::new(
            "editnovel_ingest",
            "Ingest PDF, EPUB, or .editnovel files into a local v2 workspace directory and return compact diagnostics.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Input file paths (.pdf, .epub, or .editnovel)"
                    },
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "profile": {
                        "type": "string",
                        "enum": ["en-light-novel", "en-litrpg", "en-fiction", "es-editorial"],
                        "default": "en-light-novel"
                    },
                    "lang": {
                        "type": "string",
                        "enum": ["auto", "es", "en"],
                        "default": "auto"
                    }
                },
                "required": ["files"]
            }))),
        ),
        Tool::new(
            "editnovel_inspect_sections",
            "List detected sections and chapters in the workspace with lightweight metadata and untitled flags.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    }
                }
            }))),
        ),
        Tool::new(
            "editnovel_inspect_section",
            "Inspect head and tail paragraph previews, footnotes, and images of a specific section by ID.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Section identifier (e.g. s01)"
                    },
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "head": {
                        "type": "integer",
                        "default": 10
                    },
                    "tail": {
                        "type": "integer",
                        "default": 10
                    }
                },
                "required": ["id"]
            }))),
        ),
        Tool::new(
            "editnovel_patch_structure",
            "Merge, rename, retype, or reorder sections in the workspace (supports dry_run).",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "merge": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Comma-separated section ID pairs to merge (e.g. [\"s03,s04\"])"
                    },
                    "rename": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Section renames in id=title format (e.g. [\"s02=Chapter 1\"])"
                    },
                    "retype": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Section type overrides in id=type format"
                    },
                    "reorder": {
                        "type": "string",
                        "description": "Comma-separated section ID order"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "default": false
                    }
                }
            }))),
        ),
        Tool::new(
            "editnovel_inspect_media",
            "Inspect extracted images and dHash filtering verdicts with local thumbnail paths.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "filter": {
                        "type": "string",
                        "default": "all",
                        "description": "Filter by status: all, kept, discarded, uncertain, or comma-separated"
                    }
                }
            }))),
        ),
        Tool::new(
            "editnovel_update_media",
            "Keep or discard images, attach accessibility alt-text, or mark images as decorative.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "keep": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "discard": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "alt": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Alt-text assignments in id=text format"
                    },
                    "decorative": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "dry_run": {
                        "type": "boolean",
                        "default": false
                    }
                }
            }))),
        ),
        Tool::new(
            "editnovel_audit_text",
            "Return deduplicated contextual spelling and glossary clusters above min_conf.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "min_conf": {
                        "type": "number",
                        "default": 0.75
                    },
                    "limit": {
                        "type": "integer",
                        "default": 50
                    }
                }
            }))),
        ),
        Tool::new(
            "editnovel_apply_rules",
            "Ignore proper nouns/glossary terms, apply high-confidence spell fixes, or add regex replacement rules.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "ignore_words": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "apply_spell": {
                        "type": "boolean",
                        "default": false
                    },
                    "min_conf": {
                        "type": "number",
                        "default": 0.90
                    },
                    "find": {
                        "type": "string"
                    },
                    "replace": {
                        "type": "string"
                    },
                    "flags": {
                        "type": "string",
                        "default": "g"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "default": false
                    }
                }
            }))),
        ),
        Tool::new(
            "editnovel_build_epub",
            "Build and validate the final EPUB 3 (or multi-volume omnibus) and optionally export a .editnovel v2 archive.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "default": "./work"
                    },
                    "out": {
                        "type": "string",
                        "description": "Output EPUB 3 file path"
                    },
                    "export_project": {
                        "type": "string",
                        "description": "Optional output .editnovel project archive path"
                    }
                },
                "required": ["out"]
            }))),
        ),
        Tool::new(
            "editnovel_verify_epub",
            "Verify an EPUB 3 file for OCF mimetype rules, XML/XHTML well-formedness, image references, and EPUB 3 metadata.",
            Arc::new(object(json!({
                "type": "object",
                "properties": {
                    "epub_path": {
                        "type": "string"
                    }
                },
                "required": ["epub_path"]
            }))),
        ),
    ]
}

pub fn dispatch_mcp_tool(name: &str, args: Value) -> Result<Value> {
    match name {
        "editnovel_ingest" => {
            let parsed: IngestArgs = serde_json::from_value(args)?;
            let files: Vec<PathBuf> = parsed.files.into_iter().map(PathBuf::from).collect();
            let res = workspace::ingest_sources(
                &files,
                &PathBuf::from(parsed.workspace),
                &parsed.profile,
                &parsed.lang,
            )?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_inspect_sections" => {
            let parsed: WorkspaceOnlyArgs = serde_json::from_value(args)?;
            let res = workspace::inspect_sections(&PathBuf::from(parsed.workspace))?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_inspect_section" => {
            let parsed: InspectSectionArgs = serde_json::from_value(args)?;
            let res = workspace::inspect_section(
                &PathBuf::from(parsed.workspace),
                &parsed.id,
                parsed.head,
                parsed.tail,
            )?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_patch_structure" => {
            let parsed: PatchStructureArgs = serde_json::from_value(args)?;
            let res = workspace::patch_structure(
                &PathBuf::from(parsed.workspace),
                &parsed.merge,
                &parsed.rename,
                &parsed.retype,
                parsed.reorder.as_deref(),
                parsed.dry_run,
            )?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_inspect_media" => {
            let parsed: InspectMediaArgs = serde_json::from_value(args)?;
            let res = workspace::inspect_media(&PathBuf::from(parsed.workspace), &parsed.filter)?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_update_media" => {
            let parsed: UpdateMediaArgs = serde_json::from_value(args)?;
            let res = workspace::update_media(
                &PathBuf::from(parsed.workspace),
                &parsed.keep,
                &parsed.discard,
                &parsed.alt,
                &parsed.decorative,
                parsed.dry_run,
            )?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_audit_text" => {
            let parsed: AuditTextArgs = serde_json::from_value(args)?;
            let res = workspace::audit_text(
                &PathBuf::from(parsed.workspace),
                parsed.min_conf,
                parsed.limit,
            )?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_apply_rules" => {
            let parsed: ApplyRulesArgs = serde_json::from_value(args)?;
            let ws_path = PathBuf::from(&parsed.workspace);
            let mut combined = serde_json::Map::new();
            combined.insert("status".to_string(), json!("ok"));

            if !parsed.ignore_words.is_empty() || parsed.apply_spell {
                let gl_res = workspace::glossary_ignore(
                    &ws_path,
                    &parsed.ignore_words,
                    parsed.apply_spell,
                    parsed.min_conf,
                )?;
                combined.insert("glossary".to_string(), serde_json::to_value(gl_res)?);
            }
            if let Some(ref find_pat) = parsed.find {
                let rep = parsed.replace.as_deref().unwrap_or("");
                let rl_res =
                    workspace::rules_add(&ws_path, find_pat, rep, &parsed.flags, parsed.dry_run)?;
                combined.insert("regex_rule".to_string(), serde_json::to_value(rl_res)?);
            }
            Ok(Value::Object(combined))
        }
        "editnovel_build_epub" => {
            let parsed: BuildEpubArgs = serde_json::from_value(args)?;
            let export_proj = parsed.export_project.map(PathBuf::from);
            let res = workspace::build_workspace(
                &PathBuf::from(parsed.workspace),
                &PathBuf::from(parsed.out),
                export_proj.as_deref(),
            )?;
            Ok(serde_json::to_value(res)?)
        }
        "editnovel_verify_epub" => {
            let parsed: VerifyEpubArgs = serde_json::from_value(args)?;
            let res = workspace::verify_epub(&PathBuf::from(parsed.epub_path))?;
            Ok(serde_json::to_value(res)?)
        }
        other => bail!("Unknown MCP tool '{}'", other),
    }
}

impl ServerHandler for EditNovelMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("editnovel", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Use EditNovel's 5-stage tool loop (ingest -> inspect/patch structure -> inspect/update media -> audit/apply rules -> build/verify EPUB 3).",
            )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(mcp_tool_definitions())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        mcp_tool_definitions().into_iter().find(|t| t.name == name)
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResponse, ErrorData>> + Send + '_ {
        let name = request.name.to_string();
        let args_val = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));

        async move {
            let known = mcp_tool_definitions().iter().any(|t| t.name == name);
            if !known {
                return Err(ErrorData::invalid_params(
                    format!("Unknown tool '{}'", name),
                    None,
                ));
            }
            match dispatch_mcp_tool(&name, args_val) {
                Ok(val) => Ok(CallToolResult::structured(val).into()),
                Err(e) => Ok(CallToolResult::structured_error(json!({
                    "status": "error",
                    "error": e.to_string(),
                }))
                .into()),
            }
        }
    }
}

pub async fn run_mcp_server() -> Result<()> {
    let service = EditNovelMcpServer
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow!("Failed to start MCP stdio server: {}", e))?;
    service
        .waiting()
        .await
        .map_err(|e| anyhow!("MCP server task error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_catalog_completeness() {
        let tools = mcp_tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(
            names,
            vec![
                "editnovel_ingest",
                "editnovel_inspect_sections",
                "editnovel_inspect_section",
                "editnovel_patch_structure",
                "editnovel_inspect_media",
                "editnovel_update_media",
                "editnovel_audit_text",
                "editnovel_apply_rules",
                "editnovel_build_epub",
                "editnovel_verify_epub",
            ]
        );
    }
}
