use crate::docs;
use crate::mcp;
use crate::workspace::{
    self, AuditTextResponse, BuildResponse, GlossaryIgnoreResponse, IngestResponse,
    InspectMediaResponse, InspectSectionDetailResponse, InspectSectionsResponse,
    PatchStructureResponse, RulesAddResponse, UpdateMediaResponse, VerifyEpubResponse,
};
use anyhow::Result;
use clap::{Parser, Subcommand};
use editnovel_engine::epub_builder::EpubBuilder;
use editnovel_engine::models::VolumeInfo;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "editnovel",
    version,
    about = "Local-first CLI and MCP Server for converting PDFs and EPUBs into clean EPUB 3 and omnibus volumes."
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Ingest {
        #[arg(required = true)]
        files: Vec<PathBuf>,

        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long, default_value = "en-light-novel")]
        profile: String,

        #[arg(long, default_value = "auto")]
        lang: String,
    },
    Inspect {
        #[command(subcommand)]
        target: InspectCommands,
    },
    Structure {
        #[command(subcommand)]
        action: StructureCommands,
    },
    Media {
        #[command(subcommand)]
        action: MediaCommands,
    },
    Audit {
        #[command(subcommand)]
        target: AuditCommands,
    },
    Glossary {
        #[command(subcommand)]
        action: GlossaryCommands,
    },
    Rules {
        #[command(subcommand)]
        action: RulesCommands,
    },
    Build {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long)]
        out: PathBuf,

        #[arg(long)]
        export_project: Option<PathBuf>,
    },
    Verify {
        epub_path: PathBuf,
    },
    Mcp,
    Docs {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    AuditBench {
        #[arg(long, short = 'd')]
        dir: PathBuf,

        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum InspectCommands {
    Sections {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,
    },
    Section {
        id: String,

        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long, default_value_t = 10)]
        head: usize,

        #[arg(long, default_value_t = 10)]
        tail: usize,
    },
    Media {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long, default_value = "all")]
        filter: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum StructureCommands {
    Patch {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long)]
        merge: Vec<String>,

        #[arg(long)]
        rename: Vec<String>,

        #[arg(long)]
        retype: Vec<String>,

        #[arg(long)]
        reorder: Option<String>,

        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MediaCommands {
    Update {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long, num_args = 1..)]
        keep: Vec<String>,

        #[arg(long, num_args = 1..)]
        discard: Vec<String>,

        #[arg(long, num_args = 1..)]
        alt: Vec<String>,

        #[arg(long, num_args = 1..)]
        decorative: Vec<String>,

        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuditCommands {
    Text {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long, default_value_t = 0.75)]
        min_conf: f64,

        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum GlossaryCommands {
    Ignore {
        #[arg(required = true)]
        words: Vec<String>,

        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long)]
        apply_spell: bool,

        #[arg(long, default_value_t = 0.90)]
        min_conf: f64,
    },
}

#[derive(Subcommand, Debug)]
pub enum RulesCommands {
    Add {
        #[arg(long, default_value = "./work")]
        workspace: PathBuf,

        #[arg(long, allow_hyphen_values = true)]
        find: String,

        #[arg(long, allow_hyphen_values = true)]
        replace: String,

        #[arg(long, default_value = "g")]
        flags: String,

        #[arg(long)]
        dry_run: bool,
    },
}

fn emit_output<T: Serialize>(value: &T, _json_flag: bool) -> Result<()> {
    let out = serde_json::to_string_pretty(value)?;
    println!("{}", out);
    Ok(())
}

