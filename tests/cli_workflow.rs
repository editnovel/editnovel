use image::{ImageFormat, Rgba, RgbaImage};
use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};
use serde_json::Value;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn editnovel_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_editnovel"))
}

fn run_cmd(args: &[&str]) -> Value {
    let output = Command::new(editnovel_bin())
        .args(args)
        .output()
        .expect("Failed to execute editnovel binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Command {:?} failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
        args,
        stdout,
        stderr
    );
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON from command {:?}: {}\nOutput:\n{}",
            args, e, stdout
        )
    })
}

fn make_png_bytes(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(width, height, Rgba(color));
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, ImageFormat::Png)
        .expect("Failed to encode PNG");
    cursor.into_inner()
}

fn create_synthetic_epub(path: &Path) {
    let cover_png = make_png_bytes(400, 600, [40, 80, 160, 255]);
    let stamp_png = make_png_bytes(100, 100, [200, 50, 50, 255]);

    let file = fs::File::create(path).expect("Failed to create synthetic EPUB");
    let mut zw = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zw.start_file("mimetype", stored).unwrap();
    zw.write_all(b"application/epub+zip").unwrap();

    let container_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
    zw.start_file("META-INF/container.xml", deflated).unwrap();
    zw.write_all(container_xml.as_bytes()).unwrap();

    let opf_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id" xml:lang="en">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:uuid:12345678-1234-5678-1234-567812345678</dc:identifier>
    <dc:title>Synthetic Chronicles - Volume 01</dc:title>
    <dc:language>en</dc:language>
    <dc:publisher>EditNovel Test Press</dc:publisher>
    <dc:description>A synthetic light novel volume generated for automated end-to-end testing.</dc:description>
    <meta property="dcterms:modified">2026-09-12T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="sec1" href="Text/ch01.xhtml" media-type="application/xhtml+xml"/>
    <item id="sec2" href="Text/ch02.xhtml" media-type="application/xhtml+xml"/>
    <item id="sec3" href="Text/ch03.xhtml" media-type="application/xhtml+xml"/>
    <item id="img1" href="Images/cover_p001.png" media-type="image/png"/>
    <item id="img2" href="Images/stamp_icon.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="sec1"/>
    <itemref idref="sec2"/>
    <itemref idref="sec3"/>
  </spine>
</package>"#;
    zw.start_file("OEBPS/content.opf", deflated).unwrap();
    zw.write_all(opf_xml.as_bytes()).unwrap();

    zw.start_file("OEBPS/Images/cover_p001.png", stored).unwrap();
    zw.write_all(&cover_png).unwrap();

    zw.start_file("OEBPS/Images/stamp_icon.png", stored).unwrap();
    zw.write_all(&stamp_png).unwrap();

    let ch01 = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Untitled</title></head>
<body>
  <h1>Untitled</h1>
  <p>Rimuru and Veldora stepped into the cavern -- the air was cold and silent.</p>
  <p class="dialogue">—Welcome to the labyrinth, traveller.</p>
  <p>The crystal glowed brightly in the darkness as they prepared their magic.</p>
</body>
</html>"#;
    zw.start_file("OEBPS/Text/ch01.xhtml", deflated).unwrap();
    zw.write_all(ch01.as_bytes()).unwrap();

    let ch02 = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
  <h1>Chapter 1</h1>
  <p>The first half of the chapter begins on the northern ridge.</p>
  <p>Snow covered the ancient ruins from centuries ago.</p>
</body>
</html>"#;
    zw.start_file("OEBPS/Text/ch02.xhtml", deflated).unwrap();
    zw.write_all(ch02.as_bytes()).unwrap();

    let ch03 = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1 Continuation</title></head>
<body>
  <h1>Chapter 1 Continuation</h1>
  <p>The second half of the chapter continues after the page break.</p>
  <p>They reached the summit before dawn.</p>
</body>
</html>"#;
    zw.start_file("OEBPS/Text/ch03.xhtml", deflated).unwrap();
    zw.write_all(ch03.as_bytes()).unwrap();

    zw.finish().unwrap();
}

