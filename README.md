# EditNovel

> Turn messy PDFs, EPUBs, and raw web-novel drafts into clean, bookstore-grade **EPUB 3** and **omnibus** volumes — locally, deterministically, and built for AI agents.

EditNovel is a local-first **CLI and MCP Server** written in Rust. It extracts text and illustrations from PDFs or existing EPUBs, removes fansub/scanner clutter (running headers, page numbers, watermarks via perceptual `dHash`), structures chapters and footnotes, audits spelling/glossaries (`EN` / `ES` / `rōmaji`), and packages validated **EPUB 3** files — **100% offline on your machine, with zero cloud uploads and zero LLM rewriting of your prose.**

---

## Quick Start for Humans & Publishers (Ask Your AI)

You don't need to learn command-line flags. Connect EditNovel to your AI assistant once, then ask it in plain English.

### 1. Connect via MCP (Claude Desktop, Cursor, ChatGPT Desktop, Antigravity)

Add this to your client's MCP configuration (`claude_desktop_config.json`, `.cursor/mcp.json`, etc.):

```json
{
  "mcpServers": {
    "editnovel": {
      "command": "npx",
      "args": ["-y", "editnovel", "mcp"]
    }
  }
}
```

Or install the Agent Skill directly for terminal agents (Claude Code, Codex, Gemini CLI):

```bash
npx skills add editnovel/editnovel
```

### 2. Example Prompts You Can Give Your AI

- *"Convert `Vol1.pdf` and `Vol2.pdf` in this folder into a single clean EPUB 3 omnibus. Remove repeated scanlation banners, keep full-page illustrations with alt-text, and preserve Japanese honorifics."*
- *"Clean up this EPUB exported from WebToEpub: fix broken dialogue quotes, merge the split prologue sections, and verify it passes EPUB 3 accessibility checks."*
- *"Inspect the images in `novel.pdf`, show me which ones look like watermark credits, and build the final EPUB once I confirm."*

---

## Why EditNovel? (For AI Agents & Power Users)

Traditional tools like Calibre break paragraphs on PDFs, leave page numbers inside chapters, and offer no structured feedback loop. EditNovel is **agent-first**:

- **Staged JSON Feedback Loop:** Never dumps an entire book into your context window. Inspect lightweight structural indices, image decisions (with local thumbnail previews for multimodal vision checks), and deduplicated spelling clusters before building.
- **Deterministic & Privacy-First:** Pure Rust execution on local files. Your manuscript never leaves your machine and is never hallucinated or paraphrased by an external model.
- **Domain-Specialized for Fiction, Light Novels & LitRPG:**
  - Multi-language typographic profiles (`en-light-novel`, `en-litrpg`, `en-fiction`, `es-editorial`) and language modes (`auto`, `es`, `en`).
  - Perceptual `dHash` image filtering to strip duplicate watermarks/banners automatically.
  - Bidirectional EPUB 3 footnotes, scene-break unification, and multi-volume omnibus merging.
  - Optional `.editnovel` project archive export for archival or portable inspection.

---

## Agent Workflow (5-Stage CLI Loop)

All commands support `--json` for strict machine-readable output and `--dry-run` for safe previews.

```bash
npx -y editnovel ingest ./books/*.pdf --workspace ./work --profile en-light-novel --lang auto --json

npx -y editnovel inspect sections --workspace ./work --json
npx -y editnovel inspect section s02 --workspace ./work --head 10 --tail 10 --json
npx -y editnovel structure patch --workspace ./work --merge s03,s04 --rename s02="Chapter 1" --dry-run --json

npx -y editnovel inspect media --workspace ./work --filter discarded,uncertain --json
npx -y editnovel media update --workspace ./work --keep img_012 --alt img_012="Cover illustration" --json

npx -y editnovel audit text --workspace ./work --min-conf 0.75 --limit 50 --json
npx -y editnovel glossary ignore Rimuru Veldora --workspace ./work --apply-spell --min-conf 0.90 --json
npx -y editnovel rules add --workspace ./work --find "--" --replace "—" --flags g --dry-run --json

npx -y editnovel build --workspace ./work --out ./output/omnibus.epub --export-project ./output/omnibus.editnovel --json
npx -y editnovel verify ./output/omnibus.epub --json
```

---

## Installation Options

| Method | Command | Best For |
| :--- | :--- | :--- |
| **Zero-install (npx)** | `npx -y editnovel --help` | AI agents & quick runs |
| **Homebrew (macOS/Linux)** | `brew install editnovel/tap/editnovel` | Daily CLI & MCP users |
| **Cargo (Rust)** | `cargo binstall editnovel` | Rust developers |

---

## Supported Inputs & Outputs

- **Inputs:** Digital PDFs (with text layer), EPUB 2/3, `.editnovel` archives, multi-volume inputs for omnibus creation.
- **Outputs:** Standard **EPUB 3** (valid OCF, Kindle/Kobo/Apple Books ready, accessibility metadata) + optional `.editnovel` portable project bundle.
