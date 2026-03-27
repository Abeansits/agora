use crate::{config, convergence, substrate, synthesis, types::*};
use anyhow::Result;
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::path::Path;

/// Run a complete forum deliberation through the modified Delphi protocol
pub fn run_forum(forum_config: &ForumConfig, forum_path: &Path) -> Result<()> {
    let mut prior_rounds: Vec<RoundData> = Vec::new();

    for round_num in 1..=forum_config.forum.max_rounds {
        let stage = match round_num {
            1 => Stage::Proposal,
            2 => Stage::CrossExam,
            _ => Stage::Revision,
        };

        eprintln!("\n=== Round {} ({}) ===", round_num, stage);

        // Generate and write prompt
        let prompt = generate_prompt(forum_config, round_num, &stage, &prior_rounds)?;
        let round_dir = substrate::create_round_dir(forum_path, round_num)?;
        substrate::write_atomic(&round_dir.join("prompt.md"), &prompt)?;
        eprintln!("  Wrote round-{}/prompt.md", round_num);

        // Invoke participants and collect responses
        let responses = invoke_participants(forum_config, &prompt, forum_path, round_num)?;

        if responses.is_empty() {
            eprintln!("  No responses received. Ending deliberation.");
            break;
        }

        eprintln!(
            "  Collected {}/{} responses",
            responses.len(),
            forum_config.participants.names.len()
        );

        // Generate synthesis
        eprintln!("  Generating synthesis...");
        let prior_synth = prior_rounds
            .last()
            .and_then(|r| r.synthesis.as_deref());
        let synth = synthesis::generate_synthesis(
            &forum_config.synthesis,
            &forum_config.forum.topic,
            round_num,
            &stage,
            &responses,
            prior_synth,
        )?;
        substrate::write_atomic(&round_dir.join("synthesis.md"), &synth)?;

        // Generate claims
        eprintln!("  Generating claims...");
        let claims = synthesis::generate_claims(
            &forum_config.synthesis,
            &forum_config.forum.topic,
            &responses,
        )?;
        substrate::write_atomic_toml(&round_dir.join("claims.toml"), &claims)?;

        let round_data = RoundData {
            number: round_num,
            stage,
            responses: responses.clone(),
            synthesis: Some(synth),
            claims: Some(claims),
        };
        prior_rounds.push(round_data);

        // Convergence check (only after min_rounds)
        if round_num >= forum_config.convergence.min_rounds {
            eprintln!("  Evaluating convergence...");
            let result = convergence::evaluate(
                &forum_config.convergence,
                &forum_config.forum.topic,
                &responses,
                forum_config.convergence.threshold,
            )?;

            match &result {
                ConvergenceResult::Converged { score, summary } => {
                    eprintln!("  CONVERGED (score: {:.1}): {}", score, summary);
                    write_final_output(forum_config, forum_path, &prior_rounds, &result)?;
                    return Ok(());
                }
                ConvergenceResult::Divergent {
                    score,
                    key_disagreements,
                } => {
                    eprintln!("  Divergent (score: {:.1})", score);
                    for d in key_disagreements {
                        eprintln!("    - {}", d);
                    }
                }
            }
        }
    }

    // Max rounds reached without convergence
    eprintln!(
        "\n=== Max rounds ({}) reached ===",
        forum_config.forum.max_rounds
    );
    let last_responses = prior_rounds
        .last()
        .map(|r| r.responses.clone())
        .unwrap_or_default();
    let final_result = convergence::evaluate(
        &forum_config.convergence,
        &forum_config.forum.topic,
        &last_responses,
        forum_config.convergence.threshold,
    )?;
    write_final_output(forum_config, forum_path, &prior_rounds, &final_result)?;

    Ok(())
}

