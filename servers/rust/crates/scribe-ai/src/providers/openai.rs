//! OpenAI Chat Completions adapter.
//!
//! Also covers Ollama, LM Studio, and the generic `openai-compatible`
//! provider — all four speak the same wire format. The only difference
//! is the base URL (which lives on the config record and is required
//! for non-OpenAI variants).

use async_trait::async_trait;
use futures::stream::{BoxStream, Stream, StreamExt};
use scribe_shared::{AIProvider, ChatRole};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::adapter::{Adapter, AdapterError, CompleteRequest, CompletionChunk};

const OPENAI_BASE: &str = "https://api.openai.com";

pub struct OpenAIAdapter;

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<RequestMessage<'a>>,
    stream: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct RequestMessage<'a> {
    role: &'a str,
    content: MessageContent<'a>,
}

/// Either a plain string (text path, byte-identical to the previous
/// `&str` form) or an array of content parts when an image is attached.
#[derive(Serialize)]
#[serde(untagged)]
enum MessageContent<'a> {
    Text(&'a str),
    Parts(Vec<ContentPart<'a>>),
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ContentPart<'a> {
    #[serde(rename = "text")]
    Text { text: &'a str },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Deserialize)]
struct StreamFrame {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

fn chunk_from_frame(frame:StreamFrame)->Option<Result<CompletionChunk,AdapterError>>{
    let choice=frame.choices.into_iter().next()?;
    if choice.finish_reason.as_deref()==Some("length"){return Some(Err(AdapterError::OutputLimit));}
    let text=choice.delta.and_then(|delta|delta.content).unwrap_or_default();
    if text.is_empty(){None}else{Some(Ok(CompletionChunk{text}))}
}

fn base_url(req: &CompleteRequest) -> Result<String, AdapterError> {
    if let Some(b) = req.base_url.as_deref() {
        if !b.is_empty() {
            return Ok(b.trim_end_matches('/').trim_end_matches("/v1").to_string());
        }
    }
    if req.provider == AIProvider::Openai {
        return Ok(OPENAI_BASE.to_string());
    }
    Err(AdapterError::MissingBaseUrl { provider: req.provider })
}

fn build_request(req: &CompleteRequest) -> ChatRequest<'_> {
    let mut messages: Vec<RequestMessage<'_>> = req
        .messages
        .iter()
        .map(|m| RequestMessage {
            role: match m.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::System => "system",
            },
            content: MessageContent::Text(&m.content),
        })
        .collect();
    // Attach the image (if any) to the most recent user message using the
    // OpenAI multimodal `image_url` part with an inline data URL.
    let images=req.image.iter().chain(req.project_images.iter()).collect::<Vec<_>>();
    if !images.is_empty() {
        if let Some(msg)=messages.iter_mut().rev().find(|m|m.role=="user") {
            let text=match &msg.content {MessageContent::Text(t)=>*t,_=>""};
            let mut parts=vec![ContentPart::Text{text}];
            for img in images {parts.push(ContentPart::ImageUrl{image_url:ImageUrl{url:format!("data:{};base64,{}",img.media_type,img.data)}});}
            msg.content=MessageContent::Parts(parts);
        }
    }
    ChatRequest {
        model: &req.model,
        messages,
        stream: true,
        temperature: req.temperature.clamp(0.0, 2.0),
        max_tokens: req.max_tokens,
    }
}

#[async_trait]
impl Adapter for OpenAIAdapter {
    async fn stream(
        &self,
        req: CompleteRequest,
    ) -> Result<BoxStream<'static, Result<CompletionChunk, AdapterError>>, AdapterError> {
        let url = format!("{}/v1/chat/completions", base_url(&req)?);
        let body = serde_json::to_vec(&build_request(&req))?;
        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", req.api_key))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(AdapterError::HttpStatus { status: status.as_u16(), message: text });
        }

        // OpenAI streams Server-Sent Events: each frame begins with `data: `
        // and the terminal frame is `data: [DONE]`. We parse incrementally
        // by buffering until a newline.
        let raw = response.bytes_stream();
        let stream = sse_lines(raw).filter_map(|line_result| async move {
            let line = match line_result {
                Ok(l) => l,
                Err(err) => return Some(Err(err)),
            };
            let payload = line.strip_prefix("data: ")?;
            if let Ok(value)=serde_json::from_str::<serde_json::Value>(payload){
                if let Some(error)=value.get("error").filter(|error|!error.is_null()){
                    return Some(Err(AdapterError::Stream(error.to_string())));
                }
            }
            if payload.trim() == "[DONE]" {
                return None;
            }
            let frame: StreamFrame = match serde_json::from_str(payload) {
                Ok(f) => f,
                Err(_) => return None, // skip malformed lines rather than killing the stream
            };
            chunk_from_frame(frame)
        });
        Ok(Box::pin(stream))
    }

    async fn ping(&self, req: CompleteRequest) -> Result<Option<String>, AdapterError> {
        // OpenAI's `/v1/models` doesn't require streaming. We just hit it
        // and look for the configured model in the response.
        let url = format!("{}/v1/models", base_url(&req)?);
        let response = reqwest::Client::new()
            .get(&url)
            .header("Authorization", format!("Bearer {}", req.api_key))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(AdapterError::HttpStatus { status: status.as_u16(), message: text });
        }
        #[derive(Deserialize)]
        struct ModelsResp {
            data: Vec<ModelEntry>,
        }
        #[derive(Deserialize)]
        struct ModelEntry {
            id: String,
        }
        let parsed: ModelsResp = response.json().await?;
        Ok(parsed.data.into_iter().map(|m| m.id).find(|id| id == &req.model).or(Some(req.model)))
    }
}

