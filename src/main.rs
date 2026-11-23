#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    cli::run();
}

#[cfg(not(target_arch = "wasm32"))]
mod cli {
use editnovel_engine::epub_builder::EpubBuilder;
use editnovel_engine::models::{FrontendSpellCorrection, VolumeInfo};
use editnovel_engine::spell_corrector::{ContextualSpellCorrector, SpellAuditOptions, SpellAuditStats};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BaselineParagraph {
    text: String,
    style: String,
    is_scene_break: bool,
    is_part_heading: bool,
    fn_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BaselineSection {
    title: String,
    start_page: usize,
    end_page: usize,
    paragraphs: Vec<BaselineParagraph>,
    footnotes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BaselineImage {
    page_num: usize,
    category: String,
    width: u32,
    height: u32,
    is_cover: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BaselineVolume {
    volume_number: usize,
    total_pages: usize,
    kept_images: Vec<BaselineImage>,
    discarded_count: usize,
    sections: Vec<BaselineSection>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BenchRecord {
    timestamp: u64,
    volumes: usize,
    pages: usize,
    ingest_ms: u128,
    pack_ms: u128,
    total_ms: u128,
    epub_bytes: usize,
    #[serde(default)]
    wasm_bytes: Option<usize>,
}

struct CliArgs {
    pdf_dir: PathBuf,
    volume_filter: Option<usize>,
    page: usize,
    limit: usize,
    category_filter: Option<String>,
    min_confidence: f64,
    json_export: Option<PathBuf>,
    summary_only: bool,
    capture_baseline: Option<PathBuf>,
    diff_baseline: Option<PathBuf>,
    bench: bool,
    bench_all: bool,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = env::args().collect();
    let mut pdf_dir = PathBuf::from(r"C:\Users\tkdgi\Workspace\scripts\leer\pdfs");
    let mut volume_filter = None;
    let mut page = 1;
    let mut limit = 20;
    let mut category_filter = None;
    let mut min_confidence = 0.80;
    let mut json_export = None;
    let mut summary_only = false;
    let mut capture_baseline = None;
    let mut diff_baseline = None;
    let mut bench = false;
    let mut bench_all = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" | "-d" => {
                if i + 1 < args.len() {
                    pdf_dir = PathBuf::from(&args[i + 1]);
                    i += 1;
                }
            }
            "--volumen" | "--volume" | "-v" => {
                if i + 1 < args.len() {
                    volume_filter = args[i + 1].parse::<usize>().ok();
                    i += 1;
                }
            }
            "--page" | "-p" => {
                if i + 1 < args.len() {
                    page = args[i + 1].parse::<usize>().unwrap_or(1).max(1);
                    i += 1;
                }
            }
            "--limit" | "-l" => {
                if i + 1 < args.len() {
                    limit = args[i + 1].parse::<usize>().unwrap_or(20).max(1);
                    i += 1;
                }
            }
            "--cat" | "--category" | "-c" => {
                if i + 1 < args.len() {
                    let cat = args[i + 1].to_lowercase();
                    if cat != "all" {
                        category_filter = Some(cat);
                    }
                    i += 1;
                }
            }
            "--min-conf" => {
                if i + 1 < args.len() {
                    min_confidence = args[i + 1].parse::<f64>().unwrap_or(0.80);
                    i += 1;
                }
            }
            "--json" => {
                if i + 1 < args.len() {
                    json_export = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--summary" | "-s" => {
                summary_only = true;
            }
            "--capture-baseline" => {
                if i + 1 < args.len() {
                    capture_baseline = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--diff-baseline" => {
                if i + 1 < args.len() {
                    diff_baseline = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--bench" => {
                bench = true;
            }
            "--all" => {
                bench_all = true;
            }
            _ => {}
        }
        i += 1;
    }

    CliArgs {
        pdf_dir,
        volume_filter,
        page,
        limit,
        category_filter,
        min_confidence,
        json_export,
        summary_only,
        capture_baseline,
        diff_baseline,
        bench,
        bench_all,
    }
}

fn extract_volume_number(filename: &str) -> usize {
    editnovel_engine::config::infer_series_and_volume(filename).1
}

fn format_diff_snippet(full_para: &str, orig: &str, repl: &str) -> String {
    let lower_para = full_para.to_lowercase();
    let lower_orig = orig.to_lowercase();
    if let Some(pos) = lower_para.find(&lower_orig) {
        let before_text = &full_para[..pos];
        let after_text = &full_para[(pos + orig.len()).min(full_para.len())..];

        let before_words: Vec<&str> = before_text.split_whitespace().collect();
        let after_words: Vec<&str> = after_text.split_whitespace().collect();

        let before_start = before_words.len().saturating_sub(5);
        let before_snippet = before_words[before_start..].join(" ");
        let after_snippet = after_words[..after_words.len().min(5)].join(" ");

        let prefix = if before_start > 0 { "... " } else { "" };
        let suffix = if after_words.len() > 5 { " ..." } else { "" };

        format!(
            "{}{} >>> {} -> {} <<< {}{}",
            prefix, before_snippet, orig, repl, after_snippet, suffix
        )
    } else {
        format!(">>> {} -> {} <<<", orig, repl)
    }
}

pub fn run() {
    let args = parse_args();

    if !args.pdf_dir.exists() {
        eprintln!("Error: PDF directory does not exist: {:?}", args.pdf_dir);
        std::process::exit(1);
    }

    let mut pdf_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&args.pdf_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext.eq_ignore_ascii_case("pdf")) {
                let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let vol_num = extract_volume_number(fname);
                if let Some(v) = args.volume_filter {
                    if vol_num != v {
                        continue;
                    }
                }
                pdf_files.push(path);
            }
        }
    }

    pdf_files.sort_by_key(|p| {
        let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        extract_volume_number(fname)
    });

    if pdf_files.is_empty() {
        println!("No PDF volumes found matching criteria in {:?}", args.pdf_dir);
        return;
    }

    if let Some(ref out_path) = args.capture_baseline {
        run_capture_baseline(&pdf_files, out_path);
        return;
    }

    if let Some(ref ref_path) = args.diff_baseline {
        run_diff_baseline(&pdf_files, ref_path, args.limit.min(8));
        return;
    }

    if args.bench {
        if !args.bench_all && args.volume_filter.is_none() {
            pdf_files.truncate(1);
        }
        run_benchmark(&pdf_files, &PathBuf::from("bench_history.json"));
        return;
    }

    println!("=== Audit Spell Checker CLI ===");
    println!("Directory: {:?}", args.pdf_dir);
    println!("Volumes to audit: {}", pdf_files.len());
    if let Some(v) = args.volume_filter {
        println!("Filter: Volume {}", v);
    }
    if let Some(ref c) = args.category_filter {
        println!("Category filter: {}", c);
    }
    println!("Min confidence: {:.0}%", args.min_confidence * 100.0);
    println!("================================\n");

    let spell_corrector = ContextualSpellCorrector::default();
    let audit_opts = SpellAuditOptions::default();
    let mut audit_stats = SpellAuditStats::default();

    let mut all_corrections: Vec<FrontendSpellCorrection> = Vec::new();
    let mut volume_counts: HashMap<usize, usize> = HashMap::new();
    let mut category_counts: HashMap<String, usize> = HashMap::new();

    for pdf_path in &pdf_files {
        let fname = pdf_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let vol_num = extract_volume_number(fname);

        let pdf_bytes = match fs::read(pdf_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Warning: failed to read {:?}: {}", pdf_path, e);
                continue;
            }
        };

        let mut builder = EpubBuilder::new();
        let vol_info = match builder.process_pdf_bytes(&pdf_bytes, fname, false) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Warning: failed to parse {:?}: {}", pdf_path, e);
                continue;
            }
        };