fn invoke_participants(
    config: &ForumConfig,
    prompt: &str,
    forum_path: &Path,
    round: u32,
) -> Result<HashMap<String, String>> {
    let round_dir = forum_path.join(format!("round-{}", round));
    let mut responses = HashMap::new();

    // Split participants by type
    let command_participants: Vec<String> = config
        .participants
        .names
        .iter()
        .filter(|n| {
            config
                .participants
                .configs
                .get(*n)
                .is_some_and(|c| c.participant_type == "command")
        })
        .cloned()
        .collect();

    let manual_participants: Vec<String> = config
        .participants
        .names
        .iter()
        .filter(|n| {
            config
                .participants
                .configs
                .get(*n)
                .is_some_and(|c| c.participant_type == "manual")
        })
        .cloned()
        .collect();

    // Parse participant timeout
    let participant_timeout = config::parse_duration(&config.timing.participant_timeout)?;

    // Invoke command participants concurrently via threads
    let handles: Vec<_> = command_participants
        .iter()
        .map(|name| {
            let name = name.clone();
            let cmd_template = config.participants.configs[&name]
                .command
                .clone()
                .unwrap();
            let prompt = prompt.to_string();
            let round_dir = round_dir.clone();
            let timeout = participant_timeout;

            std::thread::spawn(move || -> Result<(String, String)> {
                eprintln!("  Invoking participant: {}", name);
                let response =
                    substrate::invoke_command(&cmd_template, &prompt, timeout)?;
                substrate::write_atomic(
                    &round_dir.join(format!("{}.md", name)),
                    &response,
                )?;
                Ok((name, response))
            })
        })
        .collect();

    // Collect command participant results
    for handle in handles {
        match handle.join() {
            Ok(Ok((name, response))) => {
                responses.insert(name, response);
            }
            Ok(Err(e)) => {
                eprintln!("  Warning: participant failed: {}", e);
            }
            Err(_) => {
                eprintln!("  Warning: participant thread panicked");
            }
        }
    }

    // Wait for manual participants via filesystem watching
    if !manual_participants.is_empty() {
        eprintln!(
            "  Waiting for manual participants: {:?}",
            manual_participants
        );
        let timeout = config::parse_duration(&config.timing.round_timeout)?;
        let manual_responses =
            substrate::watch_for_responses(&round_dir, &manual_participants, timeout)?;

        let received: Vec<&String> = manual_responses.keys().collect();
        let missing: Vec<&String> = manual_participants
            .iter()
            .filter(|n| !manual_responses.contains_key(*n))
            .collect();

        if !missing.is_empty() {
            eprintln!("  Timed out waiting for: {:?}", missing);
        }
        if !received.is_empty() {
            eprintln!("  Received from: {:?}", received);
        }

        responses.extend(manual_responses);
    }

    Ok(responses)
}

fn generate_prompt(
    config: &ForumConfig,
    _round: u32,
    stage: &Stage,
    prior_rounds: &[RoundData],
) -> Result<String> {
    match stage {
        Stage::Proposal => Ok(generate_proposal_prompt(config)),
        Stage::CrossExam => generate_crossexam_prompt(config, prior_rounds),
        Stage::Revision => generate_revision_prompt(config, prior_rounds),
    }
}

fn generate_proposal_prompt(config: &ForumConfig) -> String {
    format!(
        "# Forum Topic\n\n\
         {}\n\n\
         ## Instructions\n\n\
         You are participating in a structured deliberation. \
         Provide your independent analysis and proposal for the topic above.\n\n\
         Consider:\n\
         - Key factors and tradeoffs\n\
         - Your recommended approach with clear reasoning\n\
         - Potential risks and mitigations\n\
         - Specific evidence or examples supporting your position\n\n\
         Write your response in clear, structured markdown.\n",
        config.forum.topic
    )
}

