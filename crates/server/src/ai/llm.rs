//! The providers' OpenAI-compatible chat APIs: what to send each of them, and reading the answer.
//! See `docs/ai.md#模型配置`.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "AiThinking")]
pub enum Thinking {
    Off,
    Low,
    High,
}

impl Thinking {
    pub fn as_str(self) -> &'static str {
        match self {
            Thinking::Off => "off",
            Thinking::Low => "low",
            Thinking::High => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Thinking> {
        [Thinking::Off, Thinking::Low, Thinking::High].into_iter().find(|t| t.as_str() == s)
    }
}

/// How a provider is told how much to think.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AiThinkingParam")]
pub enum ThinkingParam {
    /// `thinking: {type}`, and `reasoning_effort` when on.
    Toggle,
    /// `enable_thinking`.
    Enable,
    /// `reasoning_effort` alone.
    Effort,
    /// Nothing is sent: the model thinks as it does by default.
    None,
}

/// When a provider takes `response_format: {type: "json_object"}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "AiJsonMode")]
pub enum JsonMode {
    Always,
    WithoutThinking,
    Never,
}

#[derive(Debug, Serialize, TS)]
#[ts(export, rename = "AiProvider")]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// Empty for one of the user's own.
    pub base_url: &'static str,
    pub model: &'static str,
    pub thinking: ThinkingParam,
    pub json: JsonMode,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "ecnu",
        name: "ECNU",
        base_url: "https://chat.ecnu.edu.cn/open/api/v1",
        model: "ecnu-max",
        thinking: ThinkingParam::Toggle,
        json: JsonMode::Always,
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        model: "deepseek-flash",
        thinking: ThinkingParam::Toggle,
        json: JsonMode::Always,
    },
    Provider {
        id: "qwen",
        name: "通义千问",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        model: "qwen-plus",
        thinking: ThinkingParam::Enable,
        json: JsonMode::WithoutThinking,
    },
    Provider {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-5-mini",
        thinking: ThinkingParam::Effort,
        json: JsonMode::Always,
    },
    Provider { id: "custom", name: "自定义", base_url: "", model: "", thinking: ThinkingParam::None, json: JsonMode::Never },
];

/// What to call a provider with.
#[derive(Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "AiConfig")]
pub struct Config {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub thinking: Thinking,
    pub api_key: String,
}

/// One line of printable text without spaces, as addresses, model names and keys are.
fn word(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl Config {
    /// The provider, once the rest is checked too.
    pub fn check(&self) -> Result<&'static Provider, String> {
        let p = PROVIDERS.iter().find(|p| p.id == self.provider).ok_or("未知的服务商")?;
        if !self.base_url.starts_with("https://") || !word(&self.base_url, 300) {
            return Err("接口地址要以 https:// 开头，不含空格".into());
        }
        if !word(&self.model, 100) {
            return Err("模型名称不能为空，不含空格，最多 100 个字符".into());
        }
        if !word(&self.api_key, 500) {
            return Err("API 密钥不能为空，不含空格".into());
        }
        Ok(p)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
    }
}

/// A chat: the instructions, and what the user gives.
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// Why a call failed, in words for the user; with the HTTP status when the API answered one.
#[derive(Debug)]
pub struct Failure {
    pub status: Option<u16>,
    pub message: String,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Failure { status: None, message: message.into() }
    }
}

/// Where chats go: the provider's API, or a stand-in in tests. Calls block.
pub trait Api: Send + Sync {
    /// The model's answer, with `json` when the prompt asks for JSON.
    fn chat(&self, c: &Config, prompt: &Prompt, json: bool, timeout: Duration) -> Result<String, Failure>;
    fn models(&self, c: &Config) -> Result<Vec<String>, Failure>;
}