pub async fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let json_flag = cli.json;

    match cli.command {
        Commands::Ingest {
            files,
            workspace,
            profile,
            lang,
        } => {
            let res: IngestResponse =
                workspace::ingest_sources(&files, &workspace, &profile, &lang)?;
            emit_output(&res, json_flag)?;
        }
        Commands::Inspect { target } => match target {
            InspectCommands::Sections { workspace } => {
                let res: InspectSectionsResponse = workspace::inspect_sections(&workspace)?;
                emit_output(&res, json_flag)?;
            }
            InspectCommands::Section {
                id,
                workspace,
                head,
                tail,
            } => {
                let res: InspectSectionDetailResponse =
                    workspace::inspect_section(&workspace, &id, head, tail)?;
                emit_output(&res, json_flag)?;
            }
            InspectCommands::Media { workspace, filter } => {
                let res: InspectMediaResponse = workspace::inspect_media(&workspace, &filter)?;
                emit_output(&res, json_flag)?;
            }
        },
        Commands::Structure { action } => match action {
            StructureCommands::Patch {
                workspace,
                merge,
                rename,
                retype,
                reorder,
                dry_run,
            } => {
                let res: PatchStructureResponse = workspace::patch_structure(
                    &workspace,
                    &merge,
                    &rename,
                    &retype,
                    reorder.as_deref(),
                    dry_run,
                )?;
                emit_output(&res, json_flag)?;
            }
        },
        Commands::Media { action } => match action {
            MediaCommands::Update {
                workspace,
                keep,
                discard,
                alt,
                decorative,
                dry_run,
            } => {
                let res: UpdateMediaResponse = workspace::update_media(
                    &workspace,
                    &keep,
                    &discard,
                    &alt,
                    &decorative,
                    dry_run,
                )?;
                emit_output(&res, json_flag)?;
            }
        },
        Commands::Audit { target } => match target {
            AuditCommands::Text {
                workspace,
                min_conf,
                limit,
            } => {
                let res: AuditTextResponse = workspace::audit_text(&workspace, min_conf, limit)?;
                emit_output(&res, json_flag)?;
            }
        },
        Commands::Glossary { action } => match action {
            GlossaryCommands::Ignore {
                words,
                workspace,
                apply_spell,
                min_conf,
            } => {
                let res: GlossaryIgnoreResponse =
                    workspace::glossary_ignore(&workspace, &words, apply_spell, min_conf)?;
                emit_output(&res, json_flag)?;
            }
        },
        Commands::Rules { action } => match action {
            RulesCommands::Add {
                workspace,
                find,
                replace,
                flags,
                dry_run,
            } => {
                let res: RulesAddResponse =
                    workspace::rules_add(&workspace, &find, &replace, &flags, dry_run)?;
                emit_output(&res, json_flag)?;
            }
        },
        Commands::Build {
            workspace,
            out,
            export_project,
        } => {
            let res: BuildResponse =
                workspace::build_workspace(&workspace, &out, export_project.as_deref())?;
            emit_output(&res, json_flag)?;
            if !res.verification.valid {
                anyhow::bail!("Built EPUB failed verification checks");
            }
        }
        Commands::Verify { epub_path } => {
            let res: VerifyEpubResponse = workspace::verify_epub(&epub_path)?;
            emit_output(&res, json_flag)?;
            if !res.valid {
                anyhow::bail!("EPUB verification failed");
            }
        }
        Commands::Mcp => {
            mcp::run_mcp_server().await?;
        }
        Commands::Docs { out } => {
            let manifest = docs::write_manifest_if_requested(out.as_deref())?;
            emit_output(&manifest, json_flag)?;
        }
        Commands::AuditBench { dir, all } => {
            run_audit_bench(&dir, all, json_flag)?;
        }
    }

    Ok(())
}

fn run_audit_bench(pdf_dir: &PathBuf, bench_all: bool, json_flag: bool) -> Result<()> {
    let mut pdf_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(pdf_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
            {
                pdf_files.push(path);
            }
        }
    }
    pdf_files.sort();
    if !bench_all && !pdf_files.is_empty() {
        pdf_files.truncate(1);
    }

    let t0 = std::time::Instant::now();
    let mut total_pages = 0usize;
    let mut volumes: Vec<VolumeInfo> = Vec::with_capacity(pdf_files.len());
    for pdf_path in &pdf_files {
        let fname = pdf_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let bytes = fs::read(pdf_path)?;
        let mut builder = EpubBuilder::new();
        let vol = builder
            .process_pdf_bytes(&bytes, fname, true)
            .map_err(|e| anyhow::anyhow!(e))?;
        total_pages += vol.total_pages;
        volumes.push(vol);
    }
    let ingest_ms = t0.elapsed().as_millis();

    let t1 = std::time::Instant::now();
    let builder = EpubBuilder::new();
    let mut total_epub_bytes = 0usize;
    for vol in &volumes {
        let epub_bytes = builder
            .build_epub_bytes(vol)
            .map_err(|e| anyhow::anyhow!(e))?;
        total_epub_bytes += epub_bytes.len();
    }
    let pack_ms = t1.elapsed().as_millis();

    let report = serde_json::json!({
        "volumes": pdf_files.len(),
        "pages": total_pages,
        "ingest_ms": ingest_ms,
        "pack_ms": pack_ms,
        "total_ms": ingest_ms + pack_ms,
        "epub_bytes": total_epub_bytes,
    });
    emit_output(&report, json_flag)?;
    Ok(())
}