        let mut vol_corrections_count = 0;

        for sec in &vol_info.sections {
            let mut paras_clone = sec.paragraphs.clone();
            let (corrections, _) = spell_corrector.audit_section_paragraphs(
                fname,
                &mut paras_clone,
                vol_num,
                sec.start_page,
                &sec.title,
                &audit_opts,
                &mut audit_stats,
                false,
                all_corrections.len() + 1,
            );

            for c in corrections {
                if c.confidence < args.min_confidence {
                    continue;
                }
                if let Some(ref cat) = args.category_filter {
                    if !c.category.eq_ignore_ascii_case(cat) {
                        continue;
                    }
                }

                *category_counts.entry(c.category.clone()).or_insert(0) += 1;
                vol_corrections_count += 1;
                all_corrections.push(c);
            }
        }

        volume_counts.insert(vol_num, vol_corrections_count);
    }

    if let Some(ref json_path) = args.json_export {
        if let Ok(json) = serde_json::to_string_pretty(&all_corrections) {
            let _ = fs::write(json_path, json);
            println!("Exported {} corrections to {:?}", all_corrections.len(), json_path);
        }
    }

    if args.summary_only {
        println!("=== AUDIT SUMMARY ===");
        println!("Total corrections found: {}", all_corrections.len());
        println!("\nPer Category:");
        for (cat, count) in &category_counts {
            println!("  - {:<12}: {:>4}", cat, count);
        }
        println!("\nPer Volume:");
        let mut sorted_vols: Vec<_> = volume_counts.iter().collect();
        sorted_vols.sort_by_key(|(v, _)| *v);
        for (v, count) in sorted_vols {
            println!("  - Volume {:02}: {:>4} issues", v, count);
        }
        println!("\nIgnored Stats:");
        println!("  - Japanese ignored     : {}", audit_stats.ignored_japanese_count);
        println!("  - Foreign/fantasy ignored: {}", audit_stats.ignored_foreign_count);
        println!("  - Proper nouns ignored : {}", audit_stats.ignored_proper_nouns_count);
        return;
    }

    let total = all_corrections.len();
    let total_pages = if total == 0 { 1 } else { (total + args.limit - 1) / args.limit };
    let current_page = args.page.min(total_pages);
    let start_idx = (current_page - 1) * args.limit;
    let end_idx = (start_idx + args.limit).min(total);

    println!(
        "--- Showing issues {}-{} of {} (Page {}/{}) ---\n",
        if total == 0 { 0 } else { start_idx + 1 },
        end_idx,
        total,
        current_page,
        total_pages
    );

    for (idx, c) in all_corrections[start_idx..end_idx].iter().enumerate() {
        let global_idx = start_idx + idx + 1;
        let diff = format_diff_snippet(&c.paragraph_before, &c.original_token, &c.replacement_token);
        let conf_pct = (c.confidence * 100.0).round() as usize;

        let cat_label = match c.category.as_str() {
            "typo" => "\x1b[31mTYPO   \x1b[0m",
            "grammar" => "\x1b[34mGRAMMAR\x1b[0m",
            "punctuation" => "\x1b[33mPUNCT  \x1b[0m",
            "casing" => "\x1b[35mCASING \x1b[0m",
            _ => "\x1b[36mSTYLE  \x1b[0m",
        };

        println!(
            "[#{:04} | V{:02}:P{:03} | {} | {:>3}%] {}",
            global_idx, c.volume_num, c.page_num, cat_label, conf_pct, diff
        );
    }

    println!("\n---------------------------------------------------------");
    println!(
        "Page {}/{} (Total: {}). Next: run with '--page {}'",
        current_page,
        total_pages,
        total,
        (current_page + 1).min(total_pages)
    );
    println!("Tip: filter with '--volumen <N>' or '--cat <typo|grammar|punctuation|casing>' or '--summary'");
}

