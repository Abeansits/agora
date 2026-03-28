use crate::substrate;
use crate::types::*;
use anyhow::Result;
use std::path::Path;

/// Generate a self-contained HTML report for a completed forum
pub fn generate_html_report(config: &ForumConfig, forum_path: &Path) -> Result<String> {
    let total_rounds = substrate::current_round(forum_path);
    let final_dir = forum_path.join("final");

    // Collect round data
    let mut rounds_html = String::new();
    for r in 1..=total_rounds {
        let round_dir = forum_path.join(format!("round-{}", r));
        let stage = match r {
            1 => "Proposal",
            2 => "Cross-Examination",
            _ => "Revision",
        };

        // Read prompt
        let prompt = read_optional(&round_dir.join("prompt.md"));

        // Read participant responses
        let mut responses_html = String::new();
        for (i, name) in config.participants.names.iter().enumerate() {
            let response = read_optional(&round_dir.join(format!("{}.md", name)));
            let checked = if i == 0 { " checked" } else { "" };
            responses_html.push_str(&format!(
                r#"<input type="radio" name="round{r}-tab" id="round{r}-{name}" class="tab-input"{checked}>
<label for="round{r}-{name}" class="tab-label">{name}</label>
<div class="tab-content"><div class="md-render"><textarea class="md-src">{response_escaped}</textarea></div></div>
"#,
                r = r,
                name = name,
                checked = checked,
                response_escaped = escape_html_attr(&response),
            ));
        }

        // Read synthesis
        let synthesis = read_optional(&round_dir.join("synthesis.md"));

        let prompt_summary = prompt
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");

        rounds_html.push_str(&format!(
            r#"<details class="round" {open}>
<summary>
  <span class="round-badge">Round {r}</span>
  <span class="stage-label">{stage}</span>
</summary>
<div class="round-body">
  <div class="section-label">Prompt</div>
  <div class="prompt-summary">{prompt_summary}</div>

  <div class="section-label">Responses</div>
  <div class="tabs">
    {responses_html}
  </div>

  <div class="section-label">Synthesis</div>
  <div class="synthesis md-render"><textarea class="md-src">{synthesis_escaped}</textarea></div>
</div>
</details>
"#,
            r = r,
            stage = stage,
            open = if r == total_rounds { "open" } else { "" },
            prompt_summary = escape_html_attr(&prompt_summary),
            responses_html = responses_html,
            synthesis_escaped = escape_html_attr(&synthesis),
        ));
    }

    // Final outputs
    let final_synthesis = read_optional(&final_dir.join("synthesis.md"));
    let final_dissent = read_optional(&final_dir.join("dissent.md"));
    let final_claims = read_optional(&final_dir.join("claims.toml"));
    let meta_summary = read_optional(&final_dir.join("meta-summary.toml"));

    // Parse score from meta-summary
    let score = meta_summary
        .lines()
        .find(|l| l.starts_with("final_score"))
        .and_then(|l| l.split('=').nth(1))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    let status = if meta_summary.contains("converged") {
        "Converged"
    } else {
        "Divergent"
    };
    let score_color = if score >= 7.0 {
        "#4ade80"
    } else if score >= 4.0 {
        "#facc15"
    } else {
        "#f87171"
    };

    let participants_list = config
        .participants
        .names
        .iter()
        .map(|n| {
            let ptype = config
                .participants
                .configs
                .get(n)
                .map_or("unknown", |c| &c.participant_type);
            format!("<span class=\"participant-chip\">{} <small>({})</small></span>", n, ptype)
        })
        .collect::<Vec<_>>()
        .join(" ");

    let has_dissent = !final_dissent.is_empty()
        && !final_dissent.contains("No unresolved disagreements");

    let dissent_section = if has_dissent {
        format!(
            r#"<div class="final-section dissent-section">
  <h2>Dissent</h2>
  <div class="md-render"><textarea class="md-src">{}</textarea></div>
</div>"#,
            escape_html_attr(&final_dissent)
        )
    } else {
        String::new()
    };

    let claims_section = if !final_claims.is_empty() {
        format!(
            r#"<div class="final-section">
  <h2>Claims</h2>
  <pre class="claims-pre">{}</pre>
</div>"#,
            escape_html_attr(&final_claims)
        )
    } else {
        String::new()
    };

    Ok(format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Agora — {topic}</title>
<style>
{css}
</style>
</head>
<body>
<div class="container">

<header>
  <div class="logo">AGORA</div>
  <div class="meta-row">
    <span class="meta-label">Forum</span>
    <span class="meta-value">{forum_id}</span>
    <span class="meta-label">Protocol</span>
    <span class="meta-value">{protocol}</span>
    <span class="meta-label">Rounds</span>
    <span class="meta-value">{total_rounds}</span>
  </div>
</header>

<section class="topic-section">
  <h1>{topic}</h1>
  <div class="participants">{participants_list}</div>
  <div class="score-row">
    <span class="score-badge" style="background:{score_color}">{score:.1}</span>
    <span class="score-status">{status}</span>
  </div>
</section>

<section class="rounds-section">
  <h2>Deliberation Rounds</h2>
  {rounds_html}
</section>

<section class="final-output">
  <div class="final-section synthesis-final">
    <h2>Final Synthesis</h2>
    <div class="md-render"><textarea class="md-src">{final_synthesis}</textarea></div>
  </div>

  {dissent_section}
  {claims_section}
</section>

<footer>
  <span>Generated by Agora v0.1</span>
  <span>{created}</span>
</footer>

</div>
<script src="https://cdn.jsdelivr.net/npm/marked@15/marked.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/dompurify@3/dist/purify.min.js"></script>
<script>
document.querySelectorAll('.md-render').forEach(el => {{
  const src = el.querySelector('.md-src');
  if (src) {{
    el.innerHTML = DOMPurify.sanitize(marked.parse(src.value));
  }}
}});
</script>
</body>
</html>"##,
        css = CSS,
        topic = escape_html_attr(&config.forum.topic),
        forum_id = escape_html_attr(&config.forum.id),
        protocol = escape_html_attr(&config.forum.protocol),
        total_rounds = total_rounds,
        participants_list = participants_list,
        score_color = score_color,
        score = score,
        status = status,
        rounds_html = rounds_html,
        final_synthesis = escape_html_attr(&final_synthesis),
        dissent_section = dissent_section,
        claims_section = claims_section,
        created = escape_html_attr(&config.forum.created),
    ))
}

