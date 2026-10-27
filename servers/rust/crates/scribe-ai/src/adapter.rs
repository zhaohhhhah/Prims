//! AI provider adapter trait + dispatcher.
//!
//! Each adapter takes a list of chat messages and produces a stream of
//! token chunks. The HTTP handler tees those chunks into SSE frames.
//!
//! Three concrete implementations cover all six providers the Node
//! server supports:
//!
//! * [`openai`] — `openai`, `ollama`, `lmstudio`, `openai-compatible`
//!   (they all speak the OpenAI Chat Completions API)
//! * [`anthropic`] — `anthropic` (Claude Messages API)
//! * [`gemini`] — `gemini` (Google AI Studio API)

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use scribe_shared::{AIProvider, ChatMessage, ImageInput};
use thiserror::Error;
use serde::{Serialize,Deserialize};
use serde_json::Value;

pub use crate::providers::{anthropic, gemini, openai};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("模型输出达到上限，请按章节或短段分批修改；本轮没有可应用的修改方案，正文未改动。")]
    OutputLimit,
    #[error("provider {provider:?} requires a base URL but none was set")]
    MissingBaseUrl { provider: AIProvider },
    #[error("provider returned {status}: {message}")]
    HttpStatus { status: u16, message: String },
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),
    #[error("encode: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("stream: {0}")]
    Stream(String),
}

impl AdapterError {
    pub fn is_tools_unsupported(&self)->bool{
        let Self::HttpStatus{status:400|422,message}=self else{return false};
        fn metadata(v:&Value,depth:usize)->String{if depth>2{return String::new();}if let Some(text)=v.as_str(){return text.chars().take(4096).collect();}if let Some(object)=v.as_object(){return ["code","type","status","message","error"].iter().filter_map(|key|object.get(*key)).map(|value|if value.is_number(){value.to_string()}else{metadata(value,depth+1)}).collect::<Vec<_>>().join(" ");}String::new()}
        let text=serde_json::from_str::<Value>(message).map(|v|metadata(&v,0)).unwrap_or_else(|_|message.chars().take(4096).collect()).to_lowercase();
        if ["authentication","unauthorized","api key","api_key","forbidden","permission","rate limit","rate_limit","timeout"].iter().any(|s|text.contains(s)){return false;}
        if text.split(|c:char|!c.is_ascii_digit()).any(|part|matches!(part,"401"|"403"|"408"|"429"|"500"|"502"|"503"|"504")){return false;}
        let tool=["tools","tool_use","tool use","tool_calls","function calling","function_call","工具"].iter().any(|s|text.contains(s));
        let unsupported=["not supported","does not support","unsupported","not support","unknown parameter","unrecognized request argument","not permitted","not allowed","不支持"].iter().any(|s|text.contains(s));
        tool&&unsupported
    }

    /// Capability fallback is intentionally narrow: never infer it from transport/auth/rate errors.
    pub fn is_image_unsupported(&self) -> bool {
        match self {
            Self::HttpStatus { status: 400 | 415 | 422, message } => image_capability_message(message),
            Self::Stream(message) => image_capability_message(message),
            _ => false,
        }
    }
}
fn image_capability_message(raw: &str) -> bool {
    // Read only provider error metadata, never raw request echoes or arbitrary object fields.
    fn fields(value: &serde_json::Value, depth: usize) -> String {
        if depth > 2 { return String::new(); }
        if let Some(text)=value.as_str(){return text.chars().take(4096).collect();}
        if let Some(object)=value.as_object(){
            return ["code","type","status","message","error"].iter().filter_map(|k|object.get(*k)).map(|v|if v.is_number(){v.to_string()}else{fields(v,depth+1)}).collect::<Vec<_>>().join(" ");
        }
        String::new()
    }
    let text=serde_json::from_str::<serde_json::Value>(raw).map(|v|fields(&v,0)).unwrap_or_else(|_|raw.chars().take(4096).collect()).to_lowercase();
    if ["unauthorized","authentication","invalid_api_key","invalid api key","permission_denied","permission denied","forbidden","rate_limit","rate limit","resource_exhausted","too many requests"].iter().any(|s|text.contains(s)) {return false;}
    // Streaming providers may include an HTTP-like numeric status inside their error object.
    if text.split(|c:char|!c.is_ascii_digit()).any(|part|matches!(part,"401"|"403"|"408"|"429"|"500"|"502"|"503"|"504")){return false;}
    let image=text.contains("image")||text.contains("vision")||text.contains("图像")||text.contains("图片");
    let explicit=["not supported","does not support","doesn't support","do not support","cannot process","can't process","unsupported","text-only","text only","only supports text","only supported by certain models","不支持","无法处理"].iter().any(|s|text.contains(s));
    image&&explicit
}

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct NativeToolDefinition{pub name:String,pub description:String,pub parameters:Value}
#[derive(Debug,Clone,Default,Serialize,Deserialize)]
pub struct NativeToolCall{pub id:String,pub name:String,pub arguments:String}
#[derive(Debug,Clone,Default,Serialize,Deserialize)]
pub struct NativeToolCallDelta{pub index:usize,pub id:Option<String>,pub name:Option<String>,pub arguments:Option<String>}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(tag="role",rename_all="snake_case")]
pub enum NativeToolTurn{Assistant{content:Option<String>,tool_calls:Vec<NativeToolCall>},Tool{tool_call_id:String,content:String,is_error:bool}}
pub fn supports_native_tools(provider:AIProvider)->bool{matches!(provider,AIProvider::Openai|AIProvider::OpenaiCompatible|AIProvider::Ollama|AIProvider::Lmstudio|AIProvider::Anthropic)}