fn volume_to_baseline(vol: &VolumeInfo) -> BaselineVolume {
    let mut kept_images = Vec::new();
    let mut discarded_count = 0usize;
    for img in &vol.all_verdicts {
        if img.is_kept {
            kept_images.push(BaselineImage {
                page_num: img.page_num,
                category: img.category.clone(),
                width: img.width,
                height: img.height,
                is_cover: img.is_cover,
            });
        } else {
            discarded_count += 1;
        }
    }

    let sections = vol
        .sections
        .iter()
        .map(|s| BaselineSection {
            title: s.title.clone(),
            start_page: s.start_page,
            end_page: s.end_page,
            paragraphs: s
                .paragraphs
                .iter()
                .map(|p| BaselineParagraph {
                    text: p.text.clone(),
                    style: p.style.clone(),
                    is_scene_break: p.is_scene_break,
                    is_part_heading: p.is_part_heading,
                    fn_count: p.footnotes.len(),
                })
                .collect(),
            footnotes: s.footnotes.iter().map(|f| f.text.clone()).collect(),
        })
        .collect();

    BaselineVolume {
        volume_number: vol.volume_number,
        total_pages: vol.total_pages,
        kept_images,
        discarded_count,
        sections,
    }
}

fn has_unbalanced_delimiters(text: &str) -> bool {
    let pairs = [
        ('«', '»'),
        ('《', '》'),
        ('“', '”'),
        ('『', '』'),
        ('「', '」'),
    ];
    for (o, c) in pairs {
        if text.matches(o).count() != text.matches(c).count() {
            return true;
        }
    }
    false
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(max_chars).collect();
        format!("{}…", prefix)
    }
}