fn generate_crossexam_prompt(
    config: &ForumConfig,
    prior_rounds: &[RoundData],
) -> Result<String> {
    let round1 = prior_rounds
        .last()
        .ok_or_else(|| anyhow::anyhow!("No prior round data for cross-examination"))?;

    // Assign cross-exam pairs
    let assignments = assign_cross_exam(&config.participants.names);

    let mut prompt = format!(
        "# Forum Topic\n\n{}\n\n## Round 1 Responses\n",
        config.forum.topic
    );

    for name in &config.participants.names {
        if let Some(response) = round1.responses.get(name) {
            prompt.push_str(&format!("\n### {}\n{}\n", name, response));
        }
    }

    if let Some(ref synth) = round1.synthesis {
        prompt.push_str(&format!("\n## Round 1 Synthesis\n{}\n", synth));
    }

    prompt.push_str("\n## Cross-Examination Assignments\n\n");
    for (critic, target) in &assignments {
        prompt.push_str(&format!("- **{}** critiques **{}**\n", critic, target));
    }

    prompt.push_str(
        "\n## Instructions\n\n\
         Find YOUR name in the assignments above.\n\n\
         1. **Critique**: Examine your assigned participant's position. \
         Find weaknesses, gaps, contradictions, or unstated assumptions.\n\
         2. **Defend/Revise**: Reconsider your own Round 1 position in light of ALL responses. \
         Defend it, revise it, or adopt elements from others.\n\n\
         Structure your response as:\n\
         ### Critique\n\
         ...\n\
         ### Revised Position\n\
         ...\n",
    );

    Ok(prompt)
}

fn generate_revision_prompt(
    config: &ForumConfig,
    prior_rounds: &[RoundData],
) -> Result<String> {
    let last = prior_rounds
        .last()
        .ok_or_else(|| anyhow::anyhow!("No prior round data for revision"))?;

    let mut prompt = format!("# Forum Topic\n\n{}\n\n", config.forum.topic);

    if let Some(ref synth) = last.synthesis {
        prompt.push_str(&format!("## Previous Round Synthesis\n{}\n\n", synth));
    }

    prompt.push_str("## Previous Round Responses\n");
    for name in &config.participants.names {
        if let Some(response) = last.responses.get(name) {
            prompt.push_str(&format!("\n### {}\n{}\n", name, response));
        }
    }

    prompt.push_str(
        "\n## Instructions\n\n\
         Based on the discussion so far, provide your FINAL revised position.\n\n\
         Consider:\n\
         - Points raised in critiques\n\
         - Areas of agreement you want to reinforce\n\
         - Disagreements you want to address directly\n\
         - Your updated recommendation\n\n\
         Be specific about what you've changed and why, or why you're holding firm.\n\
         Write in clear markdown.\n",
    );

    Ok(prompt)
}

/// Assign cross-examination: shuffle participants, each critiques the next
fn assign_cross_exam(participants: &[String]) -> Vec<(String, String)> {
    let n = participants.len();
    if n < 2 {
        return Vec::new();
    }

    let mut shuffled = participants.to_vec();
    let mut rng = rand::thread_rng();
    shuffled.shuffle(&mut rng);

    shuffled
        .iter()
        .enumerate()
        .map(|(i, critic)| {
            let target = &shuffled[(i + 1) % n];
            (critic.clone(), target.clone())
        })
        .collect()
}