fn read_optional(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Escape for embedding in HTML textarea content or attributes.
/// Only need to escape < and & (textarea doesn't interpret HTML tags).
fn escape_html_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const CSS: &str = r#"
:root {
  --bg: #0d1117;
  --surface: #161b22;
  --surface-2: #1c2333;
  --border: #30363d;
  --text: #e6edf3;
  --text-dim: #8b949e;
  --accent: #58a6ff;
  --accent-dim: #1f3a5f;
  --green: #4ade80;
  --yellow: #facc15;
  --red: #f87171;
  --font: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  --mono: "SF Mono", "Fira Code", "Fira Mono", Menlo, Consolas, monospace;
}

* { margin: 0; padding: 0; box-sizing: border-box; }

body {
  background: var(--bg);
  color: var(--text);
  font-family: var(--font);
  font-size: 15px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}

.container {
  max-width: 900px;
  margin: 0 auto;
  padding: 40px 24px;
}

header {
  margin-bottom: 32px;
}

.logo {
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 4px;
  color: var(--accent);
  margin-bottom: 12px;
}

.meta-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 16px;
  font-size: 13px;
}

.meta-label {
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  font-size: 11px;
}

.meta-value {
  color: var(--text);
  font-family: var(--mono);
  font-size: 13px;
}

.topic-section {
  margin-bottom: 40px;
}

.topic-section h1 {
  font-size: 24px;
  font-weight: 600;
  line-height: 1.3;
  margin-bottom: 16px;
  color: var(--text);
}

.participants {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 16px;
}

.participant-chip {
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 20px;
  padding: 4px 14px;
  font-size: 13px;
  font-weight: 500;
}

.participant-chip small {
  color: var(--text-dim);
  font-weight: 400;
}

.score-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.score-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  border-radius: 12px;
  font-size: 18px;
  font-weight: 700;
  color: var(--bg);
}

.score-status {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-dim);
}

h2 {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 16px;
  color: var(--text);
}

.rounds-section {
  margin-bottom: 40px;
}

.round {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  margin-bottom: 12px;
  overflow: hidden;
}

.round summary {
  padding: 16px 20px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 12px;
  list-style: none;
  user-select: none;
}

.round summary::-webkit-details-marker { display: none; }

.round summary::before {
  content: "\25B6";
  font-size: 10px;
  color: var(--text-dim);
  transition: transform 0.2s;
}

.round[open] summary::before {
  transform: rotate(90deg);
}

.round-badge {
  background: var(--accent-dim);
  color: var(--accent);
  font-size: 12px;
  font-weight: 600;
  padding: 2px 10px;
  border-radius: 10px;
  letter-spacing: 0.3px;
}

