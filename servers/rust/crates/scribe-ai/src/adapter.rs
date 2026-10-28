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
    pub usage:Option<TokenUsage>,
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

/// Provider-reported cumulative counters. Never estimate missing usage from text length.
#[derive(Debug,Clone,Default)]
pub struct TokenUsage{pub input:Option<u64>,pub output:Option<u64>,pub total:Option<u64>,pub cached:Option<u64>}
impl TokenUsage{
 pub fn merge(&mut self,next:&Self){fn merge(a:&mut Option<u64>,b:Option<u64>){if let Some(b)=b{*a=Some(a.unwrap_or(0).max(b));}}merge(&mut self.input,next.input);merge(&mut self.output,next.output);merge(&mut self.total,next.total);merge(&mut self.cached,next.cached);}
 pub fn reported(&self)->bool{self.input.is_some()||self.output.is_some()||self.total.is_some()}
 pub fn complete(&self)->bool{self.input.is_some()&&self.output.is_some()}
 pub fn total(&self)->u64{self.total.unwrap_or_else(||self.input.unwrap_or(0).saturating_add(self.output.unwrap_or(0)))}
}
pub fn usage_from_json(value:&Value,format:&str)->Option<TokenUsage>{
 let count=|v:&Value,key:&str|v.get(key).and_then(Value::as_u64).filter(|n|*n<=1_000_000_000_000);
 let mut result=TokenUsage::default();
 match format{
  "openai"=>{let u=value.get("usage")?;result.input=count(u,"prompt_tokens");result.output=count(u,"completion_tokens");result.total=count(u,"total_tokens");result.cached=u.get("prompt_tokens_details").and_then(|v|count(v,"cached_tokens")).or_else(||count(u,"prompt_cache_hit_tokens"));},
  "anthropic"=>{let u=value.get("usage").or_else(||value.get("message").and_then(|m|m.get("usage")))?;result.input=count(u,"input_tokens").map(|n|n.saturating_add(count(u,"cache_read_input_tokens").unwrap_or(0)).saturating_add(count(u,"cache_creation_input_tokens").unwrap_or(0)));result.output=count(u,"output_tokens");result.cached=count(u,"cache_read_input_tokens");},
  "gemini"=>{let u=value.get("usageMetadata")?;result.input=count(u,"promptTokenCount");result.output=count(u,"candidatesTokenCount").map(|n|n.saturating_add(count(u,"thoughtsTokenCount").unwrap_or(0)));result.total=count(u,"totalTokenCount");result.cached=count(u,"cachedContentTokenCount");},_=>return None,
 }
 result.reported().then_some(result)
}
impl AdapterError{
 pub fn is_context_too_long(&self)->bool{
  let raw=match self{Self::HttpStatus{status:400|413|422,message}|Self::Stream(message)=>message,_=>return false};
  fn fields(v:&Value,depth:usize)->String{if depth>2{return String::new();}if let Some(s)=v.as_str(){return s.chars().take(4096).collect();}if let Some(v)=v.as_object(){return ["code","type","message","error"].iter().filter_map(|k|v.get(*k)).map(|v|fields(v,depth+1)).collect::<Vec<_>>().join(" ");}String::new()}
  let text=serde_json::from_str::<Value>(raw).map(|v|fields(&v,0)).unwrap_or_else(|_|raw.chars().take(4096).collect()).to_lowercase();
  if ["invalid_api_key","authentication","unauthorized","rate_limit","rate limit","permission","forbidden"].iter().any(|v|text.contains(v)){return false;}
  ["context_length_exceeded","maximum context length","context window exceeded","prompt is too long","prompt too long","input is too long","input token count exceeds","上下文长度超","上下文过长"].iter().any(|v|text.contains(v))||(text.contains("input token count")&&text.contains("exceed"))
 }
}
#[cfg(test)]mod usage_tests{use super::*;use serde_json::json;#[test]fn counters_are_cumulative_and_cached_tokens_not_double_counted(){let a=usage_from_json(&json!({"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,"prompt_tokens_details":{"cached_tokens":50}}}),"openai").unwrap();assert_eq!(a.total(),120);assert_eq!(a.cached,Some(50));let mut b=usage_from_json(&json!({"message":{"usage":{"input_tokens":10,"cache_read_input_tokens":50,"cache_creation_input_tokens":20,"output_tokens":0}}}),"anthropic").unwrap();b.merge(&usage_from_json(&json!({"usage":{"output_tokens":25}}),"anthropic").unwrap());assert_eq!(b.input,Some(80));assert_eq!(b.total(),105);b.merge(&b.clone());assert_eq!(b.total(),105);assert!(usage_from_json(&json!({}),"openai").is_none());}#[test]fn context_errors_are_narrow(){assert!(AdapterError::HttpStatus{status:400,message:r#"{"error":{"code":"context_length_exceeded"}}"#.into()}.is_context_too_long());for (status,message) in [(401,"maximum context length"),(429,"prompt too long"),(500,"prompt too long"),(400,r#"{"error":{"message":"bad request","request":{"text":"prompt too long"}}}"#),(400,"invalid_api_key context_length_exceeded")]{assert!(!AdapterError::HttpStatus{status,message:message.into()}.is_context_too_long());}}}