fn write_final_output(
    config: &ForumConfig,
    forum_path: &Path,
    rounds: &[RoundData],
    convergence_result: &ConvergenceResult,
) -> Result<()> {
    let final_dir = substrate::create_final_dir(forum_path)?;

    // Final synthesis — use last round's or the best available
    if let Some(last) = rounds.last() {
        if let Some(ref synth) = last.synthesis {
            substrate::write_atomic(&final_dir.join("synthesis.md"), synth)?;
        }
        if let Some(ref claims) = last.claims {
            substrate::write_atomic_toml(&final_dir.join("claims.toml"), claims)?;
        }
    }

    // Dissent document
    match convergence_result {
        ConvergenceResult::Divergent {
            key_disagreements, ..
        } => {
            let last_responses = rounds
                .last()
                .map(|r| &r.responses)
                .cloned()
                .unwrap_or_default();
            let dissent = synthesis::generate_dissent(
                &config.synthesis,
                &config.forum.topic,
                &last_responses,
                key_disagreements,
            )?;
            substrate::write_atomic(&final_dir.join("dissent.md"), &dissent)?;
        }
        ConvergenceResult::Converged { .. } => {
            substrate::write_atomic(
                &final_dir.join("dissent.md"),
                "# Dissent\n\nNo unresolved disagreements — forum reached consensus.\n",
            )?;
        }
    }

    // Meta summary
    let (status, score) = match convergence_result {
        ConvergenceResult::Converged { score, .. } => ("converged", *score),
        ConvergenceResult::Divergent { score, .. } => ("divergent", *score),
    };
    let meta_summary = format!(
        "[summary]\n\
         status = \"{}\"\n\
         final_score = {:.1}\n\
         total_rounds = {}\n\
         participants = {}\n",
        status,
        score,
        rounds.len(),
        rounds.last().map_or(0, |r| r.responses.len()),
    );
    substrate::write_atomic_toml(&final_dir.join("meta-summary.toml"), &meta_summary)?;

    eprintln!(
        "\n=== Final output written to {}/final/ ===",
        forum_path.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assign_cross_exam_all_assigned() {
        let participants = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];
        let assignments = assign_cross_exam(&participants);
        assert_eq!(assignments.len(), 3);

        // Every participant appears exactly once as critic
        let mut critics: Vec<&str> = assignments.iter().map(|(c, _)| c.as_str()).collect();
        critics.sort();
        critics.dedup();
        assert_eq!(critics.len(), 3);

        // No one critiques themselves
        for (critic, target) in &assignments {
            assert_ne!(critic, target);
        }
    }

    #[test]
    fn test_assign_cross_exam_two_participants() {
        let participants = vec!["alice".to_string(), "bob".to_string()];
        let assignments = assign_cross_exam(&participants);
        assert_eq!(assignments.len(), 2);
        for (critic, target) in &assignments {
            assert_ne!(critic, target);
        }
    }

    #[test]
    fn test_assign_cross_exam_single_participant() {
        let participants = vec!["solo".to_string()];
        let assignments = assign_cross_exam(&participants);
        assert!(assignments.is_empty());
    }

    #[test]
    fn test_generate_proposal_prompt() {
        let config = make_test_config("Should we use Rust?");
        let prompt = generate_proposal_prompt(&config);
        assert!(prompt.contains("Should we use Rust?"));
        assert!(prompt.contains("Instructions"));
        assert!(prompt.contains("independent analysis"));
    }

    #[test]
    fn test_generate_crossexam_prompt() {
        let config = make_test_config("Rust vs Go?");
        let mut responses = HashMap::new();
        responses.insert("alice".to_string(), "Rust is great".to_string());
        responses.insert("bob".to_string(), "Go is simpler".to_string());

        let prior = vec![RoundData {
            number: 1,
            stage: Stage::Proposal,
            responses,
            synthesis: Some("Both have merits".to_string()),
            claims: None,
        }];

        let prompt = generate_crossexam_prompt(&config, &prior).unwrap();
        assert!(prompt.contains("Rust vs Go?"));
        assert!(prompt.contains("Rust is great"));
        assert!(prompt.contains("Go is simpler"));
        assert!(prompt.contains("Cross-Examination Assignments"));
        assert!(prompt.contains("Critique"));
    }

    #[test]
    fn test_generate_revision_prompt() {
        let config = make_test_config("Topic?");
        let mut responses = HashMap::new();
        responses.insert("alice".to_string(), "Revised position".to_string());

        let prior = vec![RoundData {
            number: 2,
            stage: Stage::CrossExam,
            responses,
            synthesis: Some("Synthesis from round 2".to_string()),
            claims: None,
        }];

        let prompt = generate_revision_prompt(&config, &prior).unwrap();
        assert!(prompt.contains("Topic?"));
        assert!(prompt.contains("Synthesis from round 2"));
        assert!(prompt.contains("FINAL revised position"));
    }

    fn make_test_config(topic: &str) -> ForumConfig {
        ForumConfig {
            forum: ForumSection {
                id: "test".into(),
                topic: topic.into(),
                created: "2026-03-27".into(),
                max_rounds: 3,
                protocol: "delphi-crossexam".into(),
            },
            participants: ParticipantsSection {
                names: vec!["alice".into(), "bob".into()],
                configs: HashMap::from([
                    (
                        "alice".into(),
                        ParticipantConfig {
                            participant_type: "manual".into(),
                            command: None,
                        },
                    ),
                    (
                        "bob".into(),
                        ParticipantConfig {
                            participant_type: "manual".into(),
                            command: None,
                        },
                    ),
                ]),
            },
            timing: TimingSection::default(),
            convergence: ConvergenceSection::default(),
            synthesis: SynthesisSection::default(),
        }
    }
}
