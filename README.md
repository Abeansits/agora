# Agora

A multi-agent deliberation tool where any LLM, CLI tool, or human can participate in structured, multi-turn discussions using the filesystem as a shared medium. Agora orchestrates a modified Delphi protocol — independent proposals, adversarial cross-examination, informed revision — then synthesizes agreement and preserves dissent as a first-class output.

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
| `human`    | (manual — writes files directly)         | filesystem   |

```bash
agora new "topic" --participant codex --participant gemini
```

### Custom Commands

```bash
agora new "topic" \
  --participant "llama:command:ollama run llama3 < {prompt_file}" \
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
judge_model = "claude-sonnet"
threshold = 7
min_rounds = 2

[synthesis]
model = "claude-sonnet"
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
