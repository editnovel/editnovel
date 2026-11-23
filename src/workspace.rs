use anyhow::{Context, Result, anyhow, bail};
use editnovel_engine::cleaner::TextCleaner;
use editnovel_engine::config::infer_series_and_volume;
use editnovel_engine::epub_builder::{EpubBuilder, detect_synopsis_from_sections, html_escape, sanitize_html_for_xhtml};
use editnovel_engine::models::{ContentItem, ContentItemPayload, ExtractedImage, Footnote, Paragraph, Section, VolumeInfo};
use editnovel_engine::pdf_extractor::ParsedPdfDocument;
use editnovel_engine::spell_corrector::{ContextualSpellCorrector, SpellAuditOptions, SpellAuditStats};
use editnovel_engine::templates::classify_section_type;
use editnovel_engine::{FrontendBookMetadata, FrontendEditorialRule, FrontendGlossaryReplacement, FrontendRegexRule, apply_transforms_internal};
use quick_xml::Reader;
use quick_xml::events::Event;
use rc_zip::fsm::{ArchiveFsm, EntryFsm, FsmResult};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDiagnostics {
    pub has_text_layer: bool,
    pub total_sources: usize,
    pub total_pages: usize,
    pub total_sections: usize,
    pub total_paragraphs: usize,
    pub chapters_without_title: usize,
    pub untitled_section_ids: Vec<String>,
    pub kept_images: usize,
    pub discarded_images: usize,
    pub uncertain_images: usize,
    pub spell_clusters: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileEntry {
    pub file_id: String,
    pub file_name: String,
    pub volume_number: usize,
    pub total_pages: usize,
    pub has_text_layer: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub version: String,
    pub profile: String,
    pub language: String,
    pub resolved_language: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub sources: Vec<SourceFileEntry>,
    pub metadata: FrontendBookMetadata,
    pub diagnostics: WorkspaceDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSection {
    pub id: String,
    pub file_id: String,
    pub volume_number: usize,
    pub title: String,
    pub original_title: String,
    pub subtitle: Option<String>,
    pub section_type: String,
    pub order: usize,
    pub start_page: usize,
    pub end_page: usize,
    pub paragraph_count: usize,
    pub word_count: usize,
    pub footnote_count: usize,
    pub chapter_file: String,
    pub image_ids: Vec<String>,
    pub paragraphs: Vec<Paragraph>,
    pub footnotes: Vec<Footnote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureTree {
    pub version: String,
    pub sections: Vec<WorkspaceSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructureOpRecord {
    pub timestamp: u64,
    pub op: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: String,
    pub file_id: String,
    pub volume_number: usize,
    pub page_num: usize,
    pub image_index: usize,
    pub filename: String,
    pub ext: String,
    pub width: u32,
    pub height: u32,
    pub category: String,
    pub status: String,
    pub is_kept: bool,
    pub is_cover: bool,
    pub is_full_page: bool,
    pub decorative: bool,
    pub alt_text: String,
    pub reason: String,
    pub binary_path: String,
    pub thumbnail_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaManifest {
    pub version: String,
    pub items: Vec<MediaItem>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorialRulesConfig {
    pub profile: String,
    pub language: String,
    pub editorial_rules: Vec<FrontendEditorialRule>,
    pub regex_rules: Vec<FrontendRegexRule>,
    pub glossary: Vec<FrontendGlossaryReplacement>,
    pub ignored_words: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellCluster {
    pub cluster_id: String,
    pub original_token: String,
    pub replacement_token: String,
    pub category: String,
    pub confidence: f64,
    pub occurrences: usize,
    pub section_ids: Vec<String>,
    pub sample_context: String,
    pub ignored: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellAuditData {
    pub language_mode: String,
    pub ignored_japanese_count: usize,
    pub ignored_foreign_count: usize,
    pub ignored_proper_nouns_count: usize,
    pub clusters: Vec<SpellCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResponse {
    pub status: String,
    pub workspace: String,
    pub profile: String,
    pub language: String,
    pub diagnostics: WorkspaceDiagnostics,
    pub sources: Vec<SourceFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionSummary {
    pub id: String,
    pub file_id: String,
    pub volume_number: usize,
    pub title: String,
    pub original_title: String,
    pub section_type: String,
    pub order: usize,
    pub start_page: usize,
    pub end_page: usize,
    pub paragraph_count: usize,
    pub word_count: usize,
    pub footnote_count: usize,
    pub image_count: usize,
    pub untitled: bool,
    pub chapter_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectSectionsResponse {
    pub workspace: String,
    pub total_sections: usize,
    pub chapters_without_title: usize,
    pub sections: Vec<SectionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParagraphPreview {
    pub index: usize,
    pub style: String,
    pub text: String,
    pub is_scene_break: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectSectionDetailResponse {
    pub id: String,
    pub title: String,
    pub original_title: String,
    pub section_type: String,
    pub volume_number: usize,
    pub start_page: usize,
    pub end_page: usize,
    pub total_paragraphs: usize,
    pub word_count: usize,
    pub head: Vec<ParagraphPreview>,
    pub tail: Vec<ParagraphPreview>,
    pub footnotes: Vec<Footnote>,
    pub image_ids: Vec<String>,
    pub chapter_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchStructureResponse {
    pub status: String,
    pub dry_run: bool,
    pub operations_applied: Vec<String>,
    pub total_sections: usize,
    pub chapters_without_title: usize,
    pub sections: Vec<SectionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectMediaResponse {
    pub workspace: String,
    pub filter: String,
    pub total_matching: usize,
    pub kept_count: usize,
    pub discarded_count: usize,
    pub uncertain_count: usize,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMediaResponse {
    pub status: String,
    pub dry_run: bool,
    pub updated_ids: Vec<String>,
    pub kept_count: usize,
    pub discarded_count: usize,
    pub uncertain_count: usize,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTextResponse {
    pub workspace: String,
    pub min_confidence: f64,
    pub limit: usize,
    pub total_active_clusters: usize,
    pub returned_clusters: usize,
    pub ignored_japanese_count: usize,
    pub ignored_foreign_count: usize,
    pub ignored_proper_nouns_count: usize,
    pub clusters: Vec<SpellCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryIgnoreResponse {
    pub status: String,
    pub added_words: Vec<String>,
    pub total_ignored_words: usize,
    pub ignored_clusters_count: usize,
    pub applied_spell_clusters: usize,
    pub modified_paragraphs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesAddResponse {
    pub status: String,
    pub dry_run: bool,
    pub rule_id: String,
    pub pattern: String,
    pub replacement: String,
    pub flags: String,
    pub affected_paragraphs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubVerificationChecks {
    pub ocf_mimetype_first_uncompressed: bool,
    pub container_xml_valid: bool,
    pub opf_epub3_metadata_valid: bool,
    pub xhtml_well_formed: bool,
    pub image_references_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyEpubResponse {
    pub valid: bool,
    pub epub_path: String,
    pub file_size_bytes: usize,
    pub checks: EpubVerificationChecks,
    pub xhtml_files_checked: usize,
    pub image_references_checked: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResponse {
    pub status: String,
    pub workspace: String,
    pub epub_path: String,
    pub epub_bytes: usize,
    pub exported_project: Option<String>,
    pub exported_project_bytes: Option<usize>,
    pub sections_built: usize,
    pub images_embedded: usize,
    pub verification: VerifyEpubResponse,
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub const SUPPORTED_PROFILES: &[&str] = &[
    "en-light-novel",
    "en-litrpg",
    "en-fiction",
    "es-editorial",
];

pub const SUPPORTED_LANGS: &[&str] = &["auto", "es", "en"];

pub fn validate_profile(profile: &str) -> Result<String> {
    if SUPPORTED_PROFILES.contains(&profile) {
        return Ok(profile.to_string());
    }
    bail!(
        "Unsupported profile '{}'. Expected one of: {}",
        profile,
        SUPPORTED_PROFILES.join(", ")
    );
}

pub fn validate_lang(lang: &str) -> Result<String> {
    if SUPPORTED_LANGS.contains(&lang) {
        return Ok(lang.to_string());
    }
    bail!(
        "Unsupported language '{}'. Expected one of: {}",
        lang,
        SUPPORTED_LANGS.join(", ")
    );
}

fn resolve_language(profile: &str, lang: &str) -> String {
    match lang {
        "es" => "es".to_string(),
        "en" => "en".to_string(),
        _ => {
            if profile == "es-editorial" {
                "es".to_string()
            } else {
                "en".to_string()
            }
        }
    }
}

fn is_untitled_section(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_lowercase();
    lower == "untitled"
        || lower == "sin título"
        || lower == "sin titulo"
        || lower.starts_with("section ")
        || lower.starts_with("sección ")
}

fn count_words(paragraphs: &[Paragraph]) -> usize {
    paragraphs
        .iter()
        .filter(|p| !p.is_scene_break)
        .map(|p| p.text.split_whitespace().count())
        .sum()
}

fn section_to_summary(sec: &WorkspaceSection) -> SectionSummary {
    SectionSummary {
        id: sec.id.clone(),
        file_id: sec.file_id.clone(),
        volume_number: sec.volume_number,
        title: sec.title.clone(),
        original_title: sec.original_title.clone(),
        section_type: sec.section_type.clone(),
        order: sec.order,
        start_page: sec.start_page,
        end_page: sec.end_page,
        paragraph_count: sec.paragraph_count,
        word_count: sec.word_count,
        footnote_count: sec.footnote_count,
        image_count: sec.image_ids.len(),
        untitled: is_untitled_section(&sec.title),
        chapter_file: sec.chapter_file.clone(),
    }
}

fn parse_epub_to_volume(epub_bytes: &[u8], filename: &str) -> Result<VolumeInfo> {
    let reader = Cursor::new(epub_bytes);
    let mut archive = ZipArchive::new(reader).context("Failed to open EPUB archive")?;

    let mut opf_content = String::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let name = f.name()?.to_string();
        if name.ends_with(".opf") {
            f.read_to_string(&mut opf_content)?;
            break;
        }
    }

    let title_re = Regex::new(r"(?s)<dc:title[^>]*>(.*?)</dc:title>")?;
    let title = title_re
        .captures(&opf_content)
        .map(|c| c[1].trim().to_string())
        .unwrap_or_else(|| filename.trim_end_matches(".epub").to_string());

    let (_, inferred_vol) = infer_series_and_volume(filename);
    let vol_re = Regex::new(r"(?i)(?:Volumen|Volume|Vol\.?)\s*(\d+)")?;
    let vol_num = vol_re
        .captures(&title)
        .or_else(|| vol_re.captures(filename))
        .and_then(|c| c[1].parse::<usize>().ok())
        .unwrap_or(inferred_vol.max(1));

    let pub_re = Regex::new(r"(?s)<dc:publisher[^>]*>(.*?)</dc:publisher>")?;
    let extracted_publisher = pub_re
        .captures(&opf_content)
        .map(|c| c[1].trim().to_string())
        .filter(|p| !p.is_empty());

    let tag_re = Regex::new(r"<[^>]+>")?;
    let desc_re = Regex::new(r"(?s)<dc:description[^>]*>(.*?)</dc:description>")?;
    let mut extracted_description = desc_re
        .captures(&opf_content)
        .map(|c| tag_re.replace_all(&c[1], "").trim().to_string())
        .filter(|d| !d.is_empty());

    let mut cover_image: Option<ExtractedImage> = None;
    let mut color_images: Vec<ExtractedImage> = Vec::new();
    let mut all_verdicts: Vec<ExtractedImage> = Vec::new();

    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let name = f.name()?.to_string();
        let lower = name.to_lowercase();
        if lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".png")
            || lower.ends_with(".webp")
        {
            let mut data = Vec::new();
            f.read_to_end(&mut data)?;
            let (width, height) = image::ImageReader::new(Cursor::new(&data))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok())
                .unwrap_or((800, 1200));
            let ext = Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_lowercase();
            let short_name = Path::new(&name)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&name)
                .to_string();
            let is_cov = cover_image.is_none()
                && (short_name.to_lowercase().contains("cover")
                    || short_name.contains("p001")
                    || all_verdicts.is_empty());
            let is_small = width < 250 || height < 250;
            let is_kept = !is_small;
            let cat = if is_cov && is_kept {
                "covers"
            } else if !is_kept {
                "icons_and_avatars"
            } else if short_name.to_lowercase().contains("color") {
                "color_inserts"
            } else {
                "story_illustrations"
            };
            let reason = if is_kept {
                "EPUB embedded illustration".to_string()
            } else {
                format!("Sub-resolution icon or stamp ({}x{} < 250px)", width, height)
            };
            let img = ExtractedImage {
                page_num: all_verdicts.len() + 1,
                image_index: all_verdicts.len() + 1,
                ext,
                data,
                width,
                height,
                filename: short_name,
                category: cat.to_string(),
                y_pos: 0.0,
                is_full_page: is_kept,
                is_cover: is_cov && is_kept,
                is_kept,
                reason,
            };
            if img.is_cover {
                cover_image = Some(img.clone());
            } else if cat == "color_inserts" {
                color_images.push(img.clone());
            }
            all_verdicts.push(img);
        }
    }

    let mut xhtml_entries: Vec<(String, String)> = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let name = f.name()?.to_string();
        let lower = name.to_lowercase();
        if (lower.ends_with(".xhtml") || lower.ends_with(".html"))
            && !lower.ends_with("nav.xhtml")
            && !lower.contains("cover")
            && !lower.contains("color_")
            && !lower.contains("colophon")
            && !lower.contains("_title")
        {
            let mut text = String::new();
            if f.read_to_string(&mut text).is_ok() {
                xhtml_entries.push((name, text));
            }
        }
    }
    xhtml_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let h1_re = Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>")?;
    let h2_re = Regex::new(r"(?is)<h2[^>]*>(.*?)</h2>")?;
    let block_re = Regex::new(
        r#"(?is)<(?P<tag>p|div|hr)\b(?P<attrs>[^>]*)>(?:(?P<content>.*?)</(?:p|div)>)?"#,
    )?;
    let cleaner = TextCleaner::new();

    let mut sections: Vec<Section> = Vec::new();
    for (idx, (_, xhtml)) in xhtml_entries.into_iter().enumerate() {
        let h1 = h1_re
            .captures(&xhtml)
            .map(|c| tag_re.replace_all(&c[1], "").trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Untitled Section {}", idx + 1));
        let h2 = h2_re
            .captures(&xhtml)
            .map(|c| tag_re.replace_all(&c[1], "").trim().to_string())
            .filter(|s| !s.is_empty());
        let full_title = match h2 {
            Some(ref sub) => format!("{}: {}", h1, sub),
            None => h1,
        };
        let mut sec = Section::new(full_title, h2, idx + 1, idx + 1);
        sec.section_type = Some(classify_section_type(&sec.title).to_string());

        for cap in block_re.captures_iter(&xhtml) {
            let tag = cap
                .name("tag")
                .map(|m| m.as_str().to_lowercase())
                .unwrap_or_default();
            let attrs = cap.name("attrs").map(|m| m.as_str()).unwrap_or_default();
            let raw_content = cap.name("content").map(|m| m.as_str()).unwrap_or_default();

            let is_break_tag =
                tag == "hr" || attrs.contains("scene-break") || attrs.contains("scene_break");
            if tag == "div" && !is_break_tag {
                continue;
            }
            let inner = tag_re.replace_all(raw_content, "").trim().to_string();
            if is_break_tag || cleaner.is_scene_break(&inner) {
                let sym = if inner.is_empty() {
                    "* * *".to_string()
                } else {
                    inner
                };
                sec.paragraphs.push(Paragraph::scene_break_with_text(&sym));
                sec.content_items
                    .push(ContentItem::scene_break_with_text(&sym, idx + 1));
                continue;
            }
            if inner.is_empty() {
                continue;
            }
            let style = if attrs.contains("dialogue") || inner.starts_with('—') {
                "dialogue"
            } else if attrs.contains("thought") || inner.starts_with('«') {
                "thought"
            } else {
                "narrative"
            };
            sec.paragraphs
                .push(Paragraph::with_style(inner.clone(), style));
            sec.content_items.push(ContentItem::text(
                inner,
                style.to_string(),
                Vec::new(),
                idx + 1,
            ));
        }
        sections.push(sec);
    }

    if extracted_description.is_none() {
        extracted_description = detect_synopsis_from_sections(&sections);
    }

    Ok(VolumeInfo {
        pdf_path: filename.to_string(),
        volume_number: vol_num,
        title,
        total_pages: sections.len().max(1),
        cover_image,
        color_images,
        sections,
        all_verdicts,
        spell_transformations: Vec::new(),
        publisher: extracted_publisher,
        description: extracted_description,
    })
}

fn generate_thumbnail_bytes(data: &[u8]) -> Vec<u8> {
    if let Ok(img) = image::load_from_memory(data) {
        let thumb = img.thumbnail(256, 256);
        let mut out = Cursor::new(Vec::new());
        if thumb.write_to(&mut out, image::ImageFormat::Png).is_ok() {
            return out.into_inner();
        }
    }
    data.to_vec()
}

fn classify_media_status(img: &ExtractedImage) -> String {
    if img.is_kept {
        "kept".to_string()
    } else if img.reason.contains("aspect")
        || img.reason.contains("front-matter")
        || (img.width >= 200 && img.height >= 200 && !img.reason.contains("dHash"))
    {
        "uncertain".to_string()
    } else {
        "discarded".to_string()
    }
}

fn build_workspace_section(
    sec_id: String,
    file_id: String,
    volume_number: usize,
    order: usize,
    sec: &Section,
    image_id_by_filename: &HashMap<String, String>,
) -> WorkspaceSection {
    let mut paragraphs = sec.paragraphs.clone();
    if paragraphs.is_empty() {
        for item in &sec.content_items {
            if let ContentItemPayload::Paragraph(ref p) = item.payload {
                paragraphs.push(p.clone());
            }
        }
    }
    let mut image_ids = Vec::new();
    for item in &sec.content_items {
        if let ContentItemPayload::Image(ref img) = item.payload {
            if let Some(id) = image_id_by_filename.get(&img.filename) {
                if !image_ids.contains(id) {
                    image_ids.push(id.clone());
                }
            }
        }
    }
    let word_count = count_words(&paragraphs);
    let sec_type = sec
        .section_type
        .clone()
        .unwrap_or_else(|| classify_section_type(&sec.title).to_string());
    let chapter_file = format!("structure/chapters/{}.xhtml", sec_id);

    WorkspaceSection {
        id: sec_id,
        file_id,
        volume_number,
        title: sec.title.clone(),
        original_title: sec.title.clone(),
        subtitle: sec.subtitle.clone(),
        section_type: sec_type,
        order,
        start_page: sec.start_page,
        end_page: sec.end_page,
        paragraph_count: paragraphs.len(),
        word_count,
        footnote_count: sec.footnotes.len(),
        chapter_file,
        image_ids,
        paragraphs,
        footnotes: sec.footnotes.clone(),
    }
}

fn to_engine_section(wsec: &WorkspaceSection, media_by_id: &HashMap<String, (&MediaItem, Vec<u8>)>) -> Section {
    let mut sec = Section::new(
        wsec.title.clone(),
        wsec.subtitle.clone(),
        wsec.start_page,
        wsec.end_page,
    );
    sec.section_type = Some(wsec.section_type.clone());
    sec.paragraphs = wsec.paragraphs.clone();
    sec.footnotes = wsec.footnotes.clone();

    for p in &wsec.paragraphs {
        if p.is_scene_break {
            sec.content_items
                .push(ContentItem::scene_break_with_text(&p.text, wsec.start_page));
        } else if p.is_part_heading {
            sec.content_items.push(ContentItem {
                item_type: "part_heading".to_string(),
                payload: ContentItemPayload::Paragraph(p.clone()),
                page_num: wsec.start_page,
                anchor: String::new(),
            });
        } else {
            sec.content_items.push(ContentItem::text(
                p.text.clone(),
                p.style.clone(),
                p.footnotes.clone(),
                wsec.start_page,
            ));
        }
    }

    for img_id in &wsec.image_ids {
        if let Some((item, bytes)) = media_by_id.get(img_id) {
            if item.is_kept {
                let ext_img = ExtractedImage {
                    page_num: item.page_num,
                    image_index: item.image_index,
                    ext: item.ext.clone(),
                    data: bytes.clone(),
                    width: item.width,
                    height: item.height,
                    filename: item.filename.clone(),
                    category: item.category.clone(),
                    y_pos: 0.0,
                    is_full_page: item.is_full_page,
                    is_cover: item.is_cover,
                    is_kept: item.is_kept,
                    reason: item.reason.clone(),
                };
                let item_type = if item.is_full_page {
                    "full_illustration"
                } else {
                    "image"
                };
                sec.content_items
                    .push(ContentItem::image(item_type, ext_img, item.page_num));
            }
        }
    }

    sec
}

fn apply_alt_and_decorative_to_xhtml(xhtml: &str, media_items: &[MediaItem]) -> String {
    let mut out = xhtml.to_string();
    for item in media_items {
        if !item.is_kept {
            continue;
        }
        let escaped_fname = regex::escape(&item.filename);
        let pat = format!(
            r#"(<img\b[^>]*src="[^"]*{}"[^>]*?)\balt="[^"]*"([^>]*/>)"#,
            escaped_fname
        );
        if let Ok(re) = Regex::new(&pat) {
            if item.decorative {
                let rep = format!(r#"$1alt="" role="presentation"$2"#);
                out = re.replace_all(&out, rep.as_str()).to_string();
            } else if !item.alt_text.trim().is_empty() {
                let safe_alt = html_escape(item.alt_text.trim());
                let rep = format!(r#"$1alt="{}"$2"#, safe_alt);
                out = re.replace_all(&out, rep.as_str()).to_string();
            }
        }
    }
    sanitize_html_for_xhtml(&out)
}

fn render_and_write_chapters(
    workspace_dir: &Path,
    sections: &[WorkspaceSection],
    media_items: &[MediaItem],
) -> Result<()> {
    let chapters_dir = workspace_dir.join("structure").join("chapters");
    if chapters_dir.exists() {
        for entry in fs::read_dir(&chapters_dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "xhtml") {
                let _ = fs::remove_file(entry.path());
            }
        }
    } else {
        fs::create_dir_all(&chapters_dir)?;
    }

    let mut media_by_id: HashMap<String, (&MediaItem, Vec<u8>)> = HashMap::new();
    for item in media_items {
        let bin_path = workspace_dir.join(&item.binary_path);
        let bytes = fs::read(&bin_path).unwrap_or_default();
        media_by_id.insert(item.id.clone(), (item, bytes));
    }

    let builder = EpubBuilder::new();
    for wsec in sections {
        let eng_sec = to_engine_section(wsec, &media_by_id);
        let raw_xhtml = builder.render_section_xhtml_custom(&eng_sec, "");
        let final_xhtml = apply_alt_and_decorative_to_xhtml(&raw_xhtml, media_items);
        let out_path = workspace_dir.join(&wsec.chapter_file);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(out_path, final_xhtml)?;
    }
    Ok(())
}

fn append_structure_op(workspace_dir: &Path, op: &str, details: serde_json::Value) -> Result<()> {
    let ops_path = workspace_dir.join("structure").join("ops.jsonl");
    if let Some(parent) = ops_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let record = StructureOpRecord {
        timestamp: unix_timestamp(),
        op: op.to_string(),
        details,
    };
    let mut line = serde_json::to_string(&record)?;
    line.push('\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ops_path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn compute_spell_audit(
    sections: &[WorkspaceSection],
    language_mode: &str,
    ignored_words: &[String],
) -> SpellAuditData {
    let corrector = ContextualSpellCorrector::default();
    let opts = SpellAuditOptions {
        language_mode: if language_mode == "es" || language_mode == "en" {
            language_mode.to_string()
        } else {
            "hybrid".to_string()
        },
        ignore_japanese: true,
        ignore_foreign: true,
        ignore_proper_nouns: true,
        ignored_words: ignored_words.to_vec(),
    };
    let mut stats = SpellAuditStats::default();
    let ignored_lower: HashSet<String> = ignored_words.iter().map(|w| w.to_lowercase()).collect();

    let mut cluster_map: BTreeMap<(String, String), SpellCluster> = BTreeMap::new();
    let mut next_idx = 1usize;

    for sec in sections {
        let mut paras_clone = sec.paragraphs.clone();
        let (corrections, _) = corrector.audit_section_paragraphs(
            &sec.file_id,
            &mut paras_clone,
            sec.volume_number,
            sec.start_page,
            &sec.title,
            &opts,
            &mut stats,
            false,
            next_idx,
        );
        next_idx += corrections.len();

        for c in corrections {
            let key = (
                c.original_token.to_lowercase(),
                c.replacement_token.to_lowercase(),
            );
            let is_ign = ignored_lower.contains(&c.original_token.to_lowercase());
            let entry = cluster_map.entry(key).or_insert_with(|| SpellCluster {
                cluster_id: String::new(),
                original_token: c.original_token.clone(),
                replacement_token: c.replacement_token.clone(),
                category: c.category.clone(),
                confidence: c.confidence,
                occurrences: 0,
                section_ids: Vec::new(),
                sample_context: c.context_snippet.clone(),
                ignored: is_ign,
                applied: false,
            });
            entry.occurrences += 1;
            if c.confidence > entry.confidence {
                entry.confidence = c.confidence;
            }
            if !entry.section_ids.contains(&sec.id) {
                entry.section_ids.push(sec.id.clone());
            }
        }
    }

    let mut clusters: Vec<SpellCluster> = cluster_map.into_values().collect();
    clusters.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then_with(|| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal))
    });
    for (idx, cl) in clusters.iter_mut().enumerate() {
        cl.cluster_id = format!("sp_{:03}", idx + 1);
    }

    SpellAuditData {
        language_mode: opts.language_mode,
        ignored_japanese_count: stats.ignored_japanese_count,
        ignored_foreign_count: stats.ignored_foreign_count,
        ignored_proper_nouns_count: stats.ignored_proper_nouns_count,
        clusters,
    }
}

fn default_editorial_rules(sections: &[WorkspaceSection]) -> Vec<FrontendEditorialRule> {
    let mut dash_count = 0usize;
    let mut quote_count = 0usize;
    let mut ellipsis_count = 0usize;
    let mut break_count = 0usize;

    for sec in sections {
        for p in &sec.paragraphs {
            if p.is_scene_break {
                break_count += 1;
            } else {
                if p.style == "dialogue" || p.text.contains('—') || p.text.contains("--") {
                    dash_count += 1;
                }
                if p.style == "thought"
                    || p.text.contains('«')
                    || p.text.contains('“')
                    || p.text.contains('"')
                {
                    quote_count += 1;
                }
                ellipsis_count += p.text.matches('…').count() + p.text.matches("...").count();
            }
        }
    }

    vec![
        FrontendEditorialRule {
            id: "rule-dialogue-dashes".to_string(),
            label: "Standardize Dialogue Em-Dashes".to_string(),
            description: "Replaces double hyphens and standard dashes in spoken lines with typographic em-dashes.".to_string(),
            enabled: true,
            affected_count: dash_count,
        },
        FrontendEditorialRule {
            id: "rule-curly-quotes".to_string(),
            label: "Smart Curly Quotes & Brackets".to_string(),
            description: "Transforms straight quotation marks and angle brackets into curved or guillemet quotes.".to_string(),
            enabled: true,
            affected_count: quote_count,
        },
        FrontendEditorialRule {
            id: "rule-ellipsis".to_string(),
            label: "Typographic Ellipses".to_string(),
            description: "Substitutes three dots (...) with a single horizontal ellipsis glyph.".to_string(),
            enabled: true,
            affected_count: ellipsis_count,
        },
        FrontendEditorialRule {
            id: "rule-clean-breaks".to_string(),
            label: "Consolidate Multiple Blank Lines & Breaks".to_string(),
            description: "Removes unnecessary blank lines and unifies deliberate scene transitions.".to_string(),
            enabled: true,
            affected_count: break_count,
        },
    ]
}

fn refresh_manifest_diagnostics(workspace_dir: &Path) -> Result<WorkspaceManifest> {
    let manifest_path = workspace_dir.join("manifest.json");
    let mut manifest: WorkspaceManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let tree: StructureTree =
        serde_json::from_str(&fs::read_to_string(workspace_dir.join("structure").join("tree.json"))?)?;
    let media: MediaManifest =
        serde_json::from_str(&fs::read_to_string(workspace_dir.join("media").join("manifest.json"))?)?;
    let spell: SpellAuditData =
        serde_json::from_str(&fs::read_to_string(workspace_dir.join("rules").join("spell_audit.json"))?)?;

    let untitled_ids: Vec<String> = tree
        .sections
        .iter()
        .filter(|s| is_untitled_section(&s.title))
        .map(|s| s.id.clone())
        .collect();
    let total_paragraphs: usize = tree.sections.iter().map(|s| s.paragraph_count).sum();
    let kept_images = media.items.iter().filter(|i| i.status == "kept").count();
    let discarded_images = media.items.iter().filter(|i| i.status == "discarded").count();
    let uncertain_images = media.items.iter().filter(|i| i.status == "uncertain").count();
    let active_clusters = spell
        .clusters
        .iter()
        .filter(|c| !c.ignored && !c.applied)
        .count();

    manifest.updated_at = unix_timestamp();
    manifest.diagnostics = WorkspaceDiagnostics {
        has_text_layer: manifest.sources.iter().all(|s| s.has_text_layer) && total_paragraphs > 0,
        total_sources: manifest.sources.len(),
        total_pages: manifest.sources.iter().map(|s| s.total_pages).sum(),
        total_sections: tree.sections.len(),
        total_paragraphs,
        chapters_without_title: untitled_ids.len(),
        untitled_section_ids: untitled_ids,
        kept_images,
        discarded_images,
        uncertain_images,
        spell_clusters: active_clusters,
    };

    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    Ok(manifest)
}

pub fn import_editnovel_archive_with_rc_zip(archive_bytes: &[u8], target_dir: &Path) -> Result<()> {
    let mut fsm = ArchiveFsm::new(archive_bytes.len() as u64);
    let archive = loop {
        if let Some(offset) = fsm.wants_read() {
            let start = (offset as usize).min(archive_bytes.len());
            let space = fsm.space();
            let avail = archive_bytes.len().saturating_sub(start);
            let to_copy = space.len().min(avail);
            space[..to_copy].copy_from_slice(&archive_bytes[start..start + to_copy]);
            fsm.fill(to_copy);
        }
        match fsm
            .process()
            .map_err(|e| anyhow!("rc-zip archive parse failed: {}", e))?
        {
            FsmResult::Continue(next) => fsm = next,
            FsmResult::Done(arch) => break arch,
        }
    };

    fs::create_dir_all(target_dir)?;

    for entry in archive.entries() {
        let Some(rel_name) = entry.sanitized_name() else {
            continue;
        };
        if rel_name.is_empty() || rel_name.ends_with('/') {
            continue;
        }
        let mut cursor = entry.header_offset as usize;
        let mut entry_fsm = EntryFsm::new(Some(entry.clone()), None);
        let mut out = Vec::with_capacity(entry.uncompressed_size as usize);
        let mut chunk = [0u8; 32768];
        loop {
            if entry_fsm.wants_read() && cursor < archive_bytes.len() {
                let space = entry_fsm.space();
                let avail = archive_bytes.len() - cursor;
                let to_copy = space.len().min(avail);
                if to_copy > 0 {
                    space[..to_copy].copy_from_slice(&archive_bytes[cursor..cursor + to_copy]);
                    entry_fsm.fill(to_copy);
                    cursor += to_copy;
                }
            }
            match entry_fsm
                .process(&mut chunk)
                .map_err(|e| anyhow!("rc-zip entry '{}' failed: {}", rel_name, e))?
            {
                FsmResult::Continue((next_fsm, outcome)) => {
                    if outcome.bytes_written > 0 {
                        out.extend_from_slice(&chunk[..outcome.bytes_written]);
                    }
                    entry_fsm = next_fsm;
                }
                FsmResult::Done(_) => break,
            }
        }
        let dest = target_dir.join(rel_name);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, out)?;
    }

    Ok(())
}

pub fn export_editnovel_archive(workspace_dir: &Path, out_file: &Path) -> Result<usize> {
    if let Some(parent) = out_file.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zw = ZipWriter::new(&mut buf);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let mut stack = vec![workspace_dir.to_path_buf()];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    files.push(p);
                }
            }
        }
        files.sort();

        for file_path in files {
            let rel = file_path
                .strip_prefix(workspace_dir)?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&file_path)?;
            let is_text_or_json = rel.ends_with(".json")
                || rel.ends_with(".jsonl")
                || rel.ends_with(".xhtml")
                || rel.ends_with(".html")
                || rel.ends_with(".css")
                || rel.ends_with(".xml");
            let opts = if is_text_or_json { deflated } else { stored };
            zw.start_file(&rel, opts)?;
            zw.write_all(&bytes)?;
        }
        zw.finish()?;
    }

    let archive_bytes = buf.into_inner();
    let len = archive_bytes.len();
    fs::write(out_file, archive_bytes)?;
    Ok(len)
}

fn ensure_workspace_unpacked(workspace_path: &Path) -> Result<PathBuf> {
    if workspace_path.is_file()
        && workspace_path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("editnovel"))
    {
        let stem = workspace_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        let unpack_dir = std::env::temp_dir().join(format!("editnovel_ws_{}", stem));
        let bytes = fs::read(workspace_path)?;
        import_editnovel_archive_with_rc_zip(&bytes, &unpack_dir)?;
        return Ok(unpack_dir);
    }
    Ok(workspace_path.to_path_buf())
}

pub fn ingest_sources(
    files: &[PathBuf],
    workspace_dir: &Path,
    profile: &str,
    lang: &str,
) -> Result<IngestResponse> {
    let profile = validate_profile(profile)?;
    let lang = validate_lang(lang)?;
    let resolved_lang = resolve_language(&profile, &lang);

    if files.is_empty() {
        bail!("At least one input file (.pdf, .epub, or .editnovel) is required");
    }

    if files.len() == 1
        && files[0]
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("editnovel"))
    {
        let bytes = fs::read(&files[0])
            .with_context(|| format!("Failed to read {:?}", files[0]))?;
        import_editnovel_archive_with_rc_zip(&bytes, workspace_dir)?;
        let manifest = refresh_manifest_diagnostics(workspace_dir)?;
        return Ok(IngestResponse {
            status: "ok".to_string(),
            workspace: workspace_dir.to_string_lossy().to_string(),
            profile: manifest.profile,
            language: manifest.resolved_language,
            diagnostics: manifest.diagnostics,
            sources: manifest.sources,
        });
    }

    fs::create_dir_all(workspace_dir.join("structure").join("chapters"))?;
    fs::create_dir_all(workspace_dir.join("media"))?;
    fs::create_dir_all(workspace_dir.join("rules"))?;

    let mut sources: Vec<SourceFileEntry> = Vec::new();
    let mut all_sections: Vec<WorkspaceSection> = Vec::new();
    let mut all_media: Vec<MediaItem> = Vec::new();
    let mut first_vol_title = String::new();
    let mut first_series = String::new();
    let mut first_publisher = String::new();
    let mut first_description = String::new();

    let mut sorted_files = files.to_vec();
    sorted_files.sort_by_key(|p| {
        let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        infer_series_and_volume(fname).1
    });

    for (file_idx, path) in sorted_files.iter().enumerate() {
        if !path.exists() {
            bail!("Input file does not exist: {:?}", path);
        }
        let fname = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("volume.pdf")
            .to_string();
        let bytes = fs::read(path).with_context(|| format!("Failed to read {:?}", path))?;
        let is_epub = fname.to_lowercase().ends_with(".epub");

        let (vol, has_text_layer) = if is_epub {
            let v = parse_epub_to_volume(&bytes, &fname)?;
            let has_text = v.sections.iter().any(|s| !s.paragraphs.is_empty());
            (v, has_text)
        } else {
            let doc = ParsedPdfDocument::from_bytes(&bytes)
                .map_err(|e| anyhow!("Failed to parse PDF '{}': {}", fname, e))?;
            let span_count: usize = doc
                .pages
                .iter()
                .flat_map(|p| &p.blocks)
                .flat_map(|b| &b.lines)
                .map(|l| l.spans.len())
                .sum();
            let mut builder = EpubBuilder::new();
            let v = builder.process_parsed_pdf(&doc, &fname, true);
            (v, span_count > 0)
        };

        let vol_num = if sorted_files.len() > 1 && vol.volume_number == 1 && file_idx > 0 {
            file_idx + 1
        } else {
            vol.volume_number
        };

        if first_vol_title.is_empty() {
            first_vol_title = vol.title.clone();
            first_series = infer_series_and_volume(&fname).0;
        }
        if first_publisher.is_empty() {
            if let Some(ref p) = vol.publisher {
                first_publisher = p.clone();
            }
        }
        if first_description.is_empty() {
            if let Some(ref d) = vol.description {
                first_description = d.clone();
            }
        }

        sources.push(SourceFileEntry {
            file_id: fname.clone(),
            file_name: fname.clone(),
            volume_number: vol_num,
            total_pages: vol.total_pages,
            has_text_layer,
        });

        let mut image_id_by_filename: HashMap<String, String> = HashMap::new();
        for img in &vol.all_verdicts {
            let media_idx = all_media.len() + 1;
            let media_id = format!("img_{:03}", media_idx);
            let ext = if img.ext.is_empty() {
                "png".to_string()
            } else {
                img.ext.to_lowercase()
            };
            let bin_rel = format!("media/{}.{}", media_id, ext);
            let thumb_rel = format!("media/thumb_{}.png", media_id);

            fs::write(workspace_dir.join(&bin_rel), &img.data)?;
            let thumb_bytes = generate_thumbnail_bytes(&img.data);
            fs::write(workspace_dir.join(&thumb_rel), thumb_bytes)?;

            let status = classify_media_status(img);
            let default_alt = if img.is_cover {
                "Cover illustration".to_string()
            } else if img.is_kept {
                format!("Illustration on page {}", img.page_num)
            } else {
                String::new()
            };

            image_id_by_filename.insert(img.filename.clone(), media_id.clone());
            all_media.push(MediaItem {
                id: media_id,
                file_id: fname.clone(),
                volume_number: vol_num,
                page_num: img.page_num,
                image_index: img.image_index,
                filename: img.filename.clone(),
                ext,
                width: img.width,
                height: img.height,
                category: img.category.clone(),
                status,
                is_kept: img.is_kept,
                is_cover: img.is_cover,
                is_full_page: img.is_full_page,
                decorative: false,
                alt_text: default_alt,
                reason: img.reason.clone(),
                binary_path: bin_rel,
                thumbnail_path: thumb_rel,
            });
        }

        for sec in &vol.sections {
            let sec_idx = all_sections.len() + 1;
            let sec_id = format!("s{:02}", sec_idx);
            let wsec = build_workspace_section(
                sec_id,
                fname.clone(),
                vol_num,
                sec_idx - 1,
                sec,
                &image_id_by_filename,
            );
            all_sections.push(wsec);
        }
    }

    render_and_write_chapters(workspace_dir, &all_sections, &all_media)?;

    let tree = StructureTree {
        version: "2.0".to_string(),
        sections: all_sections.clone(),
    };
    fs::write(
        workspace_dir.join("structure").join("tree.json"),
        serde_json::to_string_pretty(&tree)?,
    )?;

    fs::write(workspace_dir.join("structure").join("ops.jsonl"), "")?;
    append_structure_op(
        workspace_dir,
        "ingest",
        serde_json::json!({
            "sources": sources.iter().map(|s| &s.file_name).collect::<Vec<_>>(),
            "sections": all_sections.len(),
            "media": all_media.len(),
        }),
    )?;

    let media_manifest = MediaManifest {
        version: "2.0".to_string(),
        items: all_media.clone(),
    };
    fs::write(
        workspace_dir.join("media").join("manifest.json"),
        serde_json::to_string_pretty(&media_manifest)?,
    )?;

    let editorial_config = EditorialRulesConfig {
        profile: profile.clone(),
        language: resolved_lang.clone(),
        editorial_rules: default_editorial_rules(&all_sections),
        regex_rules: Vec::new(),
        glossary: Vec::new(),
        ignored_words: Vec::new(),
    };
    fs::write(
        workspace_dir.join("rules").join("editorial.json"),
        serde_json::to_string_pretty(&editorial_config)?,
    )?;

    let spell_audit = compute_spell_audit(&all_sections, &resolved_lang, &[]);
    fs::write(
        workspace_dir.join("rules").join("spell_audit.json"),
        serde_json::to_string_pretty(&spell_audit)?,
    )?;

    let untitled_ids: Vec<String> = all_sections
        .iter()
        .filter(|s| is_untitled_section(&s.title))
        .map(|s| s.id.clone())
        .collect();
    let total_paragraphs: usize = all_sections.iter().map(|s| s.paragraph_count).sum();
    let kept_images = all_media.iter().filter(|i| i.status == "kept").count();
    let discarded_images = all_media.iter().filter(|i| i.status == "discarded").count();
    let uncertain_images = all_media.iter().filter(|i| i.status == "uncertain").count();

    let diagnostics = WorkspaceDiagnostics {
        has_text_layer: sources.iter().all(|s| s.has_text_layer) && total_paragraphs > 0,
        total_sources: sources.len(),
        total_pages: sources.iter().map(|s| s.total_pages).sum(),
        total_sections: all_sections.len(),
        total_paragraphs,
        chapters_without_title: untitled_ids.len(),
        untitled_section_ids: untitled_ids,
        kept_images,
        discarded_images,
        uncertain_images,
        spell_clusters: spell_audit.clusters.len(),
    };

    let now = unix_timestamp();
    let metadata = FrontendBookMetadata {
        title: if sources.len() > 1 {
            format!("{} Omnibus", first_series)
        } else {
            first_vol_title
        },
        author: String::new(),
        illustrator: String::new(),
        series: first_series,
        volume: if sources.len() > 1 {
            format!("Volumes 1-{}", sources.len())
        } else {
            "Volume 01".to_string()
        },
        language: resolved_lang.clone(),
        publisher: first_publisher,
        description: first_description,
        isbn: None,
        published_year: None,
        copyright_notice: None,
        translator: None,
    };

    let manifest = WorkspaceManifest {
        version: "2.0".to_string(),
        profile: profile.clone(),
        language: lang.clone(),
        resolved_language: resolved_lang.clone(),
        created_at: now,
        updated_at: now,
        sources: sources.clone(),
        metadata,
        diagnostics: diagnostics.clone(),
    };

    fs::write(
        workspace_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    Ok(IngestResponse {
        status: "ok".to_string(),
        workspace: workspace_dir.to_string_lossy().to_string(),
        profile,
        language: resolved_lang,
        diagnostics,
        sources,
    })
}

pub fn inspect_sections(workspace_path: &Path) -> Result<InspectSectionsResponse> {
    let ws_dir = ensure_workspace_unpacked(workspace_path)?;
    let tree_path = ws_dir.join("structure").join("tree.json");
    let tree: StructureTree = serde_json::from_str(
        &fs::read_to_string(&tree_path)
            .with_context(|| format!("Workspace structure/tree.json not found at {:?}", tree_path))?,
    )?;

    let summaries: Vec<SectionSummary> = tree.sections.iter().map(section_to_summary).collect();
    let untitled_count = summaries.iter().filter(|s| s.untitled).count();

    Ok(InspectSectionsResponse {
        workspace: workspace_path.to_string_lossy().to_string(),
        total_sections: summaries.len(),
        chapters_without_title: untitled_count,
        sections: summaries,
    })
}

pub fn inspect_section(
    workspace_path: &Path,
    section_id: &str,
    head: usize,
    tail: usize,
) -> Result<InspectSectionDetailResponse> {
    let ws_dir = ensure_workspace_unpacked(workspace_path)?;
    let tree_path = ws_dir.join("structure").join("tree.json");
    let tree: StructureTree = serde_json::from_str(&fs::read_to_string(&tree_path)?)?;

    let sec = tree
        .sections
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(section_id))
        .ok_or_else(|| anyhow!("Section '{}' not found in workspace", section_id))?;

    let total = sec.paragraphs.len();
    let head_count = head.min(total);
    let head_previews: Vec<ParagraphPreview> = sec
        .paragraphs
        .iter()
        .enumerate()
        .take(head_count)
        .map(|(idx, p)| ParagraphPreview {
            index: idx,
            style: p.style.clone(),
            text: p.text.clone(),
            is_scene_break: p.is_scene_break,
        })
        .collect();

    let tail_start = total.saturating_sub(tail);
    let tail_previews: Vec<ParagraphPreview> = sec
        .paragraphs
        .iter()
        .enumerate()
        .skip(tail_start)
        .map(|(idx, p)| ParagraphPreview {
            index: idx,
            style: p.style.clone(),
            text: p.text.clone(),
            is_scene_break: p.is_scene_break,
        })
        .collect();

    Ok(InspectSectionDetailResponse {
        id: sec.id.clone(),
        title: sec.title.clone(),
        original_title: sec.original_title.clone(),
        section_type: sec.section_type.clone(),
        volume_number: sec.volume_number,
        start_page: sec.start_page,
        end_page: sec.end_page,
        total_paragraphs: total,
        word_count: sec.word_count,
        head: head_previews,
        tail: tail_previews,
        footnotes: sec.footnotes.clone(),
        image_ids: sec.image_ids.clone(),
        chapter_file: sec.chapter_file.clone(),
    })
}

pub fn patch_structure(
    workspace_dir: &Path,
    merges: &[String],
    renames: &[String],
    retypes: &[String],
    reorder: Option<&str>,
    dry_run: bool,
) -> Result<PatchStructureResponse> {
    let tree_path = workspace_dir.join("structure").join("tree.json");
    let mut tree: StructureTree = serde_json::from_str(&fs::read_to_string(&tree_path)?)?;
    let mut ops_applied: Vec<String> = Vec::new();

    for merge_spec in merges {
        let parts: Vec<String> = merge_spec
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() < 2 {
            bail!("Invalid --merge specification '{}'; expected id1,id2", merge_spec);
        }
        let target_id = &parts[0];
        if !tree.sections.iter().any(|s| s.id.eq_ignore_ascii_case(target_id)) {
            bail!("Target section '{}' for merge not found", target_id);
        }
        for source_id in &parts[1..] {
            if source_id.eq_ignore_ascii_case(target_id) {
                continue;
            }
            let src_pos = tree
                .sections
                .iter()
                .position(|s| s.id.eq_ignore_ascii_case(source_id))
                .ok_or_else(|| anyhow!("Source section '{}' for merge not found", source_id))?;
            let src_sec = tree.sections.remove(src_pos);
            let dst_sec = tree
                .sections
                .iter_mut()
                .find(|s| s.id.eq_ignore_ascii_case(target_id))
                .ok_or_else(|| anyhow!("Target section '{}' for merge not found", target_id))?;

            dst_sec.paragraphs.extend(src_sec.paragraphs);
            dst_sec.footnotes.extend(src_sec.footnotes);
            for img_id in src_sec.image_ids {
                if !dst_sec.image_ids.contains(&img_id) {
                    dst_sec.image_ids.push(img_id);
                }
            }
            dst_sec.end_page = dst_sec.end_page.max(src_sec.end_page);
            dst_sec.paragraph_count = dst_sec.paragraphs.len();
            dst_sec.word_count = count_words(&dst_sec.paragraphs);
            dst_sec.footnote_count = dst_sec.footnotes.len();
        }
        ops_applied.push(format!("merge:{}", merge_spec));
    }

    for rename_spec in renames {
        let (id, new_title) = rename_spec
            .split_once('=')
            .ok_or_else(|| anyhow!("Invalid --rename '{}'; expected id=title", rename_spec))?;
        let id = id.trim();
        let new_title = new_title.trim().trim_matches('"').to_string();
        let sec = tree
            .sections
            .iter_mut()
            .find(|s| s.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| anyhow!("Section '{}' not found for rename", id))?;
        sec.title = new_title.clone();
        sec.section_type = classify_section_type(&sec.title).to_string();
        ops_applied.push(format!("rename:{}={}", id, new_title));
    }

    for retype_spec in retypes {
        let (id, new_type) = retype_spec
            .split_once('=')
            .ok_or_else(|| anyhow!("Invalid --retype '{}'; expected id=type", retype_spec))?;
        let id = id.trim();
        let new_type = new_type.trim().to_string();
        let sec = tree
            .sections
            .iter_mut()
            .find(|s| s.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| anyhow!("Section '{}' not found for retype", id))?;
        sec.section_type = new_type.clone();
        ops_applied.push(format!("retype:{}={}", id, new_type));
    }

    if let Some(order_spec) = reorder {
        let desired_ids: Vec<String> = order_spec
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let mut reordered = Vec::with_capacity(tree.sections.len());
        for wanted in &desired_ids {
            if let Some(pos) = tree
                .sections
                .iter()
                .position(|s| s.id.eq_ignore_ascii_case(wanted))
            {
                reordered.push(tree.sections.remove(pos));
            } else {
                bail!("Section '{}' in --reorder not found", wanted);
            }
        }
        reordered.append(&mut tree.sections);
        tree.sections = reordered;
        ops_applied.push(format!("reorder:{}", order_spec));
    }

    for (idx, sec) in tree.sections.iter_mut().enumerate() {
        sec.order = idx;
    }

    if !dry_run {
        fs::write(&tree_path, serde_json::to_string_pretty(&tree)?)?;
        let media: MediaManifest = serde_json::from_str(&fs::read_to_string(
            workspace_dir.join("media").join("manifest.json"),
        )?)?;
        render_and_write_chapters(workspace_dir, &tree.sections, &media.items)?;
        append_structure_op(
            workspace_dir,
            "patch",
            serde_json::json!({
                "operations": ops_applied,
            }),
        )?;
        refresh_manifest_diagnostics(workspace_dir)?;
    }

    let summaries: Vec<SectionSummary> = tree.sections.iter().map(section_to_summary).collect();
    let untitled_count = summaries.iter().filter(|s| s.untitled).count();

    Ok(PatchStructureResponse {
        status: "ok".to_string(),
        dry_run,
        operations_applied: ops_applied,
        total_sections: summaries.len(),
        chapters_without_title: untitled_count,
        sections: summaries,
    })
}

pub fn inspect_media(workspace_path: &Path, filter: &str) -> Result<InspectMediaResponse> {
    let ws_dir = ensure_workspace_unpacked(workspace_path)?;
    let media_path = ws_dir.join("media").join("manifest.json");
    let media: MediaManifest = serde_json::from_str(&fs::read_to_string(&media_path)?)?;

    let kept_count = media.items.iter().filter(|i| i.status == "kept").count();
    let discarded_count = media.items.iter().filter(|i| i.status == "discarded").count();
    let uncertain_count = media.items.iter().filter(|i| i.status == "uncertain").count();

    let filters: HashSet<String> = filter
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let include_all = filters.is_empty() || filters.contains("all");

    let items: Vec<MediaItem> = media
        .items
        .into_iter()
        .filter(|item| include_all || filters.contains(&item.status.to_lowercase()))
        .collect();

    Ok(InspectMediaResponse {
        workspace: workspace_path.to_string_lossy().to_string(),
        filter: filter.to_string(),
        total_matching: items.len(),
        kept_count,
        discarded_count,
        uncertain_count,
        items,
    })
}

pub fn update_media(
    workspace_dir: &Path,
    keep: &[String],
    discard: &[String],
    alt: &[String],
    decorative: &[String],
    dry_run: bool,
) -> Result<UpdateMediaResponse> {
    let media_path = workspace_dir.join("media").join("manifest.json");
    let mut media: MediaManifest = serde_json::from_str(&fs::read_to_string(&media_path)?)?;
    let mut updated_ids: Vec<String> = Vec::new();

    for k in keep {
        for id in k.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let item = media
                .items
                .iter_mut()
                .find(|i| i.id.eq_ignore_ascii_case(id) || i.filename.eq_ignore_ascii_case(id))
                .ok_or_else(|| anyhow!("Media item '{}' not found", id))?;
            item.is_kept = true;
            item.status = "kept".to_string();
            if !updated_ids.contains(&item.id) {
                updated_ids.push(item.id.clone());
            }
        }
    }

    for d in discard {
        for id in d.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let item = media
                .items
                .iter_mut()
                .find(|i| i.id.eq_ignore_ascii_case(id) || i.filename.eq_ignore_ascii_case(id))
                .ok_or_else(|| anyhow!("Media item '{}' not found", id))?;
            item.is_kept = false;
            item.status = "discarded".to_string();
            if !updated_ids.contains(&item.id) {
                updated_ids.push(item.id.clone());
            }
        }
    }

    for a in alt {
        let (id, text) = a
            .split_once('=')
            .ok_or_else(|| anyhow!("Invalid --alt '{}'; expected id=text", a))?;
        let id = id.trim();
        let text = text.trim().trim_matches('"').to_string();
        let item = media
            .items
            .iter_mut()
            .find(|i| i.id.eq_ignore_ascii_case(id) || i.filename.eq_ignore_ascii_case(id))
            .ok_or_else(|| anyhow!("Media item '{}' not found", id))?;
        item.alt_text = text;
        item.decorative = false;
        if !updated_ids.contains(&item.id) {
            updated_ids.push(item.id.clone());
        }
    }

    for dec in decorative {
        for id in dec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let item = media
                .items
                .iter_mut()
                .find(|i| i.id.eq_ignore_ascii_case(id) || i.filename.eq_ignore_ascii_case(id))
                .ok_or_else(|| anyhow!("Media item '{}' not found", id))?;
            item.decorative = true;
            item.alt_text.clear();
            if !updated_ids.contains(&item.id) {
                updated_ids.push(item.id.clone());
            }
        }
    }

    if !dry_run {
        fs::write(&media_path, serde_json::to_string_pretty(&media)?)?;
        let tree: StructureTree = serde_json::from_str(&fs::read_to_string(
            workspace_dir.join("structure").join("tree.json"),
        )?)?;
        render_and_write_chapters(workspace_dir, &tree.sections, &media.items)?;
        refresh_manifest_diagnostics(workspace_dir)?;
    }

    let kept_count = media.items.iter().filter(|i| i.status == "kept").count();
    let discarded_count = media.items.iter().filter(|i| i.status == "discarded").count();
    let uncertain_count = media.items.iter().filter(|i| i.status == "uncertain").count();

    Ok(UpdateMediaResponse {
        status: "ok".to_string(),
        dry_run,
        updated_ids,
        kept_count,
        discarded_count,
        uncertain_count,
        items: media.items,
    })
}

pub fn audit_text(workspace_path: &Path, min_conf: f64, limit: usize) -> Result<AuditTextResponse> {
    let ws_dir = ensure_workspace_unpacked(workspace_path)?;
    let spell_path = ws_dir.join("rules").join("spell_audit.json");
    let spell: SpellAuditData = serde_json::from_str(&fs::read_to_string(&spell_path)?)?;

    let active: Vec<SpellCluster> = spell
        .clusters
        .into_iter()
        .filter(|c| !c.ignored && !c.applied && c.confidence >= min_conf)
        .collect();
    let total_active = active.len();
    let limited: Vec<SpellCluster> = active.into_iter().take(limit).collect();

    Ok(AuditTextResponse {
        workspace: workspace_path.to_string_lossy().to_string(),
        min_confidence: min_conf,
        limit,
        total_active_clusters: total_active,
        returned_clusters: limited.len(),
        ignored_japanese_count: spell.ignored_japanese_count,
        ignored_foreign_count: spell.ignored_foreign_count,
        ignored_proper_nouns_count: spell.ignored_proper_nouns_count,
        clusters: limited,
    })
}

pub fn glossary_ignore(
    workspace_dir: &Path,
    words: &[String],
    apply_spell: bool,
    min_conf: f64,
) -> Result<GlossaryIgnoreResponse> {
    let editorial_path = workspace_dir.join("rules").join("editorial.json");
    let mut editorial: EditorialRulesConfig =
        serde_json::from_str(&fs::read_to_string(&editorial_path)?)?;
    let spell_path = workspace_dir.join("rules").join("spell_audit.json");
    let mut spell: SpellAuditData = serde_json::from_str(&fs::read_to_string(&spell_path)?)?;

    let mut added_words = Vec::new();
    for raw in words {
        for w in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !editorial
                .ignored_words
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(w))
            {
                editorial.ignored_words.push(w.to_string());
                added_words.push(w.to_string());
            }
        }
    }

    let ignored_lower: HashSet<String> = editorial
        .ignored_words
        .iter()
        .map(|w| w.to_lowercase())
        .collect();

    let mut ignored_clusters_count = 0usize;
    for cl in &mut spell.clusters {
        if ignored_lower.contains(&cl.original_token.to_lowercase()) {
            cl.ignored = true;
        }
        if cl.ignored {
            ignored_clusters_count += 1;
        }
    }

    let mut applied_spell_clusters = 0usize;
    let mut modified_paragraphs = 0usize;

    if apply_spell {
        let tree_path = workspace_dir.join("structure").join("tree.json");
        let mut tree: StructureTree = serde_json::from_str(&fs::read_to_string(&tree_path)?)?;

        let to_apply: Vec<(String, String)> = spell
            .clusters
            .iter_mut()
            .filter(|c| !c.ignored && !c.applied && c.confidence >= min_conf)
            .map(|c| {
                c.applied = true;
                applied_spell_clusters += 1;
                (c.original_token.clone(), c.replacement_token.clone())
            })
            .collect();

        if !to_apply.is_empty() {
            for sec in &mut tree.sections {
                for p in &mut sec.paragraphs {
                    let mut changed = false;
                    for (orig, repl) in &to_apply {
                        if p.text.contains(orig) {
                            p.text = p.text.replace(orig, repl);
                            changed = true;
                        }
                    }
                    if changed {
                        modified_paragraphs += 1;
                    }
                }
                sec.word_count = count_words(&sec.paragraphs);
            }
            fs::write(&tree_path, serde_json::to_string_pretty(&tree)?)?;
            let media: MediaManifest = serde_json::from_str(&fs::read_to_string(
                workspace_dir.join("media").join("manifest.json"),
            )?)?;
            render_and_write_chapters(workspace_dir, &tree.sections, &media.items)?;
        }
    }

    fs::write(&editorial_path, serde_json::to_string_pretty(&editorial)?)?;
    fs::write(&spell_path, serde_json::to_string_pretty(&spell)?)?;
    refresh_manifest_diagnostics(workspace_dir)?;

    Ok(GlossaryIgnoreResponse {
        status: "ok".to_string(),
        added_words,
        total_ignored_words: editorial.ignored_words.len(),
        ignored_clusters_count,
        applied_spell_clusters,
        modified_paragraphs,
    })
}

pub fn rules_add(
    workspace_dir: &Path,
    find: &str,
    replace: &str,
    flags: &str,
    dry_run: bool,
) -> Result<RulesAddResponse> {
    if find.is_empty() {
        bail!("--find pattern must not be empty");
    }
    let case_insensitive = flags.contains('i');
    let multi_line = flags.contains('m');
    let rx = RegexBuilder::new(find)
        .case_insensitive(case_insensitive)
        .multi_line(multi_line)
        .build()
        .with_context(|| format!("Invalid regex pattern '{}'", find))?;

    let tree_path = workspace_dir.join("structure").join("tree.json");
    let mut tree: StructureTree = serde_json::from_str(&fs::read_to_string(&tree_path)?)?;
    let editorial_path = workspace_dir.join("rules").join("editorial.json");
    let mut editorial: EditorialRulesConfig =
        serde_json::from_str(&fs::read_to_string(&editorial_path)?)?;

    let mut affected_paragraphs = 0usize;
    let is_global = flags.contains('g') || flags.is_empty();

    for sec in &mut tree.sections {
        for p in &mut sec.paragraphs {
            if rx.is_match(&p.text) {
                affected_paragraphs += 1;
                if !dry_run {
                    if is_global {
                        p.text = rx.replace_all(&p.text, replace).to_string();
                    } else {
                        p.text = rx.replace(&p.text, replace).to_string();
                    }
                }
            }
        }
        if !dry_run {
            sec.word_count = count_words(&sec.paragraphs);
        }
    }

    let rule_id = format!("regex_{:03}", editorial.regex_rules.len() + 1);

    if !dry_run {
        editorial.regex_rules.push(FrontendRegexRule {
            id: rule_id.clone(),
            pattern: find.to_string(),
            replacement: replace.to_string(),
            enabled: true,
            flags: flags.to_string(),
            affected_count: affected_paragraphs,
        });
        fs::write(&editorial_path, serde_json::to_string_pretty(&editorial)?)?;
        fs::write(&tree_path, serde_json::to_string_pretty(&tree)?)?;
        let media: MediaManifest = serde_json::from_str(&fs::read_to_string(
            workspace_dir.join("media").join("manifest.json"),
        )?)?;
        render_and_write_chapters(workspace_dir, &tree.sections, &media.items)?;
        refresh_manifest_diagnostics(workspace_dir)?;
    }

    Ok(RulesAddResponse {
        status: "ok".to_string(),
        dry_run,
        rule_id,
        pattern: find.to_string(),
        replacement: replace.to_string(),
        flags: flags.to_string(),
        affected_paragraphs,
    })
}

pub fn build_workspace(
    workspace_dir: &Path,
    out_epub: &Path,
    export_project: Option<&Path>,
) -> Result<BuildResponse> {
    let manifest: WorkspaceManifest =
        serde_json::from_str(&fs::read_to_string(workspace_dir.join("manifest.json"))?)?;
    let tree: StructureTree = serde_json::from_str(&fs::read_to_string(
        workspace_dir.join("structure").join("tree.json"),
    )?)?;
    let media: MediaManifest = serde_json::from_str(&fs::read_to_string(
        workspace_dir.join("media").join("manifest.json"),
    )?)?;
    let editorial: EditorialRulesConfig = serde_json::from_str(&fs::read_to_string(
        workspace_dir.join("rules").join("editorial.json"),
    )?)?;

    let mut media_by_id: HashMap<String, (&MediaItem, Vec<u8>)> = HashMap::new();
    for item in &media.items {
        let bytes = fs::read(workspace_dir.join(&item.binary_path)).unwrap_or_default();
        media_by_id.insert(item.id.clone(), (item, bytes));
    }

    let mut builder = EpubBuilder::new();
    if !manifest.metadata.publisher.trim().is_empty() {
        builder.custom_publisher = Some(manifest.metadata.publisher.trim().to_string());
    }
    if !manifest.metadata.description.trim().is_empty() {
        builder.custom_description = Some(manifest.metadata.description.trim().to_string());
    }
    if !manifest.metadata.series.trim().is_empty() {
        builder.custom_series_title = Some(manifest.metadata.series.trim().to_string());
    }
    if !manifest.metadata.author.trim().is_empty() {
        builder.custom_author = Some(manifest.metadata.author.trim().to_string());
    }
    if !manifest.metadata.illustrator.trim().is_empty() {
        builder.custom_illustrator = Some(manifest.metadata.illustrator.trim().to_string());
    }

    let mut vol_numbers: Vec<usize> = tree.sections.iter().map(|s| s.volume_number).collect();
    for src in &manifest.sources {
        vol_numbers.push(src.volume_number);
    }
    vol_numbers.sort();
    vol_numbers.dedup();
    if vol_numbers.is_empty() {
        vol_numbers.push(1);
    }

    let mut volumes: Vec<VolumeInfo> = Vec::new();
    let is_omnibus = vol_numbers.len() > 1;

    for &vnum in &vol_numbers {
        let prefix = if is_omnibus {
            format!("v{:02}", vnum)
        } else {
            String::new()
        };

        let mut vol_sections = Vec::new();
        for wsec in tree.sections.iter().filter(|s| s.volume_number == vnum) {
            let mut eng_sec = to_engine_section(wsec, &media_by_id);
            for item in &mut eng_sec.content_items {
                if let ContentItemPayload::Paragraph(ref mut p) = item.payload {
                    p.text = apply_transforms_internal(
                        &p.text,
                        &editorial.editorial_rules,
                        &editorial.regex_rules,
                        &editorial.glossary,
                    );
                }
            }
            for p in &mut eng_sec.paragraphs {
                p.text = apply_transforms_internal(
                    &p.text,
                    &editorial.editorial_rules,
                    &editorial.regex_rules,
                    &editorial.glossary,
                );
            }
            let body_html = builder.render_section_body_html(&eng_sec, &prefix);
            let custom_body = apply_alt_and_decorative_to_xhtml(&body_html, &media.items);
            eng_sec.custom_html = Some(custom_body);
            vol_sections.push(eng_sec);
        }

        let mut cover_image: Option<ExtractedImage> = None;
        let mut color_images: Vec<ExtractedImage> = Vec::new();
        let mut all_verdicts: Vec<ExtractedImage> = Vec::new();

        for item in media.items.iter().filter(|i| i.volume_number == vnum) {
            let bytes = media_by_id
                .get(&item.id)
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
            let ext_img = ExtractedImage {
                page_num: item.page_num,
                image_index: item.image_index,
                ext: item.ext.clone(),
                data: bytes,
                width: item.width,
                height: item.height,
                filename: item.filename.clone(),
                category: item.category.clone(),
                y_pos: 0.0,
                is_full_page: item.is_full_page,
                is_cover: item.is_cover,
                is_kept: item.is_kept,
                reason: item.reason.clone(),
            };
            if item.is_kept {
                if item.is_cover && cover_image.is_none() {
                    cover_image = Some(ext_img.clone());
                } else if item.category == "color_inserts" {
                    color_images.push(ext_img.clone());
                }
            }
            all_verdicts.push(ext_img);
        }

        let src_entry = manifest
            .sources
            .iter()
            .find(|s| s.volume_number == vnum);
        let pdf_path = src_entry
            .map(|s| s.file_name.clone())
            .unwrap_or_else(|| format!("volume_{:02}.pdf", vnum));
        let total_pages = src_entry.map(|s| s.total_pages).unwrap_or(vol_sections.len().max(1));

        volumes.push(VolumeInfo {
            pdf_path,
            volume_number: vnum,
            title: manifest.metadata.title.clone(),
            total_pages,
            cover_image,
            color_images,
            sections: vol_sections,
            all_verdicts,
            spell_transformations: Vec::new(),
            publisher: if manifest.metadata.publisher.is_empty() {
                None
            } else {
                Some(manifest.metadata.publisher.clone())
            },
            description: if manifest.metadata.description.is_empty() {
                None
            } else {
                Some(manifest.metadata.description.clone())
            },
        });
    }

    let epub_bytes = if volumes.len() == 1 {
        builder
            .build_epub_bytes(&volumes[0])
            .map_err(|e| anyhow!(e))?
    } else {
        builder
            .build_omnibus_bytes(&volumes)
            .map_err(|e| anyhow!(e))?
    };

    if let Some(parent) = out_epub.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let epub_len = epub_bytes.len();
    fs::write(out_epub, epub_bytes)?;

    let (exported_project, exported_project_bytes) = if let Some(proj_path) = export_project {
        let sz = export_editnovel_archive(workspace_dir, proj_path)?;
        (Some(proj_path.to_string_lossy().to_string()), Some(sz))
    } else {
        (None, None)
    };

    let verification = verify_epub(out_epub)?;
    let images_embedded = media.items.iter().filter(|i| i.is_kept).count();

    Ok(BuildResponse {
        status: if verification.valid {
            "ok".to_string()
        } else {
            "error".to_string()
        },
        workspace: workspace_dir.to_string_lossy().to_string(),
        epub_path: out_epub.to_string_lossy().to_string(),
        epub_bytes: epub_len,
        exported_project,
        exported_project_bytes,
        sections_built: tree.sections.len(),
        images_embedded,
        verification,
    })
}

fn check_xml_well_formed(xml: &str, file_label: &str, errors: &mut Vec<String>) -> bool {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                stack.push(name);
            }
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_string();
                match stack.pop() {
                    Some(open) if open == name => {}
                    Some(open) => {
                        errors.push(format!(
                            "{}: mismatched closing tag </{}> for <{}>",
                            file_label, name, open
                        ));
                        return false;
                    }
                    None => {
                        errors.push(format!(
                            "{}: unexpected closing tag </{}>",
                            file_label, name
                        ));
                        return false;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(format!("{}: XML parse error: {}", file_label, e));
                return false;
            }
            _ => {}
        }
        buf.clear();
    }
    if !stack.is_empty() {
        errors.push(format!(
            "{}: unclosed XML tags remaining: {:?}",
            file_label, stack
        ));
        return false;
    }
    true
}

pub fn verify_epub(epub_path: &Path) -> Result<VerifyEpubResponse> {
    let bytes = fs::read(epub_path)
        .with_context(|| format!("Failed to read EPUB file at {:?}", epub_path))?;
    let file_size_bytes = bytes.len();
    let reader = Cursor::new(&bytes);
    let mut archive = ZipArchive::new(reader).context("Invalid ZIP container in EPUB")?;

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let mut ocf_mimetype_first_uncompressed = false;
    if archive.is_empty() {
        errors.push("EPUB archive is empty".to_string());
    } else {
        let mut first = archive.by_index(0)?;
        let first_name = first.name()?.to_string();
        let compression = first.compression();
        let mut content = Vec::new();
        first.read_to_end(&mut content)?;
        if first_name != "mimetype" {
            errors.push(format!(
                "First ZIP entry must be 'mimetype', found '{}'",
                first_name
            ));
        } else if compression != CompressionMethod::Stored {
            errors.push(format!(
                "'mimetype' entry must be uncompressed (Stored), found {:?}",
                compression
            ));
        } else if content != b"application/epub+zip" {
            errors.push("Invalid 'mimetype' content; expected 'application/epub+zip'".to_string());
        } else {
            ocf_mimetype_first_uncompressed = true;
        }
    }

    let mut archive_entries: HashSet<String> = HashSet::new();
    for i in 0..archive.len() {
        let f = archive.by_index(i)?;
        archive_entries.insert(f.name()?.to_string());
    }

    let mut container_xml_valid = false;
    let mut opf_path = "OEBPS/content.opf".to_string();
    if let Ok(mut container_file) = archive.by_name("META-INF/container.xml") {
        let mut container_xml = String::new();
        container_file.read_to_string(&mut container_xml)?;
        if check_xml_well_formed(&container_xml, "META-INF/container.xml", &mut errors) {
            let rootfile_re = Regex::new(r#"full-path="([^"]+)""#)?;
            if let Some(cap) = rootfile_re.captures(&container_xml) {
                opf_path = cap[1].to_string();
                if archive_entries.contains(&opf_path) {
                    container_xml_valid = true;
                } else {
                    errors.push(format!(
                        "container.xml rootfile '{}' does not exist in archive",
                        opf_path
                    ));
                }
            } else {
                errors.push("META-INF/container.xml is missing rootfile full-path".to_string());
            }
        }
    } else {
        errors.push("Missing META-INF/container.xml".to_string());
    }

    let mut opf_epub3_metadata_valid = false;
    let mut manifest_hrefs: HashSet<String> = HashSet::new();
    if let Ok(mut opf_file) = archive.by_name(&opf_path) {
        let mut opf_xml = String::new();
        opf_file.read_to_string(&mut opf_xml)?;
        if check_xml_well_formed(&opf_xml, &opf_path, &mut errors) {
            let has_v3 = opf_xml.contains("version=\"3.0\"");
            let has_id = opf_xml.contains("<dc:identifier");
            let has_title = opf_xml.contains("<dc:title>");
            let has_lang = opf_xml.contains("<dc:language>");
            let has_modified = opf_xml.contains("property=\"dcterms:modified\"");

            if !has_v3 {
                errors.push("OPF package is missing version=\"3.0\"".to_string());
            }
            if !has_id {
                errors.push("OPF metadata is missing <dc:identifier>".to_string());
            }
            if !has_title {
                errors.push("OPF metadata is missing <dc:title>".to_string());
            }
            if !has_lang {
                errors.push("OPF metadata is missing <dc:language>".to_string());
            }
            if !has_modified {
                errors.push("OPF metadata is missing dcterms:modified".to_string());
            }
            opf_epub3_metadata_valid = has_v3 && has_id && has_title && has_lang && has_modified;

            let item_href_re = Regex::new(r#"<item\b[^>]*href="([^"]+)""#)?;
            for cap in item_href_re.captures_iter(&opf_xml) {
                let href = &cap[1];
                let resolved = if opf_path.starts_with("OEBPS/") {
                    format!("OEBPS/{}", href)
                } else {
                    href.to_string()
                };
                manifest_hrefs.insert(resolved.clone());
                if !archive_entries.contains(&resolved) {
                    errors.push(format!(
                        "OPF manifest item '{}' is missing from ZIP archive",
                        resolved
                    ));
                    opf_epub3_metadata_valid = false;
                }
            }
        }
    } else {
        errors.push(format!("Missing OPF package file '{}'", opf_path));
    }

    let mut xhtml_well_formed = true;
    let mut xhtml_files_checked = 0usize;
    let mut image_references_valid = true;
    let mut image_references_checked = 0usize;

    let img_src_re = Regex::new(r#"<(?:img|image)\b[^>]*(?:src|href|xlink:href)="([^"]+)""#)?;

    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let name = f.name()?.to_string();
        if name.ends_with(".xhtml") || name.ends_with(".ncx") {
            let mut content = String::new();
            f.read_to_string(&mut content)?;
            xhtml_files_checked += 1;
            if !check_xml_well_formed(&content, &name, &mut errors) {
                xhtml_well_formed = false;
            }
            if name.ends_with(".xhtml") {
                for cap in img_src_re.captures_iter(&content) {
                    image_references_checked += 1;
                    let raw_src = &cap[1];
                    let resolved = if let Some(stripped) = raw_src.strip_prefix("../") {
                        format!("OEBPS/{}", stripped)
                    } else if name.starts_with("OEBPS/Text/") {
                        format!("OEBPS/Text/{}", raw_src)
                    } else if name.starts_with("OEBPS/") {
                        format!("OEBPS/{}", raw_src)
                    } else {
                        raw_src.to_string()
                    };
                    if !archive_entries.contains(&resolved) {
                        errors.push(format!(
                            "{}: referenced image '{}' (resolved '{}') does not exist in archive",
                            name, raw_src, resolved
                        ));
                        image_references_valid = false;
                    } else if !manifest_hrefs.contains(&resolved) {
                        warnings.push(format!(
                            "{}: referenced image '{}' is not listed in OPF manifest",
                            name, resolved
                        ));
                    }
                }
            }
        }
    }

    if xhtml_files_checked == 0 {
        errors.push("No XHTML content documents found in EPUB".to_string());
        xhtml_well_formed = false;
    }

    let valid = ocf_mimetype_first_uncompressed
        && container_xml_valid
        && opf_epub3_metadata_valid
        && xhtml_well_formed
        && image_references_valid
        && errors.is_empty();

    Ok(VerifyEpubResponse {
        valid,
        epub_path: epub_path.to_string_lossy().to_string(),
        file_size_bytes,
        checks: EpubVerificationChecks {
            ocf_mimetype_first_uncompressed,
            container_xml_valid,
            opf_epub3_metadata_valid,
            xhtml_well_formed,
            image_references_valid,
        },
        xhtml_files_checked,
        image_references_checked,
        errors,
        warnings,
    })
}