fn format_delta(prev: u128, curr: u128) -> String {
    if prev == 0 {
        return "-".to_string();
    }
    let diff = (curr as f64) - (prev as f64);
    let pct = (diff / (prev as f64)) * 100.0;
    if pct > 0.0 {
        format!("+{:>5.1}%", pct)
    } else {
        format!("{:>6.1}%", pct)
    }
}

fn format_size(bytes: usize) -> String {
    let mb = (bytes as f64) / (1024.0 * 1024.0);
    format!("{:.2} MB", mb)
}

fn get_wasm_bundle_size() -> Option<usize> {
    let candidates = [
        PathBuf::from("../frontend/src/wasm/engine_bg.wasm"),
        PathBuf::from("frontend/src/wasm/engine_bg.wasm"),
        PathBuf::from("../../frontend/src/wasm/engine_bg.wasm"),
    ];
    for p in &candidates {
        if let Ok(meta) = fs::metadata(p) {
            return Some(meta.len() as usize);
        }
    }
    None
}

fn print_bench_table(
    prev: Option<&BenchRecord>,
    ingest_ms: u128,
    pack_ms: u128,
    total_ms: u128,
    curr_wasm_bytes: Option<usize>,
) {
    let prev_ingest = prev.map_or("-".to_string(), |p| format!("{} ms", p.ingest_ms));
    let prev_pack = prev.map_or("-".to_string(), |p| format!("{} ms", p.pack_ms));
    let prev_total = prev.map_or("-".to_string(), |p| format!("{} ms", p.total_ms));

    let delta_ingest = prev.map_or("-".to_string(), |p| format_delta(p.ingest_ms, ingest_ms));
    let delta_pack = prev.map_or("-".to_string(), |p| format_delta(p.pack_ms, pack_ms));
    let delta_total = prev.map_or("-".to_string(), |p| format_delta(p.total_ms, total_ms));

    let prev_wasm = prev.and_then(|p| p.wasm_bytes).or(Some(8_735_400));
    let prev_wasm_str = prev_wasm.map_or("-".to_string(), format_size);
    let curr_wasm_str = curr_wasm_bytes.map_or("-".to_string(), format_size);
    let delta_wasm = match (prev_wasm, curr_wasm_bytes) {
        (Some(p), Some(c)) => format_delta(p as u128, c as u128),
        _ => "-".to_string(),
    };

    println!("CONCEPTO     |      ANTERIOR |        ACTUAL | DELTA");
    println!("-------------+---------------+---------------+-------");
    println!("Ingesta      | {:>13} | {:>11} ms | {:>5}", prev_ingest, ingest_ms, delta_ingest);
    println!("Empaquetado  | {:>13} | {:>11} ms | {:>5}", prev_pack, pack_ms, delta_pack);
    println!("Total        | {:>13} | {:>11} ms | {:>5}", prev_total, total_ms, delta_total);
    println!("WASM Bundle  | {:>13} | {:>14} | {:>5}", prev_wasm_str, curr_wasm_str, delta_wasm);
}

