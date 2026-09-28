//! Anonymous views (PRD 5.3): render a paper as text for another model, then suppress
//! seat labels, provider names and model IDs with a documented, deterministic transform.
//! No paraphrasing; only identity strings are replaced.

use rt_core::FrozenSeat;
use serde_json::Value;

const REPLACEMENT: &str = "another model";

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Replace whole-word, case-insensitive occurrences of `token` in `text`.
/// Returns the new text and whether a non-word-bounded occurrence remained (possible identity).
fn replace_token(text: &str, token: &str) -> (String, bool) {
    if token.is_empty() {
        return (text.to_string(), false);
    }
    let lower = text.to_lowercase();
    let tl = token.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut leftover = false;
    let mut search = 0;
    // byte offsets in `lower` align with `text` for ASCII tokens; for safety we work on char indices via find on lowercase copy
    while let Some(pos) = lower[search..].find(&tl) {
        let start = search + pos;
        let end = start + tl.len();
        let before_ok = start == 0 || !text[..start].chars().next_back().map(is_word_char).unwrap_or(false);
        let after_ok = end >= text.len() || !text[end..].chars().next().map(is_word_char).unwrap_or(false);
        if before_ok && after_ok {
            out.push_str(&text[last..start]);
            out.push_str(REPLACEMENT);
            last = end;
        } else {
            leftover = true;
        }
        search = end;
    }
    out.push_str(&text[last..]);
    (out, leftover)
}

/// Identity tokens for a run: labels, providers, model IDs, and the bare model family before a dash.
pub fn identity_tokens(seats: &[FrozenSeat]) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for s in seats {
        for t in [s.label.as_str(), s.provider.as_str(), s.model_id.as_str()] {
            let t = t.trim();
            if t.len() >= 3 && !tokens.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                tokens.push(t.to_string());
            }
        }
    }
    // longest first so "gpt-4o-mini" is handled before "gpt"
    tokens.sort_by_key(|t| std::cmp::Reverse(t.len()));
    tokens
}

/// Apply the transform. Returns (text, possibly_identifiable).
pub fn suppress_identity(text: &str, seats: &[FrozenSeat]) -> (String, bool) {
    let mut cur = text.to_string();
    let mut flagged = false;
    for tok in identity_tokens(seats) {
        let (next, leftover) = replace_token(&cur, &tok);
        cur = next;
        flagged |= leftover;
    }
    (cur, flagged)
}

fn list(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// Render an answer paper as the reviewer sees it.
pub fn render_answer(body: &Value) -> String {
    let mut s = String::new();
    s.push_str(body["answer"].as_str().unwrap_or_default());
    s.push_str("\n\nCLAIMS\n");
    for c in body["claims"].as_array().map(|a| a.iter()).into_iter().flatten() {
        let refs = list(c, "references");
        let refs = if refs.is_empty() { String::new() } else { format!(" (refs: {})", refs.join(", ")) };
        s.push_str(&format!("- {}: {}{}\n", c["id"].as_str().unwrap_or("?"), c["text"].as_str().unwrap_or(""), refs));
    }
    let assumptions = list(body, "assumptions");
    if !assumptions.is_empty() {
        s.push_str("\nASSUMPTIONS\n");
        for a in assumptions {
            s.push_str(&format!("- {a}\n"));
        }
    }
    let limits = list(body, "limitations");
    if !limits.is_empty() {
        s.push_str("\nLIMITATIONS\n");
        for l in limits {
            s.push_str(&format!("- {l}\n"));
        }
    }
    s
}

/// Render a review as the improving seat sees it (issues with IDs so decisions can reference them).
pub fn render_review(body: &Value) -> String {
    let mut s = String::new();
    let correct = list(body, "correct_claim_ids");
    if !correct.is_empty() {
        s.push_str(&format!("Claims judged correct: {}\n", correct.join(", ")));
    }
    let issues = body["issues"].as_array().map(|a| a.iter()).into_iter().flatten().collect::<Vec<_>>();
    if issues.is_empty() {
        s.push_str("No material issue found.\n");
    } else {
        s.push_str("ISSUES\n");
        for i in issues {
            let target = i.get("target_claim_id").and_then(Value::as_str).map(|t| format!(" on {t}")).unwrap_or_default();
            s.push_str(&format!("- {}{}: {}", i["id"].as_str().unwrap_or("?"), target, i["explanation"].as_str().unwrap_or("")));
            if let Some(fix) = i.get("suggested_correction").and_then(Value::as_str) {
                s.push_str(&format!(" Suggested: {fix}"));
            }
            s.push('\n');
        }
    }
    let missing = list(body, "missing");
    if !missing.is_empty() {
        s.push_str("MISSING\n");
        for m in missing {
            s.push_str(&format!("- {m}\n"));
        }
    }
    if body["needs_external_verification"].as_bool().unwrap_or(false) {
        s.push_str("The reviewer flagged that external verification is needed.\n");
    }
    s
}

/// Render an improved answer for the synthesizer: revised answer, claims and unresolved issues.
/// Excludes the decision audit and self-rating (PRD 5.3).
pub fn render_improved_for_synthesis(body: &Value) -> String {
    let mut s = String::new();
    s.push_str(body["answer"].as_str().unwrap_or_default());
    s.push_str("\n\nCLAIMS\n");
    for c in body["claims"].as_array().map(|a| a.iter()).into_iter().flatten() {
        s.push_str(&format!("- {}: {}\n", c["id"].as_str().unwrap_or("?"), c["text"].as_str().unwrap_or("")));
    }
    let unresolved = list(body, "unresolved");
    if !unresolved.is_empty() {
        s.push_str("\nUNRESOLVED\n");
        for u in unresolved {
            s.push_str(&format!("- {u}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_whole_words_case_insensitively_and_flags_partial_matches() {
        let (t, flagged) = replace_token("GPT says gpt is best. ChatGPT too.", "GPT");
        assert_eq!(t, "another model says another model is best. ChatGPT too.");
        assert!(flagged, "ChatGPT contains the token inside a word");
        let (t, flagged) = replace_token("nothing here", "GPT");
        assert_eq!(t, "nothing here");
        assert!(!flagged);
    }

    #[test]
    fn handles_hyphenated_model_ids_longest_first() {
        use rt_core::{CredentialRef, MicroUsd, RoleSettings, SeatLimits};
        let rs = RoleSettings { temperature: None, max_output_tokens: 1, reasoning_effort: None, web_search: false };
        let seat = FrozenSeat { seat_index: 0, label: "GPT".into(), provider: "OpenAI".into(), credential_ref: CredentialRef::new(), model_id: "gpt-4o-mini".into(), model_is_alias: false, answer: rs.clone(), review: rs.clone(), improve: rs, prompt_answer: "".into(), prompt_review: "".into(), prompt_improve: "".into(), limits: SeatLimits { cost_cap: MicroUsd::ZERO } };
        let (t, flagged) = suppress_identity("Use gpt-4o-mini from OpenAI, not GPT.", &[seat]);
        assert_eq!(t, "Use another model from another model, not another model.");
        assert!(!flagged);
    }

    #[test]
    fn synthesis_view_excludes_decisions() {
        let body = serde_json::json!({"decisions":[{"review_alias":"R-x","issue_id":"I1","decision":"rejected","reason":"SECRET"}],"answer":"A","claims":[{"id":"C1","text":"t"}],"unresolved":["u"]});
        let s = render_improved_for_synthesis(&body);
        assert!(!s.contains("SECRET"));
        assert!(s.contains("UNRESOLVED"));
    }
}
