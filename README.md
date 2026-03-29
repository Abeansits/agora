# Agora

A multi-agent deliberation tool where any LLM, CLI tool, or human can participate in structured, multi-turn discussions using the filesystem as a shared medium. Agora orchestrates a modified Delphi protocol — independent proposals, adversarial cross-examination, informed revision — then synthesizes agreement and preserves dissent as a first-class output.

## Who This Is For

**Agora is for you if:**
- You use multiple AI models and want better decisions than any single model gives
- You want structured disagreement, not just "ask Claude" — cross-examination surfaces blind spots
- You make architecture, planning, or strategy decisions regularly and want to stress-test your thinking
- You want a record of *why* a decision was made, including the dissenting views

**Agora is NOT for:**
- Simple Q&A where one model is enough — Agora is overkill for "fix this bug"
- Real-time chat — deliberation takes minutes, not seconds
- People who want a framework or SDK — this is a standalone CLI tool
- Consensus-seeking — Agora preserves dissent as a first-class output, not a failure mode

## Prerequisites

- **Rust** (1.85+, edition 2024)
- **Claude Code** (`claude` CLI) — used for synthesis, convergence evaluation, and as a participant
- At least one participant CLI installed and authenticated: `codex`, `gemini`, `opencode`, or just use `human` for manual participation
- Optional: `herenow` CLI for publishing HTML reports via `--publish`

## Quick Start

```bash
# Build
cargo build --release

# Run a 3-model deliberation
agora new "Should we use Pipecat or Vapi for voice?" \
  --participant codex \
  --participant gemini \
  --participant claude

# Check progress
agora status <forum-id>

# View result
agora result <forum-id>

# Generate HTML report
agora result --html <forum-id>

# Publish report to the web
agora result --html --publish <forum-id>
```

### What You'll See

```
   █████╗  ██████╗  ██████╗ ██████╗  █████╗
  ██╔══██╗██╔════╝ ██╔═══██╗██╔══██╗██╔══██╗
  ███████║██║  ███╗██║   ██║██████╔╝███████║
  ██╔══██║██║   ██║██║   ██║██╔══██╗██╔══██║
  ██║  ██║╚██████╔╝╚██████╔╝██║  ██║██║  ██║
  ╚═╝  ╚═╝ ╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝
  v0.1.1  Structured deliberation between AI models

  Forum  agora-2026-03-27-a1b2c3d4
  Topic  Should we use Pipecat or Vapi for voice?
  With   codex, gemini, claude
  Rules  5 rounds, 5m timeout

=== Round 1 (proposal) ===
  Wrote round-1/prompt.md
  Invoking participant: codex
  Invoking participant: gemini
  Invoking participant: claude
  Collected 3/3 responses
  Generating synthesis...
  Generating claims...

=== Round 2 (cross-examination) ===
  Wrote round-2/prompt.md
  Invoking participant: codex
  Invoking participant: gemini
  Invoking participant: claude
  Collected 3/3 responses
  Generating synthesis...
  Generating claims...
  Evaluating convergence...
  CONVERGED (score: 8.0): Strong agreement on core architecture...

=== Final output written to ~/.agora/sessions/agora-2026-03-27-a1b2c3d4/final/ ===
```

## Protocol

```
 Round 1: PROPOSAL (blind)
 Each participant independently proposes their position.
         |
         v
 Round 2: CROSS-EXAMINATION (adversarial)
 Each participant critiques an assigned other's position,
 then defends or revises their own.
         |
         v
 Round 3+: REVISION (informed)
 Participants revise their positions given all prior context.
         |
         v
 CONVERGENCE CHECK (LLM judge, score 1-10)
   >= threshold --> final/synthesis.md + final/claims.toml
   < threshold  --> another round (up to max_rounds)
                    final/dissent.md preserves disagreements
```

Dissent is not failure — it's the most valuable output when models genuinely disagree.

## CLI Reference

### `agora new`

```bash
agora new "Your question or topic" \
  --participant codex \
  --participant gemini \
  --participant human \
  --timeout 5m \
  --max-rounds 5 \
  --context notes.md    # attach supplementary material
```

Creates a forum and runs the full deliberation (blocking). The `--context` flag accepts a file path or inline text that gets included in every round's prompt. Context is snapshotted at creation time (not re-read each round) for reproducibility.