/// The request body for `c`'s provider: its way to set the thinking level, and JSON mode where it
/// has one that goes with that level.
pub fn body(c: &Config, p: &Provider, prompt: &Prompt, json: bool) -> Value {
    let mut b = json!({
        "model": c.model,
        "messages": [{ "role": "system", "content": prompt.system }, { "role": "user", "content": prompt.user }],
        "stream": true,
    });
    let effort = |t: Thinking| match t {
        Thinking::Off => "minimal",
        Thinking::Low => "low",
        Thinking::High => "high",
    };
    match (p.thinking, c.thinking) {
        // DeepSeek thinks unless told not to.
        (ThinkingParam::Toggle, Thinking::Off) => b["thinking"] = json!({ "type": "disabled" }),
        (ThinkingParam::Toggle, t) => {
            b["thinking"] = json!({ "type": "enabled" });
            b["reasoning_effort"] = effort(t).into();
        }
        (ThinkingParam::Enable, t) => b["enable_thinking"] = (t != Thinking::Off).into(),
        (ThinkingParam::Effort, t) => b["reasoning_effort"] = effort(t).into(),
        (ThinkingParam::None, _) => {}
    }
    let json_mode = match p.json {
        JsonMode::Always => true,
        JsonMode::WithoutThinking => c.thinking == Thinking::Off,
        JsonMode::Never => false,
    };
    if json && json_mode {
        b["response_format"] = json!({ "type": "json_object" });
    }
    b
}

/// The API over HTTPS. It is the user's own address the server calls, so: HTTPS only, no
/// redirects, and only so much read back.
pub struct Https;

/// How much of the model's answer is taken.
const MAX_ANSWER: usize = 1 << 20;
/// How much is read from a stream, which repeats a lot around the few characters of each piece.
const MAX_STREAM: u64 = 64 << 20;
/// How much of an error answer is read.
const MAX_ERROR: u64 = 64 << 10;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().https_only(true).max_redirects(0).http_status_as_error(false).timeout_global(Some(timeout)).build().into()
}

fn transport(e: ureq::Error) -> Failure {
    Failure::new(match e {
        ureq::Error::Timeout(_) => "接口超时".to_owned(),
        ureq::Error::BodyExceedsLimit(_) => "接口的回复过长".to_owned(),
        e => format!("无法连接接口：{e}"),
    })
}

/// The message in an API's error answer, shortened: OpenAI's `error.message`, or what others write,
/// such as ECNU's `detail`.
fn api_message(v: &Value) -> String {
    let message = [&v["error"]["message"], &v["error"], &v["message"], &v["detail"], &v["detail"][0]["msg"]]
        .into_iter()
        .find_map(Value::as_str)
        .unwrap_or_default();
    message.chars().take(200).collect()
}

/// The error an API answered with: its message, or just the status.
fn refused(status: u16, body: &str) -> Failure {
    let message = api_message(&serde_json::from_str(body).unwrap_or_default());
    Failure {
        status: Some(status),
        message: if message.is_empty() { format!("接口返回 {status}") } else { format!("接口返回 {status}：{message}") },
    }
}

impl Https {
    fn send(&self, c: &Config, req: ureq::RequestBuilder<ureq::typestate::WithBody>, body: &Value) -> Result<ureq::Body, Failure> {
        let res = req
            .header("Authorization", format!("Bearer {}", c.api_key))
            .content_type("application/json")
            .send(body.to_string())
            .map_err(transport)?;
        Self::accepted(res)
    }

    fn accepted(res: ureq::http::Response<ureq::Body>) -> Result<ureq::Body, Failure> {
        let status = res.status().as_u16();
        let mut body = res.into_body();
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let text = body.with_config().limit(MAX_ERROR).lossy_utf8(true).read_to_string().unwrap_or_default();
        Err(refused(status, &text))
    }
}

impl Api for Https {
    fn chat(&self, c: &Config, prompt: &Prompt, json: bool, timeout: Duration) -> Result<String, Failure> {
        let p = c.check().map_err(Failure::new)?;
        let body = self.send(c, agent(timeout).post(c.url("/chat/completions")), &body(c, p, prompt, json))?;
        let streamed = body.mime_type() == Some("text/event-stream");
        let reader = body.into_with_config().limit(MAX_STREAM).reader();
        let answer = if streamed { read_stream(BufReader::new(reader))? } else { read_whole(reader)? };
        if answer.len() > MAX_ANSWER {
            return Err(Failure::new("模型的回复过长"));
        }
        Ok(answer)
    }

    fn models(&self, c: &Config) -> Result<Vec<String>, Failure> {
        c.check().map_err(Failure::new)?;
        let res = agent(Duration::from_secs(20))
            .get(c.url("/models"))
            .header("Authorization", format!("Bearer {}", c.api_key))
            .call()
            .map_err(transport)?;
        let text = Self::accepted(res)?.with_config().limit(MAX_ERROR * 16).read_to_string().map_err(transport)?;
        let v: Value = serde_json::from_str(&text).map_err(|_| Failure::new("接口返回的模型列表无法识别"))?;
        let mut ids: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(str::to_owned)).collect();
        ids.sort();
        Ok(ids)
    }
}

