//! Model client: Jev, a decision model, through the provider the user signed up with.
//!
//! | provider  | endpoint                                       | model             | boolean type |
//! |-----------|------------------------------------------------|-------------------|--------------|
//! | TypeSafe  | `https://api.typesafe.ai/v1/systemone`         | `jev-latest`      | `noul`       |
//! | Vercel    | `https://ai-gateway.vercel.sh/v1/evaluate`     | `typesafe-ai/jev` | `boolean`    |
//!
//! Questions are written once, gateway-style (`boolean`), and translated per provider; answers
//! come back normalized ([`Answer`]). [`Client::ask_batched`] puts many items in one call: the
//! state holds each item under its key and each question names its item. About 60 items fit
//! (the limit is the state size); 40 leaves room.

use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    TypeSafe,
    Vercel,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Provider> {
        match s.to_lowercase().as_str() {
            "typesafe" | "direct" => Some(Provider::TypeSafe),
            "vercel" | "gateway" => Some(Provider::Vercel),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::TypeSafe => "typesafe",
            Provider::Vercel => "vercel",
        }
    }

    /// Environment variables checked for the key, in order.
    pub fn key_env(self) -> &'static [&'static str] {
        match self {
            Provider::TypeSafe => &["TIDYWARD_API_KEY", "TYPESAFE_API_KEY"],
            Provider::Vercel => &["TIDYWARD_API_KEY", "AI_GATEWAY_API_KEY"],
        }
    }

    fn url(self) -> &'static str {
        match self {
            Provider::TypeSafe => "https://api.typesafe.ai/v1/systemone",
            Provider::Vercel => "https://ai-gateway.vercel.sh/v1/evaluate",
        }
    }

    fn model(self) -> &'static str {
        match self {
            Provider::TypeSafe => "jev-latest",
            Provider::Vercel => "typesafe-ai/jev",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Bool(f64),
    Choice { choice: String, confidence: f64 },
}

