---
name: editnovel
description: >-
  Convert messy PDFs, EPUBs, and light novel or fiction manuscripts into clean, validated EPUB 3
  or multi-volume omnibus books using EditNovel's deterministic local CLI and MCP feedback loop.
  Use when a user asks to convert, clean, merge, or typeset PDFs or EPUBs into EPUB 3.
---

# EditNovel Agent Workflow

Use EditNovel's staged local CLI (or `editnovel mcp` server) to convert and refine books deterministically without dumping raw book text into your context window.

## 5-Stage Feedback Loop

1. **Ingest into a workspace directory**:
   `editnovel ingest <files...> --workspace ./work --profile <en-light-novel|en-litrpg|en-fiction|es-editorial> --json`
   Inspect the returned `diagnostics` (`has_text_layer`, `chapters_without_title`, `discarded_images`, `spell_clusters`).

2. **Inspect and patch structure**:
   `editnovel inspect sections --workspace ./work --json`
   Only fetch minimal previews (`inspect section <id> --head 10 --tail 10 --json`) when ambiguous.
   Apply declarative patches (`structure patch --merge ... --rename ... --retype ... --dry-run --json`).

3. **Review media & accessibility**:
   `editnovel inspect media --workspace ./work --filter discarded,uncertain --json`
   Inspect thumbnail previews on disk if uncertain, then approve/discard and attach `alt` text (`media update --keep ... --discard ... --alt ... --json`).

4. **Audit deduplicated text & glossary**:
   `editnovel audit text --workspace ./work --min-conf 0.75 --json`
   Protect proper nouns, rōmaji, and domain terms (`glossary ignore ... --apply-spell --min-conf 0.90 --json`) and re-audit.

5. **Build & validate EPUB 3**:
   `editnovel build --workspace ./work --out ./output/book.epub --json`
   Optionally add `--export-project ./output/book.editnovel` when the user wants a portable archive.