### `agora status <forum-id>`

Shows current round, participant responses received/missing, and completion state.

### `agora list`

Lists all forums with status and topic.

### `agora result <forum-id>`

Prints the final synthesis and dissent to terminal. Add `--html` to generate a self-contained HTML report. Add `--publish` to push it to the web via here.now.

### `agora respond <forum-id>`

For human participants — submit a response from another terminal while the forum is running:

```bash
agora respond <forum-id> -r 1 -n human -f my-response.md
```

## Participant Types

### Presets (built-in)

| Preset     | Command                                  | Input Method |
|------------|------------------------------------------|--------------|
| `codex`    | `codex exec --full-auto -`               | stdin        |
| `gemini`   | `cat {prompt_file} \| gemini -p ' '`     | file pipe    |
| `claude`   | `cat {prompt_file} \| claude -p -`       | file pipe    |
| `opencode` | `opencode run`                           | stdin        |
| `ollama`   | `cat {prompt_file} \| ollama run llama3` | file pipe    |
| `human`    | (manual — writes files directly)         | filesystem   |

```bash
agora new "topic" --participant codex --participant gemini
```

### Custom Presets

Save reusable presets with `agora preset`:

```bash
# Add a custom preset
agora preset add mistral "cat {prompt_file} | ollama run mistral"

# List all presets (built-in + custom)
agora preset list

# Use it
agora new "topic" --participant mistral --participant codex

# Remove it
agora preset remove mistral
```

Custom presets are stored in `~/.agora/config.toml` and override built-ins of the same name.

### Custom Commands (inline)

```bash
agora new "topic" \
  --participant "llama:command:cat {prompt_file} | ollama run llama3" \
  --participant "gpt:command:cat {prompt_file} | openai-cli chat"
```

The prompt is delivered to commands via:
1. **stdin** — piped directly (safest)
2. **`{prompt_file}`** — replaced with a temp file path in the command
3. **`$AGORA_PROMPT_FILE`** — env var pointing to the same temp file

### Human / Manual

```bash
agora new "topic" --participant human --participant codex
```

When the fire keeper needs a human response, it prints instructions:
```
Waiting for human. Submit your response:
  Option A: Write to ~/.agora/sessions/<id>/round-1/human.md
  Option B: agora respond <id> -r 1 -n human -f response.md
```

### Other Models

Any CLI that reads from stdin or a file can participate. Examples:

```bash
# Cursor (editor, no CLI agent mode — use via custom command if they add one)
# Pi (no public CLI — use via API wrapper)

# Any ollama model
agora preset add deepseek "cat {prompt_file} | ollama run deepseek-r1"
```

## Configuration

Forums are configured via `meta.toml`, generated automatically by `agora new`:

```toml
[forum]
id = "agora-2026-03-27-001"
topic = "Should we use Pipecat or Vapi?"
created = "2026-03-27T00:30:00Z"
max_rounds = 5
protocol = "delphi-crossexam"
context = "Optional supplementary material..."

[participants]
names = ["codex", "gemini"]

[participants.codex]
type = "command"
command = "codex exec --full-auto -"

[participants.gemini]
type = "command"
command = "gemini -p \" \""

[timing]
round_timeout = "5m"
participant_timeout = "2m"

[convergence]
policy = "llm-judge"
judge_model = "claude-opus"
threshold = 7
min_rounds = 2

[synthesis]
model = "claude-opus"
```

## Directory Structure

```
~/.agora/sessions/<forum-id>/
  meta.toml
  round-1/
    prompt.md
    codex.md
    gemini.md
    synthesis.md
    claims.toml
  round-2/
    ...
  final/
    synthesis.md
    claims.toml
    dissent.md
    meta-summary.toml
    report.html        # with --html flag
```

## Architecture

```
Participants (any CLI, LLM, or human)
        |  write responses
        v
   Filesystem Substrate
   sessions/<id>/round-N/*.md
        |  watch (notify)
        v
    Fire Keeper (this binary)
    - Orchestrates rounds
    - Generates synthesis (via claude CLI)
    - Evaluates convergence (LLM judge)
    - Writes final output
```

---

<p align="center">Built on 🌍 with ❤️</p>