impl Answer {
    pub fn probability(&self) -> f64 {
        match self {
            Answer::Bool(p) => *p,
            Answer::Choice { confidence, .. } => *confidence,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallStat {
    pub items: usize,
    pub questions: usize,
    pub input_tokens: u64,
    pub ms: u64,
}

pub struct Client {
    provider: Provider,
    key: String,
    agent: ureq::Agent,
    pub workers: usize,
}

/// Translate gateway-style questions to what the provider expects.
pub fn to_provider(p: Provider, questions: &Map<String, Value>) -> Map<String, Value> {
    questions
        .iter()
        .map(|(k, q)| {
            let mut q = q.clone();
            if p == Provider::TypeSafe && q["type"] == "boolean" {
                q["type"] = json!("noul");
            }
            (k.clone(), q)
        })
        .collect()
}

/// Normalize a response into answers + input tokens.
pub fn parse_response(p: Provider, d: &Value) -> Result<(HashMap<String, Answer>, u64)> {
    let answers = d["answers"].as_object().ok_or_else(|| anyhow!("no answers in response: {d}"))?;
    let mut out = HashMap::new();
    for (k, a) in answers {
        let ans = match a["type"].as_str() {
            Some("choice") => Answer::Choice {
                choice: a["choice"].as_str().unwrap_or_default().to_string(),
                confidence: a["confidence"].as_f64().unwrap_or(0.0),
            },
            Some("noul") => Answer::Bool(a["noul"].as_f64().unwrap_or(0.0)),
            _ => Answer::Bool(a["probability"].as_f64().unwrap_or(0.0)),
        };
        out.insert(k.clone(), ans);
    }
    let usage = &d["usage"];
    let tokens = match p {
        Provider::TypeSafe => usage["input_tokens"].as_u64(),
        Provider::Vercel => usage["inputTokens"].as_u64(),
    }
    .or_else(|| usage["input_tokens"].as_u64())
    .or_else(|| usage["inputTokens"].as_u64())
    .unwrap_or(0);
    Ok((out, tokens))
}

/// Answers per item key, then per question key.
pub type ItemAnswers = HashMap<String, HashMap<String, Answer>>;
type CallResult = Result<(usize, HashMap<String, Answer>, CallStat)>;

pub struct Job {
    pub key: String,
    pub state: Map<String, Value>,
    pub questions: Map<String, Value>,
}

impl Client {
    pub fn new(provider: Provider, key: String) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .into();
        Client { provider, key, agent, workers: 4 }
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// One call; retried up to 3 times on network errors and 429/5xx.
    pub fn ask(&self, state: &Value, questions: &Map<String, Value>) -> Result<(HashMap<String, Answer>, CallStat)> {
        let body = json!({"model": self.provider.model(), "state": state, "questions": to_provider(self.provider, questions)});
        let mut last = String::new();
        for attempt in 0..3 {
            let t0 = Instant::now();
            let res = self
                .agent
                .post(self.provider.url())
                .header("Authorization", &format!("Bearer {}", self.key))
                .header("Content-Type", "application/json")
                .send_json(&body);
            match res {
                Ok(mut r) => {
                    let status = r.status().as_u16();
                    let text = r.body_mut().read_to_string().unwrap_or_default();
                    if status == 200 {
                        let d: Value = serde_json::from_str(&text)?;
                        let (answers, tokens) = parse_response(self.provider, &d)?;
                        let stat = CallStat { items: 1, questions: questions.len(), input_tokens: tokens, ms: t0.elapsed().as_millis() as u64 };
                        return Ok((answers, stat));
                    }
                    last = format!("HTTP {status}: {}", text.chars().take(250).collect::<String>());
                    if status != 429 && status < 500 {
                        bail!("{} error {last}", self.provider.name());
                    }
                }
                Err(e) => last = e.to_string(),
            }
            std::thread::sleep(Duration::from_millis(1500 * (attempt + 1)));
        }
        bail!("{} error after 3 tries: {last}", self.provider.name())
    }

    /// Many items, `size` per call, `workers` calls at a time. Returns answers per item key and
    /// one [`CallStat`] per call.
    pub fn ask_batched(&self, jobs: &[Job], size: usize, note: &str) -> Result<(ItemAnswers, Vec<CallStat>)> {
        let groups: Vec<&[Job]> = jobs.chunks(size.max(1)).collect();
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<CallResult>> = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..self.workers.max(1).min(groups.len().max(1)) {
                s.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        let Some(g) = groups.get(i) else { break };
                        let mut state = Map::new();
                        state.insert("instructions".into(), json!(note));
                        let mut qs = Map::new();
                        for j in g.iter() {
                            state.insert(j.key.clone(), Value::Object(j.state.clone()));
                            for (k, q) in &j.questions {
                                let mut q = q.clone();
                                let instr = q["instructions"].as_str().unwrap_or_default().to_string();
                                q["instructions"] = json!(format!("Item {}: {instr}", j.key));
                                qs.insert(format!("{}.{k}", j.key), q);
                            }
                        }
                        let r = self.ask(&Value::Object(state), &qs).map(|(a, mut st)| {
                            st.items = g.len();
                            (i, a, st)
                        });
                        results.lock().unwrap().push(r);
                    }
                });
            }
        });
        let mut by_item: ItemAnswers = HashMap::new();
        let mut stats = Vec::new();
        for r in results.into_inner().unwrap() {
            let (_, answers, st) = r?;
            for (k, a) in answers {
                if let Some((item, q)) = k.split_once('.') {
                    by_item.entry(item.to_string()).or_default().insert(q.to_string(), a);
                }
            }
            stats.push(st);
        }
        for j in jobs {
            let got = by_item.get(&j.key).map(|m| m.len()).unwrap_or(0);
            if got < j.questions.len() {
                bail!("model returned {got} of {} answers for {}", j.questions.len(), j.key);
            }
        }
        Ok((by_item, stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_normalizes() {
        let q: Map<String, Value> = serde_json::from_value(json!({"a": {"type": "boolean", "instructions": "x"}})).unwrap();
        assert_eq!(to_provider(Provider::TypeSafe, &q)["a"]["type"], "noul");
        assert_eq!(to_provider(Provider::Vercel, &q)["a"]["type"], "boolean");

        let direct = json!({"answers": {"a": {"type": "noul", "noul": 0.8},
                                         "p": {"type": "choice", "choice": "photo", "confidence": 0.9}},
                            "usage": {"input_tokens": 120, "output_tokens": 5}});
        let (a, t) = parse_response(Provider::TypeSafe, &direct).unwrap();
        assert_eq!((a["a"].probability(), t), (0.8, 120));
        assert_eq!(a["p"], Answer::Choice { choice: "photo".into(), confidence: 0.9 });

        let gw = json!({"answers": {"a": {"type": "boolean", "probability": 0.3}}, "usage": {"inputTokens": 99}});
        let (a, t) = parse_response(Provider::Vercel, &gw).unwrap();
        assert_eq!((a["a"].probability(), t), (0.3, 99));
    }
}
