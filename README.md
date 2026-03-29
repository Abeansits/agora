# Ting

A multi-agent deliberation tool where any LLM, CLI tool, or human can participate in structured, multi-turn discussions using the filesystem as a shared medium. Ting orchestrates a modified Delphi protocol — independent proposals, adversarial cross-examination, informed revision — then synthesizes agreement and preserves dissent as a first-class output.

## Who This Is For

**Ting is for you if:**
- You use multiple AI models and want better decisions than any single model gives
- You want structured disagreement, not just "ask Claude" — cross-examination surfaces blind spots
- You make architecture, planning, or strategy decisions regularly and want to stress-test your thinking
- You want a record of *why* a decision was made, including the dissenting views

**Ting is NOT for:**
- Simple Q&A where one model is enough — Ting is overkill for "fix this bug"
- Real-time chat — deliberation takes minutes, not seconds
- People who want a framework or SDK — this is a standalone CLI tool
- Consensus-seeking — Ting preserves dissent as a first-class output, not a failure mode

## Prerequisites

- **Rust** (1.85+, edition 2024)
- **Claude Code** (`claude` CLI) — required for synthesis generation and convergence evaluation (fire keeper internals). Also available as a participant preset, but not required as one
- At least one participant CLI installed and authenticated: `codex`, `gemini`, `opencode`, or just use `human` for manual participation
- Optional: `herenow` CLI for publishing HTML reports via `--publish`

## Quick Start

```bash
# Build
cargo build --release

# Run a 3-model deliberation
ting new "Should we use Pipecat or Vapi for voice?" \
  --participant codex \
  --participant gemini \
  --participant claude

# Check progress
ting status <forum-id>

# View result
ting result <forum-id>

# Generate HTML report
ting result --html <forum-id>

# Publish report to the web
ting result --html --publish <forum-id>
```

### What You'll See

```
  ████████╗██╗███╗   ██╗ ██████╗
  ╚══██╔══╝██║████╗  ██║██╔════╝
     ██║   ██║██╔██╗ ██║██║  ███╗
     ██║   ██║██║╚██╗██║██║   ██║
     ██║   ██║██║ ╚████║╚██████╔╝
     ╚═╝   ╚═╝╚═╝  ╚═══╝ ╚═════╝
  v0.3.0  Structured deliberation between AI models

  Forum  ting-2026-03-27-a1b2c3d4
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

=== Final output written to ~/.ting/sessions/ting-2026-03-27-a1b2c3d4/final/ ===
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

### `ting new`

```bash
ting new "Your question or topic" \
  --participant codex \
  --participant gemini \
  --participant human \
  --timeout 5m \
  --max-rounds 5 \
  --context notes.md    # attach supplementary material
```

Creates a forum and runs the full deliberation (blocking). The `--context` flag accepts a file path or inline text that gets included in every round's prompt. Context is snapshotted at creation time (not re-read each round) for reproducibility.

### `ting status <forum-id>`

Shows current round, participant responses received/missing, and completion state.

### `ting list`

Lists all forums with status and topic.

### `ting result <forum-id>`

Prints the final synthesis and dissent to terminal. Add `--html` to generate a self-contained HTML report. Add `--publish` to push it to the web via here.now.

### `ting respond <forum-id>`

For human participants — submit a response from another terminal while the forum is running.
Round, participant name, and input method are all auto-detected:

```bash
# Simplest: auto-detects round + participant, opens $EDITOR
ting respond <forum-id>

# Explicit: specify round, name, and file
ting respond <forum-id> -r 2 -n human -f my-response.md
```

### `ting status <forum-id>`

Shows round-by-round progress with who has/hasn't responded:

```bash
ting status <forum-id>

# View a specific round's responses
ting status <forum-id> --round 2
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
ting new "topic" --participant codex --participant gemini
```

### Custom Presets

Save reusable presets with `ting preset`:

```bash
# Add a custom preset
ting preset add mistral "cat {prompt_file} | ollama run mistral"

# List all presets (built-in + custom)
ting preset list

# Use it
ting new "topic" --participant mistral --participant codex

# Remove it
ting preset remove mistral
```

Custom presets are stored in `~/.ting/config.toml` and override built-ins of the same name.

### Custom Commands (inline)

```bash
ting new "topic" \
  --participant "llama:command:cat {prompt_file} | ollama run llama3" \
  --participant "gpt:command:cat {prompt_file} | openai-cli chat"
```

The prompt is delivered to commands via:
1. **stdin** — piped directly (safest)
2. **`{prompt_file}`** — replaced with a temp file path in the command
3. **`$TING_PROMPT_FILE`** — env var pointing to the same temp file

### Human / Manual

```bash
ting new "topic" --participant human --participant codex
```

When the fire keeper needs a human response, it prints instructions:
```
  ✓ claude responded (1,203 words)
  ✓ codex responded (987 words)

  ⏳ Waiting for YOU (human)

    Read others' responses:  ting status <id> --round 1
    Write your response:     ting respond <id>
    Or edit directly:        ~/.ting/sessions/<id>/round-1/human.md

  Watching for your file... (timeout in 4m30s)
```

### Other Models

Any CLI that reads from stdin or a file can participate. Examples:

```bash
# Cursor (editor, no CLI agent mode — use via custom command if they add one)
# Pi (no public CLI — use via API wrapper)

# Any ollama model
ting preset add deepseek "cat {prompt_file} | ollama run deepseek-r1"
```

## Configuration

Forums are configured via `meta.toml`, generated automatically by `ting new`:

```toml
[forum]
id = "ting-2026-03-27-001"
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
~/.ting/sessions/<forum-id>/
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
