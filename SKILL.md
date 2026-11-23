---
name: editnovel
description: >-
  Convert messy PDFs, EPUBs, and light novel or fiction manuscripts into clean, validated EPUB 3
  or multi-volume omnibus books using EditNovel's deterministic local CLI and MCP feedback loop.
  Use when a user asks to convert, clean, merge, or typeset PDFs or EPUBs into EPUB 3.
---

# EditNovel Agent Workflow

Use EditNovel's staged local CLI (or `editnovel mcp` server) to convert and refine books deterministically without dumping raw book text into your context window.

## 5-Stage CLI Feedback Loop

1. **Ingest into a workspace directory**:
   `editnovel ingest <files...> --workspace ./work --profile <en-light-novel|en-litrpg|en-fiction|es-editorial> --lang <auto|es|en> --json`
   Inspect the returned `diagnostics` (`has_text_layer`, `chapters_without_title`, `discarded_images`, `uncertain_images`, `spell_clusters`).

2. **Inspect and patch structure**:
   `editnovel inspect sections --workspace ./work --json`
   Only fetch minimal previews (`editnovel inspect section <id> --workspace ./work --head 10 --tail 10 --json`) when ambiguous.
   Apply declarative patches (`editnovel structure patch --workspace ./work --merge s03,s04 --rename s02="Chapter 1" --retype s01=prologue --reorder s01,s02,s03 --dry-run --json`).

3. **Review media & accessibility**:
   `editnovel inspect media --workspace ./work --filter discarded,uncertain --json`
   Inspect thumbnail previews in `media/` on disk if uncertain, then approve/discard and attach `alt` text or mark decorative (`editnovel media update --workspace ./work --keep img_012 --discard img_013 --alt img_012="Cover illustration" --decorative img_014 --dry-run --json`).

4. **Audit deduplicated text, glossary & regex rules**:
   `editnovel audit text --workspace ./work --min-conf 0.75 --limit 50 --json`
   Protect proper nouns, rōmaji, and domain terms (`editnovel glossary ignore Rimuru Veldora --workspace ./work --apply-spell --min-conf 0.90 --json`).
   Add deterministic regex transformations (`editnovel rules add --workspace ./work --find "<pat>" --replace "<rep>" --flags g --dry-run --json`).

5. **Build & validate EPUB 3**:
   `editnovel build --workspace ./work --out ./output/book.epub --export-project ./output/book.editnovel --json`
   `editnovel verify ./output/book.epub --json`

## MCP Tools (`editnovel mcp`)

- `editnovel_ingest`
- `editnovel_inspect_sections`
- `editnovel_inspect_section`
- `editnovel_patch_structure`
- `editnovel_inspect_media`
- `editnovel_update_media`
- `editnovel_audit_text`
- `editnovel_apply_rules`
- `editnovel_build_epub`
- `editnovel_verify_epub`