/// Convert a stream of raw byte chunks into a stream of `\n`-terminated
/// lines (UTF-8). Tolerates server flushes mid-line.
fn sse_lines<S, E>(stream: S) -> impl Stream<Item = Result<String, AdapterError>>
where
    S: Stream<Item = Result<bytes::Bytes, E>>,
    E: Into<AdapterError>,
{
    async_stream::stream! {
        let mut buf = String::new();
        let mut stream = std::pin::pin!(stream);
        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(b) => b,
                Err(err) => {
                    yield Err(err.into());
                    return;
                }
            };
            match std::str::from_utf8(&chunk) {
                Ok(s) => buf.push_str(s),
                Err(err) => {
                    warn!(?err, "non-utf8 chunk from provider");
                    continue;
                }
            }
            while let Some(idx) = buf.find('\n') {
                let line: String = buf.drain(..=idx).collect();
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').to_string();
                if !trimmed.is_empty() {
                    yield Ok(trimmed);
                }
            }
        }
        let leftover = buf.trim();
        if !leftover.is_empty() {
            yield Ok(leftover.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribe_shared::{ChatMessage, ImageInput};

    fn request(messages: Vec<ChatMessage>, image: Option<ImageInput>) -> CompleteRequest {
        CompleteRequest {
            provider: AIProvider::Openai,
            model: "gpt-test".into(),
            base_url: None,
            api_key: "k".into(),
            messages,
            image,
            project_images: Vec::new(),
            temperature: 0.0,
            max_tokens: Some(16),
        }
    }

    #[test]
    fn standard_length_finish_frame_is_not_a_complete_answer(){
        let frame:StreamFrame=serde_json::from_str(r#"{"id":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#).unwrap();
        assert!(matches!(chunk_from_frame(frame),Some(Err(AdapterError::OutputLimit))));
        let frame:StreamFrame=serde_json::from_str(r#"{"choices":[{"delta":{"content":"partial text"},"finish_reason":"length"}]}"#).unwrap();
        assert!(matches!(chunk_from_frame(frame),Some(Err(AdapterError::OutputLimit))));
    }
    #[test]
    fn null_or_stop_finish_frames_preserve_normal_stream_behavior(){
        let frame:StreamFrame=serde_json::from_str(r#"{"choices":[{"delta":{"content":"answer"},"finish_reason":null}]}"#).unwrap();
        assert_eq!(chunk_from_frame(frame).unwrap().unwrap().text,"answer");
        let frame:StreamFrame=serde_json::from_str(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#).unwrap();
        assert!(chunk_from_frame(frame).is_none());
    }

    #[test]
    fn text_only_message_serialises_content_as_a_bare_string() {
        let req = request(
            vec![ChatMessage { role: ChatRole::User, content: "hi".into() }],
            None,
        );
        let json = serde_json::to_value(build_request(&req)).unwrap();
        assert_eq!(json["messages"][0]["content"], serde_json::json!("hi"));
    }

    #[test]
    fn image_attaches_as_an_image_url_data_url_on_last_user_message() {
        let req = request(
            vec![
                ChatMessage { role: ChatRole::System, content: "sys".into() },
                ChatMessage { role: ChatRole::User, content: "read it".into() },
            ],
            Some(ImageInput { media_type: "image/png".into(), data: "QUJD".into() }),
        );
        let json = serde_json::to_value(build_request(&req)).unwrap();
        let content = &json["messages"][1]["content"];
        assert!(content.is_array(), "content should be a parts array, got {content}");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "read it");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,QUJD");
    }

    #[test]
    fn multiple_project_images_preserve_text_and_order(){
        let mut req=request(vec![ChatMessage{role:ChatRole::User,content:"read project".into()}],None);
        req.project_images=vec![ImageInput{media_type:"image/png".into(),data:"QUJD".into()},ImageInput{media_type:"image/png".into(),data:"REVG".into()}];
        let json=serde_json::to_value(build_request(&req)).unwrap();
        let parts=&json["messages"][0]["content"];assert_eq!(parts.as_array().unwrap().len(),3);assert_eq!(parts[0]["text"],"read project");assert_eq!(parts[2]["image_url"]["url"],"data:image/png;base64,REVG");
    }
}