fn run_benchmark(pdf_files: &[PathBuf], history_path: &PathBuf) {
    let mut history: Vec<BenchRecord> = if history_path.exists() {
        fs::read_to_string(history_path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let prev = history
        .iter()
        .rev()
        .find(|r| r.volumes == pdf_files.len())
        .cloned();

    let mut total_pages = 0usize;
    let mut total_epub_bytes = 0usize;

    let t0 = std::time::Instant::now();
    let mut volumes: Vec<VolumeInfo> = Vec::with_capacity(pdf_files.len());
    for pdf_path in pdf_files {
        let fname = pdf_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let bytes = fs::read(pdf_path).expect("Failed to read PDF");
        let mut builder = EpubBuilder::new();
        let vol = builder
            .process_pdf_bytes(&bytes, fname, true)
            .expect("Failed to process PDF");
        total_pages += vol.total_pages;
        volumes.push(vol);
    }
    let ingest_ms = t0.elapsed().as_millis();

    let t1 = std::time::Instant::now();
    let builder = EpubBuilder::new();
    for vol in &volumes {
        let epub_bytes = builder.build_epub_bytes(vol).expect("Failed to build EPUB");
        total_epub_bytes += epub_bytes.len();
    }
    let pack_ms = t1.elapsed().as_millis();
    let total_ms = ingest_ms + pack_ms;

    let wasm_bytes = get_wasm_bundle_size();
    print_bench_table(prev.as_ref(), ingest_ms, pack_ms, total_ms, wasm_bytes);

    let now_sec = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let record = BenchRecord {
        timestamp: now_sec,
        volumes: pdf_files.len(),
        pages: total_pages,
        ingest_ms,
        pack_ms,
        total_ms,
        epub_bytes: total_epub_bytes,
        wasm_bytes,
    };
    history.push(record);

    if let Some(parent) = history_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&history) {
        let _ = fs::write(history_path, json);
    }
}

fn run_capture_baseline(pdf_files: &[PathBuf], out_path: &PathBuf) {
    let mut baseline_vols: Vec<BaselineVolume> = pdf_files
        .par_iter()
        .filter_map(|pdf_path| {
            let fname = pdf_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let bytes = fs::read(pdf_path).ok()?;
            let mut builder = EpubBuilder::new();
            let vol = builder.process_pdf_bytes(&bytes, fname, true).ok()?;
            Some(volume_to_baseline(&vol))
        })
        .collect();

    baseline_vols.sort_by_key(|v| v.volume_number);

    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string(&baseline_vols).expect("Failed to serialize baseline");
    fs::write(out_path, json).expect("Failed to write baseline file");
    let total_secs: usize = baseline_vols.iter().map(|v| v.sections.len()).sum();
    let total_paras: usize = baseline_vols
        .iter()
        .flat_map(|v| &v.sections)
        .map(|s| s.paragraphs.len())
        .sum();
    let total_imgs: usize = baseline_vols.iter().map(|v| v.kept_images.len()).sum();
    println!(
        "CAPTURED BASELINE: {} vols | {} secs | {} paras | {} kept imgs -> {:?}",
        baseline_vols.len(),
        total_secs,
        total_paras,
        total_imgs,
        out_path
    );
}

fn run_diff_baseline(pdf_files: &[PathBuf], ref_path: &PathBuf, sample_limit: usize) {
    let ref_data = fs::read_to_string(ref_path).expect("Failed to read baseline file");
    let ref_vols: Vec<BaselineVolume> =
        serde_json::from_str(&ref_data).expect("Failed to parse baseline JSON");
    let ref_map: HashMap<usize, BaselineVolume> =
        ref_vols.into_iter().map(|v| (v.volume_number, v)).collect();

    let mut got_vols: Vec<BaselineVolume> = pdf_files
        .par_iter()
        .filter_map(|pdf_path| {
            let fname = pdf_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let bytes = fs::read(pdf_path).ok()?;
            let mut builder = EpubBuilder::new();
            let vol = builder.process_pdf_bytes(&bytes, fname, true).ok()?;
            Some(volume_to_baseline(&vol))
        })
        .collect();
    got_vols.sort_by_key(|v| v.volume_number);

    let mut sec_bound_diffs = Vec::new();
    let mut title_diffs = Vec::new();
    let mut para_cnt_diffs = Vec::new();
    let mut style_diffs = Vec::new();
    let mut text_diffs = Vec::new();
    let mut fn_diffs = Vec::new();
    let mut img_diffs = Vec::new();
    let mut total_unbal_got = 0usize;
    let mut total_unbal_ref = 0usize;

    println!("VOL | SEC(G/R) | TD | PARA(G/R)   | SD  | TXD  | UNB(G/R) | FN(G/R) | IMG(G/R)");
    println!("----+----------+----+-------------+-----+------+----------+---------+---------");

    for got in &got_vols {
        let Some(ref_v) = ref_map.get(&got.volume_number) else {
            continue;
        };
        let vnum = got.volume_number;

        let got_img_set: BTreeSet<(usize, String, u32, u32)> = got
            .kept_images
            .iter()
            .map(|i| (i.page_num, i.category.clone(), i.width, i.height))
            .collect();
        let ref_img_set: BTreeSet<(usize, String, u32, u32)> = ref_v
            .kept_images
            .iter()
            .map(|i| (i.page_num, i.category.clone(), i.width, i.height))
            .collect();
        if got_img_set != ref_img_set {
            let miss: Vec<_> = ref_img_set.difference(&got_img_set).collect();
            let extra: Vec<_> = got_img_set.difference(&ref_img_set).collect();
            img_diffs.push(format!("V{:02}: miss={:?} extra={:?}", vnum, miss, extra));
        }

        if got.sections.len() != ref_v.sections.len() {
            sec_bound_diffs.push(format!(
                "V{:02}: sec_count got={} ref={}",
                vnum,
                got.sections.len(),
                ref_v.sections.len()
            ));
        }

        let mut v_td = 0usize;
        let mut v_sd = 0usize;
        let mut v_txd = 0usize;
        let mut v_unb_g = 0usize;
        let mut v_unb_r = 0usize;
        let mut v_para_g = 0usize;
        let mut v_para_r = 0usize;
        let mut v_fn_g = 0usize;
        let mut v_fn_r = 0usize;

        for s in &got.sections {
            v_para_g += s.paragraphs.len();
            v_fn_g += s.footnotes.len();
            for p in &s.paragraphs {
                if has_unbalanced_delimiters(&p.text) {
                    v_unb_g += 1;
                }
            }
        }
        for s in &ref_v.sections {
            v_para_r += s.paragraphs.len();
            v_fn_r += s.footnotes.len();
            for p in &s.paragraphs {
                if has_unbalanced_delimiters(&p.text) {
                    v_unb_r += 1;
                }
            }
        }
        total_unbal_got += v_unb_g;
        total_unbal_ref += v_unb_r;

        let min_secs = got.sections.len().min(ref_v.sections.len());
        for s_idx in 0..min_secs {
            let gs = &got.sections[s_idx];
            let rs = &ref_v.sections[s_idx];

            if gs.start_page != rs.start_page || gs.end_page != rs.end_page {
                sec_bound_diffs.push(format!(
                    "V{:02}:S{:02} pages got={}..{} ref={}..{} ({})",
                    vnum, s_idx + 1, gs.start_page, gs.end_page, rs.start_page, rs.end_page, rs.title
                ));
            }
            if gs.title != rs.title {
                v_td += 1;
                title_diffs.push(format!(
                    "V{:02}:S{:02} got='{}' | ref='{}'",
                    vnum, s_idx + 1, gs.title, rs.title
                ));
            }
            if gs.footnotes != rs.footnotes {
                fn_diffs.push(format!(
                    "V{:02}:S{:02} fn got={} ref={}",
                    vnum,
                    s_idx + 1,
                    gs.footnotes.len(),
                    rs.footnotes.len()
                ));
            }
            if gs.paragraphs.len() != rs.paragraphs.len() {
                para_cnt_diffs.push(format!(
                    "V{:02}:S{:02} ({}) paras got={} ref={}",
                    vnum,
                    s_idx + 1,
                    truncate_str(&rs.title, 22),
                    gs.paragraphs.len(),
                    rs.paragraphs.len()
                ));
            }

            let min_p = gs.paragraphs.len().min(rs.paragraphs.len());
            for p_idx in 0..min_p {
                let gp = &gs.paragraphs[p_idx];
                let rp = &rs.paragraphs[p_idx];
                if gp.style != rp.style {
                    v_sd += 1;
                    style_diffs.push(format!(
                        "V{:02}:S{:02}:P{:03} style got={} ref={} '{}'",
                        vnum,
                        s_idx + 1,
                        p_idx + 1,
                        gp.style,
                        rp.style,
                        truncate_str(&gp.text, 40)
                    ));
                }
                if gp.text != rp.text {
                    v_txd += 1;
                    text_diffs.push(format!(
                        "V{:02}:S{:02}:P{:03} G='{}' | R='{}'",
                        vnum,
                        s_idx + 1,
                        p_idx + 1,
                        truncate_str(&gp.text, 48),
                        truncate_str(&rp.text, 48)
                    ));
                }
            }
        }

        println!(
            "V{:02} | {:>3}/{:<3}  | {:>2} | {:>5}/{:<5} | {:>3} | {:>4} | {:>3}/{:<3}  | {:>3}/{:<3} | {:>2}/{:<2}",
            vnum,
            got.sections.len(),
            ref_v.sections.len(),
            v_td,
            v_para_g,
            v_para_r,
            v_sd,
            v_txd,
            v_unb_g,
            v_unb_r,
            v_fn_g,
            v_fn_r,
            got.kept_images.len(),
            ref_v.kept_images.len()
        );
    }

    println!(
        "\nSUMMARY: SecBound={} | Title={} | ParaCnt={} | Style={} | Text={} | FN={} | Img={} | Unbal(G/R)={}/{}",
        sec_bound_diffs.len(),
        title_diffs.len(),
        para_cnt_diffs.len(),
        style_diffs.len(),
        text_diffs.len(),
        fn_diffs.len(),
        img_diffs.len(),
        total_unbal_got,
        total_unbal_ref
    );

    let groups = [
        ("SEC_BOUND", &sec_bound_diffs),
        ("TITLE", &title_diffs),
        ("PARA_CNT", &para_cnt_diffs),
        ("STYLE", &style_diffs),
        ("TEXT", &text_diffs),
        ("FN", &fn_diffs),
        ("IMG", &img_diffs),
    ];
    for (name, list) in groups {
        if !list.is_empty() {
            println!("\n[{}] (showing {}/{})", name, list.len().min(sample_limit), list.len());
            for item in list.iter().take(sample_limit) {
                println!("  {}", item);
            }
        }
    }
}
}