#[derive(Debug, Clone)]
pub struct CompleteRequest {
    pub provider: AIProvider,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: String,
    pub messages: Vec<ChatMessage>,
    pub native_tools:Vec<NativeToolDefinition>,
    pub native_turns:Vec<NativeToolTurn>,
    /// Optional inline image, attached to the last user message by
    /// vision-capable adapters. `None` for the text-only path.
    pub image: Option<ImageInput>,
    pub project_images: Vec<ImageInput>,
    /// Sampling temperature. 0–2 in OpenAI; clamped per-provider.
    pub temperature: f32,
    /// Max output tokens. None ⇒ adapter default.
    pub max_tokens: Option<u32>,
}

impl CompleteRequest {
    pub fn system_then_user(
        provider: AIProvider,
        model: String,
        base_url: Option<String>,
        api_key: String,
        system: &str,
        user: &str,
    ) -> Self {
        let messages = vec![
            ChatMessage { role: scribe_shared::ChatRole::System, content: system.to_string() },
            ChatMessage { role: scribe_shared::ChatRole::User, content: user.to_string() },
        ];
        Self {
            provider,
            model,
            base_url,
            api_key,
            messages,
            native_tools:Vec::new(),native_turns:Vec::new(),
            image: None,
            project_images: Vec::new(),
            temperature: 0.4,
            max_tokens: None,
        }
    }
}

/// One token-stream chunk. We don't try to expose finish reason / token
/// counts to clients — the SSE consumer just concatenates `text`.
#[derive(Debug, Clone, Default)]
pub struct CompletionChunk {
    pub text: String,
    pub tool_call_deltas:Vec<NativeToolCallDelta>,
}

#[async_trait]
pub trait Adapter: Send + Sync {
    async fn stream(
        &self,
        req: CompleteRequest,
    ) -> Result<BoxStream<'static, Result<CompletionChunk, AdapterError>>, AdapterError>;

    /// Quick connectivity check. Returns the model the provider echoed back
    /// (if available) so the UI can show the user it's wired up correctly.
    async fn ping(&self, req: CompleteRequest) -> Result<Option<String>, AdapterError>;
}

/// Dispatch on `provider` to the right adapter.
pub fn adapter_for(provider: AIProvider) -> Box<dyn Adapter> {
    match provider {
        AIProvider::Openai
        | AIProvider::Ollama
        | AIProvider::Lmstudio
        | AIProvider::OpenaiCompatible => Box::new(openai::OpenAIAdapter),
        AIProvider::Anthropic => Box::new(anthropic::AnthropicAdapter),
        AIProvider::Gemini => Box::new(gemini::GeminiAdapter),
    }
}

/// Helper to convert the abstract chunk stream to raw SSE-formatted
/// `Bytes` frames. The route handler maps these into `axum::response::sse`.
pub fn chunks_to_sse_lines(
    stream: BoxStream<'static, Result<CompletionChunk, AdapterError>>,
) -> BoxStream<'static, Result<Bytes, AdapterError>> {
    use futures::StreamExt;
    Box::pin(stream.map(|chunk| chunk.map(|c| Bytes::from(c.text))))
}

#[cfg(test)]mod capability_tests{
 use super::*;
 #[test]fn tool_capability_fallback_is_narrow_and_never_auth_or_transport(){
  assert!(AdapterError::HttpStatus{status:400,message:"This model does not support tools".into()}.is_tools_unsupported());
  for status in [401,403,408,429,500]{assert!(!AdapterError::HttpStatus{status,message:"tools not supported".into()}.is_tools_unsupported());}
  for message in ["invalid tools argument","invalid API key; tools unsupported","timeout","model not found"]{assert!(!AdapterError::HttpStatus{status:400,message:message.into()}.is_tools_unsupported());}
 }
 #[test]fn only_explicit_image_capability_errors_allow_retry(){for message in [r#"{"error":{"code":"unsupported_image","message":"This model does not support image inputs"}}"#,r#"{"error":{"message":"image_url is only supported by certain models"}}"#,"该模型不支持图片输入"]{assert!(AdapterError::HttpStatus{status:400,message:message.into()}.is_image_unsupported());assert!(AdapterError::Stream(message.into()).is_image_unsupported());}}
 #[test]fn auth_rate_transport_and_generic_validation_do_not_allow_retry(){for status in [401,403,408,429,500,502,503,504]{assert!(!AdapterError::HttpStatus{status,message:"model does not support images".into()}.is_image_unsupported());}for message in ["invalid image_url","invalid model","image too large",r#"{"code":"invalid_api_key","message":"unsupported image request without authentication"}"#,r#"{"status":429,"message":"images unsupported"}"#]{assert!(!AdapterError::HttpStatus{status:400,message:message.into()}.is_image_unsupported());assert!(!AdapterError::Stream(message.into()).is_image_unsupported());}assert!(!AdapterError::Stream("network connection reset".into()).is_image_unsupported());}
 #[tokio::test]async fn actual_network_error_never_allows_retry(){let error=reqwest::Client::builder().no_proxy().build().unwrap().get("http://127.0.0.1:0").send().await.unwrap_err();assert!(!AdapterError::Network(error).is_image_unsupported());}
 #[test]fn request_echoes_cannot_trigger_a_capability_retry(){let message=r#"{"error":{"message":"bad request","request":{"message":"images unsupported"}}}"#;assert!(!AdapterError::HttpStatus{status:400,message:message.into()}.is_image_unsupported());}
}
