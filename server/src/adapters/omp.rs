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
        a.extend(["--session-dir".into(), input.state_dir.join("omp-sessions").display().to_string()]);
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
        a.extend(["--".into(), if p.starts_with('@') { format!(" {p}") } else { p.clone() }]);
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
                        "text" if !str_of(c, "text").trim().is_empty() => out.push(AgentEvent::Text(str_of(c, "text").into())),
                        "toolCall" => {
                            // `intent` is the model's one-line reason; fall back to the arguments.
                            // It often starts with the same verb tool_label adds, so drop that once.
                            let detail = match str_of(c, "intent") {
                                "" => describe(&c["arguments"]),
                                i => {
                                    let i = i.trim();
                                    let i = i.strip_prefix("Running ").or_else(|| i.strip_prefix("Ran ")).unwrap_or(i);
                                    short(i)
                                }
                            };
                            out.push(AgentEvent::Tool { name: str_of(c, "name").into(), detail });
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
                let msg = e["error"]["message"].as_str().or(e["message"].as_str()).unwrap_or("omp failed");
                out.push(AgentEvent::Error(msg.into()));
            }
            _ => {}
        }
    }

    /// From `omp models --json`: the models this user can run, with their thinking levels.
    fn models(&self) -> Vec<ModelOption> {
        let v: Value = serde_json::from_str(&super::output("omp", &["models", "--json"])).unwrap_or_default();
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
                    efforts: m["thinking"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect(),
                    aliases: vec![],
                }
            })
            .collect()
    }
}