fn unreadable() -> Failure {
    Failure::new("接口返回的内容无法识别")
}

/// What a stream of server-sent events adds up to.
fn read_stream(reader: impl BufRead) -> Result<String, Failure> {
    let mut answer = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| Failure::new(format!("读取接口回复时出错：{e}")))?;
        let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
        if data == "[DONE]" {
            break;
        }
        let v: Value = serde_json::from_str(data).map_err(|_| unreadable())?;
        if !v["error"].is_null() {
            return Err(Failure::new(format!("接口报错：{}", api_message(&v))));
        }
        if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
            answer.push_str(piece);
            if answer.len() > MAX_ANSWER {
                return Err(Failure::new("模型的回复过长"));
            }
        }
    }
    Ok(answer)
}

/// The answer of an API that does not stream.
fn read_whole(mut reader: impl std::io::Read) -> Result<String, Failure> {
    let mut text = String::new();
    reader.read_to_string(&mut text).map_err(|e| Failure::new(format!("读取接口回复时出错：{e}")))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| unreadable())?;
    v["choices"][0]["message"]["content"].as_str().map(str::to_owned).ok_or_else(unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: &str, thinking: Thinking) -> Config {
        Config { provider: provider.into(), base_url: "https://x.test/v1".into(), model: "m".into(), thinking, api_key: "k".into() }
    }

    fn sent(provider: &str, thinking: Thinking) -> Value {
        let c = config(provider, thinking);
        let mut b = body(&c, c.check().unwrap(), &Prompt { system: "s".into(), user: "u".into() }, true);
        b.as_object_mut().unwrap().retain(|k, _| !["model", "messages", "stream"].contains(&k.as_str()));
        b
    }

    #[test]
    fn each_provider_gets_its_own_thinking_and_json_parameters() {
        let fmt = json!({ "type": "json_object" });
        assert_eq!(sent("deepseek", Thinking::Off), json!({ "thinking": { "type": "disabled" }, "response_format": fmt }));
        assert_eq!(
            sent("ecnu", Thinking::High),
            json!({ "thinking": { "type": "enabled" }, "reasoning_effort": "high", "response_format": fmt })
        );
        assert_eq!(sent("qwen", Thinking::Off), json!({ "enable_thinking": false, "response_format": fmt }));
        assert_eq!(sent("qwen", Thinking::Low), json!({ "enable_thinking": true }), "no JSON mode while thinking");
        assert_eq!(sent("openai", Thinking::Off), json!({ "reasoning_effort": "minimal", "response_format": fmt }));
        assert_eq!(sent("custom", Thinking::High), json!({}));
    }

    #[test]
    fn checks_the_config() {
        assert!(config("ecnu", Thinking::Off).check().is_ok());
        assert!(config("anthropic", Thinking::Off).check().is_err());
        for url in ["http://x.test/v1", "https://x.test/ v1", "ftp://x"] {
            assert!(Config { base_url: url.into(), ..config("custom", Thinking::Off) }.check().is_err(), "{url}");
        }
        assert!(Config { api_key: "a b".into(), ..config("ecnu", Thinking::Off) }.check().is_err());
        assert!(Config { model: String::new(), ..config("ecnu", Thinking::Off) }.check().is_err());
    }

    #[test]
    fn reads_streams_and_errors() {
        let stream = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}\n\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"a\\\"\"}}]}\n\
                      : keep-alive\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\": 1}\"}}]}\n\
                      data: [DONE]\n";
        assert_eq!(read_stream(stream.as_bytes()).unwrap(), "{\"a\": 1}");
        let failed = read_stream("data: {\"error\":{\"message\":\"余额不足\"}}\n".as_bytes()).unwrap_err();
        assert_eq!(failed.message, "接口报错：余额不足");
        assert_eq!(refused(401, "{\"error\":{\"message\":\"invalid key\"}}").message, "接口返回 401：invalid key");
        assert_eq!(refused(502, "<html>").message, "接口返回 502");
        assert_eq!(refused(401, "{\"detail\":\"token 无效或已过期\"}").message, "接口返回 401：token 无效或已过期");
        assert_eq!(read_whole("{\"choices\":[{\"message\":{\"content\":\"好\"}}]}".as_bytes()).unwrap(), "好");
    }
}