fn create_synthetic_pdf(path: &Path) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });

    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 18.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
            Operation::new("Tj", vec![Object::string_literal("Capítulo 1")]),
            Operation::new("ET", vec![]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 11.into()]),
            Operation::new("Td", vec![72.into(), 680.into()]),
            Operation::new(
                "Tj",
                vec![Object::string_literal(
                    "El caballero caminaba por el bosque silencioso al amanecer.",
                )],
            ),
            Operation::new("ET", vec![]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 11.into()]),
            Operation::new("Td", vec![72.into(), 650.into()]),
            Operation::new(
                "Tj",
                vec![Object::string_literal(
                    "—Debemos continuar nuestro viaje hacia la capital.",
                )],
            ),
            Operation::new("ET", vec![]),
        ],
    };

    let content_stream = Stream::new(dictionary! {}, content.encode().unwrap());
    let content_id = doc.add_object(content_stream);

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).expect("Failed to save synthetic PDF");
}

#[test]
fn test_full_5_stage_cli_lifecycle_and_editnovel_archive() {
    let temp_root = std::env::temp_dir().join(format!(
        "editnovel_e2e_test_{}",
        std::process::id()
    ));
    if temp_root.exists() {
        let _ = fs::remove_dir_all(&temp_root);
    }
    fs::create_dir_all(&temp_root).unwrap();

    let epub_input = temp_root.join("Synthetic_Vol1.epub");
    create_synthetic_epub(&epub_input);

    let ws_dir = temp_root.join("work_epub");
    let out_epub = temp_root.join("output").join("final_book.epub");
    let out_proj = temp_root.join("output").join("final_book.editnovel");

    let ingest_json = run_cmd(&[
        "ingest",
        epub_input.to_str().unwrap(),
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--profile",
        "en-light-novel",
        "--lang",
        "en",
        "--json",
    ]);

    assert_eq!(ingest_json["status"], "ok");
    assert_eq!(ingest_json["profile"], "en-light-novel");
    assert_eq!(ingest_json["language"], "en");
    assert_eq!(ingest_json["diagnostics"]["has_text_layer"], true);
    assert_eq!(ingest_json["diagnostics"]["total_sections"], 3);
    assert_eq!(ingest_json["diagnostics"]["chapters_without_title"], 1);
    assert_eq!(ingest_json["diagnostics"]["kept_images"], 1);
    assert_eq!(ingest_json["diagnostics"]["discarded_images"], 1);

    assert!(ws_dir.join("manifest.json").exists());
    assert!(ws_dir.join("structure").join("tree.json").exists());
    assert!(ws_dir.join("structure").join("ops.jsonl").exists());
    assert!(ws_dir.join("structure").join("chapters").join("s01.xhtml").exists());
    assert!(ws_dir.join("media").join("manifest.json").exists());
    assert!(ws_dir.join("media").join("img_001.png").exists());
    assert!(ws_dir.join("media").join("thumb_img_001.png").exists());
    assert!(ws_dir.join("rules").join("editorial.json").exists());
    assert!(ws_dir.join("rules").join("spell_audit.json").exists());

    let sections_json = run_cmd(&[
        "inspect",
        "sections",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(sections_json["total_sections"], 3);
    assert_eq!(sections_json["chapters_without_title"], 1);

    let sec_detail_json = run_cmd(&[
        "inspect",
        "section",
        "s01",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--head",
        "2",
        "--tail",
        "1",
        "--json",
    ]);
    assert_eq!(sec_detail_json["id"], "s01");
    assert_eq!(sec_detail_json["head"].as_array().unwrap().len(), 2);
    assert_eq!(sec_detail_json["tail"].as_array().unwrap().len(), 1);

    let patch_dry = run_cmd(&[
        "structure",
        "patch",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--merge",
        "s02,s03",
        "--rename",
        "s01=Prologue: The Cavern",
        "--retype",
        "s01=prologue",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(patch_dry["dry_run"], true);
    assert_eq!(patch_dry["total_sections"], 2);

    let sections_after_dry = run_cmd(&[
        "inspect",
        "sections",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(sections_after_dry["total_sections"], 3);

    let patch_real = run_cmd(&[
        "structure",
        "patch",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--merge",
        "s02,s03",
        "--rename",
        "s01=Prologue: The Cavern",
        "--retype",
        "s01=prologue",
        "--reorder",
        "s01,s02",
        "--json",
    ]);
    assert_eq!(patch_real["dry_run"], false);
    assert_eq!(patch_real["total_sections"], 2);
    assert_eq!(patch_real["chapters_without_title"], 0);
    assert_eq!(patch_real["sections"][0]["title"], "Prologue: The Cavern");
    assert_eq!(patch_real["sections"][0]["section_type"], "prologue");
    assert_eq!(patch_real["sections"][1]["paragraph_count"], 4);

    let media_disc_json = run_cmd(&[
        "inspect",
        "media",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--filter",
        "discarded,uncertain",
        "--json",
    ]);
    assert_eq!(media_disc_json["total_matching"], 1);
    assert_eq!(media_disc_json["items"][0]["id"], "img_002");

    let media_upd_json = run_cmd(&[
        "media",
        "update",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--keep",
        "img_001",
        "--alt",
        "img_001=Main cover artwork",
        "--decorative",
        "img_002",
        "--json",
    ]);
    assert_eq!(media_upd_json["status"], "ok");
    assert_eq!(media_upd_json["items"][0]["alt_text"], "Main cover artwork");
    assert_eq!(media_upd_json["items"][1]["decorative"], true);

    let audit_json = run_cmd(&[
        "audit",
        "text",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--min-conf",
        "0.75",
        "--limit",
        "50",
        "--json",
    ]);
    assert_eq!(audit_json["limit"], 50);

    let glossary_json = run_cmd(&[
        "glossary",
        "ignore",
        "Rimuru",
        "Veldora",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--apply-spell",
        "--min-conf",
        "0.90",
        "--json",
    ]);
    assert_eq!(glossary_json["status"], "ok");
    assert_eq!(glossary_json["total_ignored_words"], 2);

    let rules_json = run_cmd(&[
        "rules",
        "add",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--find",
        "--",
        "--replace",
        "—",
        "--flags",
        "g",
        "--json",
    ]);
    assert_eq!(rules_json["status"], "ok");
    assert_eq!(rules_json["affected_paragraphs"], 1);

    let build_json = run_cmd(&[
        "build",
        "--workspace",
        ws_dir.to_str().unwrap(),
        "--out",
        out_epub.to_str().unwrap(),
        "--export-project",
        out_proj.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(build_json["status"], "ok");
    assert_eq!(build_json["sections_built"], 2);
    assert_eq!(build_json["verification"]["valid"], true);
    assert!(out_epub.exists());
    assert!(out_proj.exists());

    {
        let proj_bytes = fs::read(&out_proj).unwrap();
        let mut proj_zip = ZipArchive::new(Cursor::new(proj_bytes)).unwrap();
        let manifest_entry = proj_zip.by_name("manifest.json").unwrap();
        assert_eq!(manifest_entry.compression(), CompressionMethod::Deflated);
        drop(manifest_entry);
        let img_entry = proj_zip.by_name("media/img_001.png").unwrap();
        assert_eq!(img_entry.compression(), CompressionMethod::Stored);
    }

    let verify_json = run_cmd(&["verify", out_epub.to_str().unwrap(), "--json"]);
    assert_eq!(verify_json["valid"], true);
    assert_eq!(
        verify_json["checks"]["ocf_mimetype_first_uncompressed"],
        true
    );
    assert_eq!(verify_json["checks"]["container_xml_valid"], true);
    assert_eq!(verify_json["checks"]["opf_epub3_metadata_valid"], true);
    assert_eq!(verify_json["checks"]["xhtml_well_formed"], true);
    assert_eq!(verify_json["checks"]["image_references_valid"], true);

    let inspect_archive_json = run_cmd(&[
        "inspect",
        "sections",
        "--workspace",
        out_proj.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(inspect_archive_json["total_sections"], 2);
    assert_eq!(
        inspect_archive_json["sections"][0]["title"],
        "Prologue: The Cavern"
    );

    let pdf_input = temp_root.join("Novela_Vol1.pdf");
    create_synthetic_pdf(&pdf_input);
    let ws_pdf = temp_root.join("work_pdf");
    let out_pdf_epub = temp_root.join("output").join("from_pdf.epub");

    let ingest_pdf_json = run_cmd(&[
        "ingest",
        pdf_input.to_str().unwrap(),
        "--workspace",
        ws_pdf.to_str().unwrap(),
        "--profile",
        "es-editorial",
        "--lang",
        "es",
        "--json",
    ]);
    assert_eq!(ingest_pdf_json["status"], "ok");
    assert_eq!(ingest_pdf_json["diagnostics"]["has_text_layer"], true);

    let build_pdf_json = run_cmd(&[
        "build",
        "--workspace",
        ws_pdf.to_str().unwrap(),
        "--out",
        out_pdf_epub.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(build_pdf_json["status"], "ok");
    assert_eq!(build_pdf_json["verification"]["valid"], true);

    let _ = fs::remove_dir_all(&temp_root);
}