.stage-label {
  color: var(--text-dim);
  font-size: 14px;
}

.round-body {
  padding: 0 20px 20px;
}

.section-label {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 1px;
  color: var(--text-dim);
  margin: 20px 0 8px;
}

.section-label:first-child {
  margin-top: 0;
}

.prompt-summary {
  background: var(--surface-2);
  border-left: 3px solid var(--accent);
  padding: 12px 16px;
  border-radius: 0 8px 8px 0;
  font-size: 13px;
  color: var(--text-dim);
}

/* Tab system (pure CSS) */
.tabs {
  position: relative;
}

.tab-input {
  display: none;
}

.tab-label {
  display: inline-block;
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-dim);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  transition: all 0.15s;
}

.tab-label:hover {
  color: var(--text);
}

.tab-input:checked + .tab-label {
  color: var(--accent);
  border-bottom-color: var(--accent);
}

.tab-content {
  display: none;
  padding: 16px;
  background: var(--surface-2);
  border-radius: 0 0 8px 8px;
  border: 1px solid var(--border);
  border-top: none;
}

.tab-input:checked + .tab-label + .tab-content {
  display: block;
}

.synthesis {
  background: var(--surface-2);
  padding: 16px;
  border-radius: 8px;
  border: 1px solid var(--border);
}

/* Hide raw markdown source textareas */
.md-src { display: none; }

/* Rendered markdown styles */
.md-render {
  font-size: 14px;
  line-height: 1.7;
  color: var(--text);
  word-wrap: break-word;
}

.md-render h1, .md-render h2, .md-render h3, .md-render h4 {
  color: var(--text);
  margin: 20px 0 8px;
  line-height: 1.3;
}

.md-render h1 { font-size: 20px; }
.md-render h2 { font-size: 17px; }
.md-render h3 { font-size: 15px; }

.md-render p { margin: 8px 0; }

.md-render ul, .md-render ol {
  margin: 8px 0;
  padding-left: 24px;
}

.md-render li { margin: 4px 0; }

.md-render strong { color: var(--text); font-weight: 600; }

.md-render code {
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 1px 6px;
  font-family: var(--mono);
  font-size: 13px;
}

.md-render pre {
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 14px 16px;
  overflow-x: auto;
  margin: 12px 0;
}

.md-render pre code {
  background: none;
  border: none;
  padding: 0;
  font-size: 12px;
  line-height: 1.5;
}

.md-render table {
  border-collapse: collapse;
  width: 100%;
  margin: 12px 0;
  font-size: 13px;
}

.md-render th, .md-render td {
  border: 1px solid var(--border);
  padding: 8px 12px;
  text-align: left;
}

.md-render th {
  background: var(--surface-2);
  font-weight: 600;
  color: var(--text);
}

.md-render td { color: var(--text-dim); }

.md-render blockquote {
  border-left: 3px solid var(--accent);
  padding: 4px 16px;
  margin: 12px 0;
  color: var(--text-dim);
}

.md-render hr {
  border: none;
  border-top: 1px solid var(--border);
  margin: 20px 0;
}

.md-render a {
  color: var(--accent);
  text-decoration: none;
}

.md-render a:hover { text-decoration: underline; }

.final-output {
  margin-bottom: 40px;
}

.final-section {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px;
  margin-bottom: 16px;
}

.synthesis-final {
  border-color: var(--accent-dim);
  border-width: 2px;
}

.dissent-section {
  border-color: #f8717133;
}

.dissent-section h2 {
  color: var(--red);
}

.claims-pre {
  background: var(--surface-2);
  padding: 16px;
  border-radius: 8px;
  font-family: var(--mono);
  font-size: 12px;
  overflow-x: auto;
  color: var(--text-dim);
  line-height: 1.5;
}

footer {
  display: flex;
  justify-content: space-between;
  padding-top: 24px;
  border-top: 1px solid var(--border);
  font-size: 12px;
  color: var(--text-dim);
}

@media (max-width: 600px) {
  .container { padding: 20px 16px; }
  .topic-section h1 { font-size: 20px; }
  .meta-row { flex-direction: column; }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html_attr() {
        assert_eq!(escape_html_attr("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html_attr("a & b"), "a &amp; b");
        assert_eq!(escape_html_attr("a > b"), "a &gt; b");
    }

    #[test]
    fn test_read_optional_missing_file() {
        let result = read_optional(Path::new("/nonexistent/path.md"));
        assert!(result.is_empty());
    }
}
