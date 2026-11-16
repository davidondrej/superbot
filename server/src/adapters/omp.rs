//! omp CLI: `omp -p --mode json`. Resumes with `-r <session id>`.
//! The system prompt is appended fresh every run. Verified against omp 18.20.3:
//! `session` carries the id, `message_end` carries assistant text and toolCall blocks.

use super::{describe, short, str_of, Adapter, AgentEvent, ModelOption, RunInput};
use serde_json::Value;

pub struct Omp;

impl Adapter for Omp {
    fn bin(&self) -> &'static str {
        "omp"
    }

    fn args(&mut self, input: &RunInput, _out: &mut Vec<AgentEvent>) -> Vec<String> {
        let mut a: Vec<String> = vec!["-p".into(), "--mode".into(), "json".into()];
        a.extend(["--auto-approve", "--no-title"].map(String::from));
        a.extend([
            "--session-dir".into(),
            input.state_dir.join("omp-sessions").display().to_string(),
        ]);
        a.extend(["--append-system-prompt".into(), input.system.clone()]);
        if let Some(m) = &input.model {
            a.extend(["--model".into(), m.clone()]);
        }
        if let Some(e) = &input.effort {
            a.extend(["--thinking".into(), e.clone()]);
        }
        if let Some(id) = &input.session_id {
            a.extend(["--resume".into(), id.clone()]);
        }
        // A leading "@" would make omp read a file, so pad it.
        let p = &input.prompt;
        a.extend([
            "--".into(),
            if p.starts_with('@') {
                format!(" {p}")
            } else {
                p.clone()
            },
        ]);
        a
    }

    fn on_event(&mut self, e: &Value, out: &mut Vec<AgentEvent>) {
        match str_of(e, "type") {
            "session" => {
                let id = str_of(e, "id");
                if !id.is_empty() {
                    out.push(AgentEvent::Session(id.into()));
                }
            }
            "message_update" => {
                let ev = &e["assistantMessageEvent"];
                if str_of(ev, "type") == "text_delta" {
                    out.push(AgentEvent::TextDelta(str_of(ev, "delta").into()));
                }
            }
            // One finished assistant message: text bubbles and tool calls. `message_start`
            // carries the same content early, so only this is used.
            "message_end" => {
                let m = &e["message"];
                if str_of(m, "role") != "assistant" {
                    return;
                }
                for c in m["content"].as_array().into_iter().flatten() {
                    match str_of(c, "type") {
                        "text" if !str_of(c, "text").trim().is_empty() => {
                            out.push(AgentEvent::Text(str_of(c, "text").into()))
                        }
                        "toolCall" => {
                            // `intent` is the model's one-line reason; fall back to the arguments.
                            // It often starts with the same verb tool_label adds, so drop that once.
                            let detail = match str_of(c, "intent") {
                                "" => describe(&c["arguments"]),
                                i => short(strip_verb(i)),
                            };
                            out.push(AgentEvent::Tool {
                                name: str_of(c, "name").into(),
                                detail,
                            });
                        }
                        _ => {}
                    }
                }
                if str_of(m, "stopReason") == "error" {
                    let msg = m["errorMessage"].as_str().unwrap_or("omp failed");
                    out.push(AgentEvent::Error(msg.into()));
                }
            }
            "error" => {
                let msg = e["error"]["message"]
                    .as_str()
                    .or(e["message"].as_str())
                    .unwrap_or("omp failed");
                out.push(AgentEvent::Error(msg.into()));
            }
            _ => {}
        }
    }

    /// From `omp models --json`: the models this user can run, with their thinking levels.
    fn models(&self) -> Vec<ModelOption> {
        let v: Value =
            serde_json::from_str(&super::output("omp", &["models", "--json"])).unwrap_or_default();
        v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| str_of(m, "kind") == "chat")
            .map(|m| {
                let selector = m["selector"].as_str().unwrap_or(str_of(m, "id"));
                ModelOption {
                    id: selector.into(),
                    label: m["name"].as_str().unwrap_or(selector).into(),
                    efforts: m["thinking"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect(),
                    aliases: vec![],
                }
            })
            .collect()
    }
}

/// Verbs the status line already supplies. omp's intent opens with one of these
/// ("Running uname", "Reading first three lines", "Writing ok.txt"), so drop it once
/// and only when a colon or space follows — "Random x" must survive.
const VERBS: [&str; 15] = [
    "Reading",
    "Running",
    "Writing",
    "Editing",
    "Searching",
    "Browsing",
    "Listing",
    "Fetching",
    "Creating",
    "Updating",
    "Deleting",
    "Read",
    "Ran",
    "Wrote",
    "Edited",
];

fn strip_verb(s: &str) -> &str {
    let s = s.trim();
    for verb in VERBS {
        let Some(rest) = s.strip_prefix(verb) else {
            continue;
        };
        if rest.starts_with(|c: char| c == ':' || c.is_whitespace()) {
            return rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::strip_verb;

    #[test]
    fn strips_one_verb_with_its_punctuation() {
        assert_eq!(strip_verb("Running: `echo hi`"), "`echo hi`");
        assert_eq!(strip_verb("Running echo hi"), "echo hi");
        assert_eq!(strip_verb("Ran: ls"), "ls");
        assert_eq!(strip_verb("Ran ls"), "ls");
        // The forms omp actually emitted while writing this: one verb per tool kind.
        assert_eq!(
            strip_verb("Running uname and listing largest /tmp entries"),
            "uname and listing largest /tmp entries"
        );
        assert_eq!(strip_verb("Reading first three lines"), "first three lines");
        assert_eq!(strip_verb("Writing ok.txt"), "ok.txt");
        assert_eq!(strip_verb("Read: /tmp/x"), "/tmp/x");
        assert_eq!(strip_verb("Running\tls"), "ls");
    }

    #[test]
    fn leaves_words_that_only_look_like_verbs() {
        assert_eq!(strip_verb("Random sample"), "Random sample");
        assert_eq!(strip_verb("Readinglist.txt"), "Readinglist.txt");
        assert_eq!(strip_verb("Inside /tmp"), "Inside /tmp");
        assert_eq!(strip_verb("Running late, so skip"), "late, so skip");
        assert_eq!(strip_verb("echo hi"), "echo hi");
    }
}
