//! Anthropic Messages API adapter.
//!
//! Differences from OpenAI:
//!   * Endpoint is `/v1/messages`.
//!   * System prompt goes in a top-level `system` field (not a message).
//!   * Auth header is `x-api-key`, plus `anthropic-version: 2023-06-01`.
//!   * Streaming format is SSE with multiple event types; we care about
//!     `content_block_delta` carrying a `text_delta`.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use scribe_shared::ChatRole;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use serde_json::Value;

use crate::adapter::{Adapter, AdapterError, CompleteRequest, CompletionChunk,NativeToolCallDelta,NativeToolTurn};

const ANTHROPIC_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicAdapter;

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<RequestMessage<'a>>,
    stream: bool,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<RequestTool<'a>>,
}

#[derive(Serialize)]
struct RequestTool<'a> {name:&'a str,description:&'a str,input_schema:&'a Value}

#[derive(Serialize)]
struct RequestMessage<'a> {
    role: &'a str,
    content: MessageContent<'a>,
}

/// User-message content is either a plain string (text-only path, which
/// serialises byte-identically to the previous `&str` form) or an array
/// of typed blocks when an image is attached.
#[derive(Serialize)]
#[serde(untagged)]
enum MessageContent<'a> {
    Text(&'a str),
    Blocks(Vec<ContentBlock<'a>>),
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ContentBlock<'a> {
    #[serde(rename = "text")]
    Text { text: &'a str },
    #[serde(rename = "image")]
    Image { source: ImageSource<'a> },
    #[serde(rename = "tool_use")]
    ToolUse {id:&'a str,name:&'a str,input:Value},
    #[serde(rename = "tool_result")]
    ToolResult {tool_use_id:&'a str,content:&'a str,is_error:bool},
}

#[derive(Serialize)]
struct ImageSource<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    media_type: &'a str,
    data: &'a str,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "content_block_start")]
    ContentBlockStart {index:usize,content_block:StartBlock},
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {index:usize,delta:Delta},
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {index:usize},
    #[serde(rename = "message_delta")]
    MessageDelta {delta:MessageDelta},
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]#[serde(tag="type")]
enum StartBlock {
    #[serde(rename="tool_use")] ToolUse{id:String,name:String,input:Value},
    #[serde(rename="text")] Text{text:String},
    #[serde(other)] Other,
}
#[derive(Deserialize)]struct MessageDelta{stop_reason:Option<String>}
#[derive(Deserialize)]
#[serde(tag = "type")]
enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename="input_json_delta")]
    InputJsonDelta {partial_json:String},
    #[serde(other)]
    Other,
}
#[derive(Default)]struct EventState{tools:HashMap<usize,bool>,stopped:bool}
impl EventState{
 fn consume(&mut self,payload:&str)->Option<Result<CompletionChunk,AdapterError>>{
    if payload.trim()=="[DONE]"{self.stopped=true;return None;}
    if let Ok(value)=serde_json::from_str::<Value>(payload){if let Some(error)=value.get("error").filter(|v|!v.is_null()){return Some(Err(AdapterError::Stream(error.to_string())));}}
    let event=serde_json::from_str::<StreamEvent>(payload).ok()?;
    let chunk=match event{
      StreamEvent::ContentBlockStart{index,content_block:StartBlock::ToolUse{id,name,input}}=>{
       if !input.is_object(){return Some(Err(AdapterError::Stream("Anthropic tool input must be an object".into())));}
       let arguments=if input.as_object().is_some_and(|v|!v.is_empty()){Some(input.to_string())}else{None};self.tools.insert(index,arguments.is_some());
       CompletionChunk{usage:None,text:String::new(),tool_call_deltas:vec![NativeToolCallDelta{index,id:Some(id),name:Some(name),arguments}]}
      },
      StreamEvent::ContentBlockDelta{index,delta:Delta::InputJsonDelta{partial_json}}=>{
       let Some(seen)=self.tools.get_mut(&index)else{return Some(Err(AdapterError::Stream("Anthropic tool argument delta without tool start".into())));};
       if partial_json.is_empty(){return None;}*seen=true;CompletionChunk{usage:None,text:String::new(),tool_call_deltas:vec![NativeToolCallDelta{index,id:None,name:None,arguments:Some(partial_json)}]}
      },
      StreamEvent::ContentBlockStop{index}=>{
       if self.tools.remove(&index)==Some(false){CompletionChunk{usage:None,text:String::new(),tool_call_deltas:vec![NativeToolCallDelta{index,id:None,name:None,arguments:Some("{}".into())}]}}else{return None;}
      },
      StreamEvent::ContentBlockStart{content_block:StartBlock::Text{text},..}|StreamEvent::ContentBlockDelta{delta:Delta::TextDelta{text},..}=>{
       if text.is_empty(){return None;}CompletionChunk{usage:None,text,tool_call_deltas:vec![]}
      },
      StreamEvent::MessageDelta{delta} if delta.stop_reason.as_deref()==Some("max_tokens")=>return Some(Err(AdapterError::OutputLimit)),
      StreamEvent::MessageStop{}=>{self.stopped=true;return None;},
      _=>return None,
    };Some(Ok(chunk))
 }
}

fn base_url(req: &CompleteRequest) -> String {
    req.base_url
        .as_deref()
        .filter(|b| !b.is_empty())
        .map(|b| b.trim_end_matches('/').to_string())
        .unwrap_or_else(|| ANTHROPIC_BASE.to_string())
}

fn build_request<'a>(req: &'a CompleteRequest) -> Result<MessagesRequest<'a>,AdapterError> {
    let mut system: Option<&'a str> = None;
    let mut messages: Vec<RequestMessage<'a>> = Vec::with_capacity(req.messages.len());
    for m in &req.messages {
        match m.role {
            ChatRole::System => {
                // Anthropic only allows one `system` field; the last
                // system message wins. (Multiple consecutive system
                // messages are rare in our prompts.)
                system = Some(m.content.as_str());
            }
            ChatRole::User => messages.push(RequestMessage {
                role: "user",
                content: MessageContent::Text(&m.content),
            }),
            ChatRole::Assistant => messages.push(RequestMessage {
                role: "assistant",
                content: MessageContent::Text(&m.content),
            }),
        }
    }
    // Attach the image (if any) to the most recent user message as a
    // content-block array. Claude wants the image block before the text.
    let images=req.image.iter().chain(req.project_images.iter()).collect::<Vec<_>>();
    if !images.is_empty() {
        if let Some(msg)=messages.iter_mut().rev().find(|m|m.role=="user") {
            let text=match &msg.content {MessageContent::Text(t)=>*t,_=>""};
            let mut blocks=Vec::new();
            for img in images {blocks.push(ContentBlock::Image{source:ImageSource{kind:"base64",media_type:&img.media_type,data:&img.data}});}
            if !text.is_empty(){blocks.push(ContentBlock::Text{text});}
            msg.content=MessageContent::Blocks(blocks);
        }
    }
    // Native assistant/tool turns are a protocol transcript, not ordinary prompt text.
    let mut previous_tool=false;
    for turn in &req.native_turns{
      match turn{
       NativeToolTurn::Assistant{content,tool_calls}=>{
        let mut blocks=vec![];if let Some(text)=content.as_deref().filter(|s|!s.is_empty()){blocks.push(ContentBlock::Text{text});}
        for call in tool_calls{let input:Value=serde_json::from_str(&call.arguments).map_err(|_|AdapterError::Stream("Invalid native tool arguments".into()))?;if !input.is_object(){return Err(AdapterError::Stream("Native tool arguments must be an object".into()));}blocks.push(ContentBlock::ToolUse{id:&call.id,name:&call.name,input});}
        if !blocks.is_empty(){messages.push(RequestMessage{role:"assistant",content:MessageContent::Blocks(blocks)});}previous_tool=false;
       },
       NativeToolTurn::Tool{tool_call_id,content,is_error}=>{
        let block=ContentBlock::ToolResult{tool_use_id:tool_call_id,content,is_error:*is_error};
        if previous_tool{if let Some(RequestMessage{content:MessageContent::Blocks(blocks),..})=messages.last_mut(){blocks.push(block);}else{return Err(AdapterError::Stream("Invalid native tool transcript".into()));}}
        else{messages.push(RequestMessage{role:"user",content:MessageContent::Blocks(vec![block])});}previous_tool=true;
       }
      }
    }
    Ok(MessagesRequest {
        model: &req.model,system,messages,stream:true,temperature:req.temperature.clamp(0.0,1.0),max_tokens:req.max_tokens,
        tools:req.native_tools.iter().map(|tool|RequestTool{name:&tool.name,description:&tool.description,input_schema:&tool.parameters}).collect(),
    })
}

// Messages requires max_tokens on strict providers. Use their live model capability,
// never substitute a small application default. Compatible APIs may accept omission.
fn reported_output_ceiling(value:&Value,model:&str)->Option<u32>{
 let item=if let Some(rows)=value.get("data").and_then(Value::as_array){rows.iter().find(|row|row.get("id").and_then(Value::as_str)==Some(model))?}else{value};
 ["max_output_tokens","max_tokens","max_completion_tokens"].iter().find_map(|name|item.get(*name).and_then(Value::as_u64).and_then(|n|u32::try_from(n).ok()).filter(|n|*n>0))
}
async fn discover_output_ceiling(req:&CompleteRequest)->Option<u32>{
 use std::{collections::HashMap,hash::{Hash,Hasher},sync::{Mutex,OnceLock},time::{Duration,Instant}};
 static CACHE:OnceLock<Mutex<HashMap<u64,(Instant,Option<u32>)>>>=OnceLock::new();
 let mut hasher=std::collections::hash_map::DefaultHasher::new();base_url(req).hash(&mut hasher);req.model.hash(&mut hasher);req.api_key.hash(&mut hasher);let key=hasher.finish();
 let cache=CACHE.get_or_init(||Mutex::new(HashMap::new()));
 if let Ok(guard)=cache.lock(){if let Some((time,value))=guard.get(&key){if time.elapsed()<Duration::from_secs(600){return *value;}}}
 let mut url=reqwest::Url::parse(&format!("{}/v1/models",base_url(req).trim_end_matches("/v1"))).ok()?;
 let deepseek=url.host_str()==Some("api.deepseek.com");
 if deepseek{url.set_path("/models");}
 // An alias may only resolve via the single-model endpoint on strict Anthropic.
 let mut urls=vec![url.clone()];if !deepseek{let mut exact=url;exact.path_segments_mut().ok()?.push(&req.model);urls.insert(0,exact);}
 let client=reqwest::Client::builder().timeout(Duration::from_secs(3)).redirect(reqwest::redirect::Policy::none()).build().ok()?;
 let mut ceiling=None;
 for url in urls{
  let request=client.get(url).header("anthropic-version",ANTHROPIC_VERSION);
  let request=if deepseek{request.bearer_auth(&req.api_key)}else{request.header("x-api-key",&req.api_key)};
  if let Ok(response)=request.send().await{if response.status().is_success()&&response.content_length().is_none_or(|n|n<=262144){if let Ok(bytes)=response.bytes().await{if bytes.len()<=262144{if let Ok(value)=serde_json::from_slice::<Value>(&bytes){ceiling=reported_output_ceiling(&value,&req.model);if ceiling.is_some(){break;}}}}}}
 }
 if let Ok(mut guard)=cache.lock(){if guard.len()>=512{guard.clear();}guard.insert(key,(Instant::now(),ceiling));}
 ceiling
}

#[async_trait]
impl Adapter for AnthropicAdapter {
    async fn stream(
        &self,
        req: CompleteRequest,
    ) -> Result<BoxStream<'static, Result<CompletionChunk, AdapterError>>, AdapterError> {
        let url = format!("{}/v1/messages", base_url(&req));
        let mut payload = build_request(&req)?;
        if payload.max_tokens.is_none(){payload.max_tokens=discover_output_ceiling(&req).await;}
        let body = serde_json::to_vec(&payload)?;
        let response = reqwest::Client::new()
            .post(&url)
            .header("x-api-key", &req.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(AdapterError::HttpStatus { status: status.as_u16(), message: text });
        }

        let stream=parse_anthropic_events(anthropic_sse(response.bytes_stream()));
        Ok(Box::pin(stream))
    }

    async fn ping(&self, req: CompleteRequest) -> Result<Option<String>, AdapterError> {
        // Anthropic has no `models` listing; emit a tiny non-streaming
        // request and verify it returns 200.
        let url = format!("{}/v1/messages", base_url(&req));
        let payload = serde_json::json!({
            "model": req.model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ping"}],
        });
        let response = reqwest::Client::new()
            .post(&url)
            .header("x-api-key", &req.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(AdapterError::HttpStatus { status: status.as_u16(), message: text });
        }
        Ok(Some(req.model))
    }
}

fn parse_anthropic_events<S>(lines:S)->impl futures::Stream<Item=Result<CompletionChunk,AdapterError>>
where S:futures::Stream<Item=Result<String,AdapterError>>{
 async_stream::stream!{
  let mut lines=std::pin::pin!(lines);let mut state=EventState::default();
  while let Some(line)=lines.next().await{let line=match line{Ok(line)=>line,Err(error)=>{yield Err(error);return;}};
   if line.starts_with(':')&&!state.stopped{yield Ok(CompletionChunk::default());continue;}
   let Some(payload)=line.strip_prefix("data:").map(str::trim_start)else{continue;};let mut emitted=false;
   if let Ok(value)=serde_json::from_str::<Value>(payload){if let Some(usage)=crate::adapter::usage_from_json(&value,"anthropic"){emitted=true;yield Ok(CompletionChunk{usage:Some(usage),..Default::default()});}}
   if let Some(result)=state.consume(payload){emitted=true;let failed=result.is_err();yield result;if failed{return;}}
   if !emitted&&!state.stopped{yield Ok(CompletionChunk::default());}
  }
  if !state.stopped{yield Err(AdapterError::Stream("Anthropic response ended before message_stop".into()));}
 }
}

fn anthropic_sse<S, E>(stream: S) -> impl futures::Stream<Item = Result<String, AdapterError>>
where
    S: futures::Stream<Item = Result<bytes::Bytes, E>>,
    E: Into<AdapterError>,
{
    async_stream::stream! {
        // Buffer bytes until a complete SSE line: TCP chunks may split a UTF-8 code point.
        let mut buf=Vec::<u8>::new();let mut stream=std::pin::pin!(stream);
        while let Some(result)=stream.next().await{
         let chunk=match result{Ok(bytes)=>bytes,Err(error)=>{yield Err(error.into());return;}};buf.extend_from_slice(&chunk);
         if buf.len()>1024*1024{yield Err(AdapterError::Stream("Anthropic SSE line exceeded limit".into()));return;}
         while let Some(index)=buf.iter().position(|b|*b==b'\n'){
          let bytes=buf.drain(..=index).collect::<Vec<_>>();let text=match String::from_utf8(bytes){Ok(text)=>text,Err(_)=>{yield Err(AdapterError::Stream("Invalid UTF-8 in Anthropic SSE".into()));return;}};
          let line=text.trim_end_matches('\n').trim_end_matches('\r');if !line.is_empty(){yield Ok(line.to_string());}
         }
        }
        if !buf.is_empty(){match String::from_utf8(buf){Ok(line) if !line.trim().is_empty()=>yield Ok(line.trim_end_matches('\r').to_string()),Ok(_)=>{},Err(_)=>yield Err(AdapterError::Stream("Invalid UTF-8 in Anthropic SSE".into()))}}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribe_shared::{AIProvider, ChatMessage, ImageInput};

    fn request(messages: Vec<ChatMessage>, image: Option<ImageInput>) -> CompleteRequest {
        CompleteRequest {
            provider: AIProvider::Anthropic,
            model: "claude-test".into(),
            base_url: None,
            api_key: "k".into(),
            messages,
            image,
            project_images: Vec::new(),
            native_tools:vec![],native_turns:vec![],
            temperature: 0.0,
            max_tokens: Some(16),
        }
    }

    #[test]
    fn text_only_message_serialises_content_as_a_bare_string() {
        let req = request(
            vec![ChatMessage { role: ChatRole::User, content: "hello".into() }],
            None,
        );
        let json = serde_json::to_value(build_request(&req).unwrap()).unwrap();
        assert_eq!(json["messages"][0]["content"], serde_json::json!("hello"));
    }

    #[test]
    fn image_attaches_as_blocks_on_last_user_message() {
        let req = request(
            vec![
                ChatMessage { role: ChatRole::System, content: "sys".into() },
                ChatMessage { role: ChatRole::User, content: "do it".into() },
            ],
            Some(ImageInput { media_type: "image/png".into(), data: "QUJD".into() }),
        );
        let json = serde_json::to_value(build_request(&req).unwrap()).unwrap();
        let content = &json["messages"][0]["content"];
        assert!(content.is_array(), "content should be a block array, got {content}");
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[0]["source"]["media_type"], "image/png");
        assert_eq!(content[0]["source"]["data"], "QUJD");
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "do it");
    }

    #[test]
    fn multiple_project_images_preserve_text_and_order(){
        let mut req=request(vec![ChatMessage{role:ChatRole::User,content:"read project".into()}],None);
        req.project_images=vec![ImageInput{media_type:"image/png".into(),data:"QUJD".into()},ImageInput{media_type:"image/png".into(),data:"REVG".into()}];
        let json=serde_json::to_value(build_request(&req).unwrap()).unwrap();
        let parts=&json["messages"][0]["content"];assert_eq!(parts.as_array().unwrap().len(),3);assert_eq!(parts[2]["text"],"read project");assert_eq!(parts[1]["source"]["data"],"REVG");
    }
    #[test]
    fn native_tools_and_results_use_anthropic_blocks_and_preserve_custom_endpoint(){
      use crate::adapter::{NativeToolDefinition,NativeToolCall};
      let mut req=request(vec![ChatMessage{role:ChatRole::System,content:"sys".into()},ChatMessage{role:ChatRole::User,content:"inspect".into()}],None);
      req.base_url=Some("https://api.deepseek.com/anthropic/".into());
      req.native_tools=vec![NativeToolDefinition{name:"read_file".into(),description:"Read an allowed file".into(),parameters:serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})}];
      req.native_turns=vec![NativeToolTurn::Assistant{content:Some("I will inspect the project.".into()),tool_calls:vec![NativeToolCall{id:"toolu_one".into(),name:"read_file".into(),arguments:r#"{"path":"main.tex"}"#.into()},NativeToolCall{id:"toolu_two".into(),name:"read_file".into(),arguments:r#"{"path":"refs.bib"}"#.into()}]},NativeToolTurn::Tool{tool_call_id:"toolu_one".into(),content:"allowed content".into(),is_error:false},NativeToolTurn::Tool{tool_call_id:"toolu_two".into(),content:"not found".into(),is_error:true}];
      let json=serde_json::to_value(build_request(&req).unwrap()).unwrap();
      assert_eq!(format!("{}/v1/messages",base_url(&req)),"https://api.deepseek.com/anthropic/v1/messages");
      assert_eq!(json["tools"][0]["name"],"read_file");assert_eq!(json["tools"][0]["input_schema"]["type"],"object");assert!(json["tools"][0].get("function").is_none());
      assert_eq!(json["messages"].as_array().unwrap().len(),3);let assistant=&json["messages"][1];assert_eq!(assistant["role"],"assistant");assert_eq!(assistant["content"][1]["type"],"tool_use");assert_eq!(assistant["content"][1]["id"],"toolu_one");assert_eq!(assistant["content"][1]["input"]["path"],"main.tex");
      let results=&json["messages"][2];assert_eq!(results["role"],"user");assert_eq!(results["content"].as_array().unwrap().len(),2);assert_eq!(results["content"][0]["tool_use_id"],"toolu_one");assert_eq!(results["content"][1]["is_error"],true);
    }
    #[test]
    fn tool_argument_stream_does_not_prefix_empty_object_and_keeps_content_index(){
      let mut state=EventState::default();
      let started=state.consume(r#"{"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}}"#).unwrap().unwrap();
      assert_eq!(started.tool_call_deltas[0].index,3);assert_eq!(started.tool_call_deltas[0].id.as_deref(),Some("toolu_1"));assert!(started.tool_call_deltas[0].arguments.is_none());
      let chunks=[r#"{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,r#"{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"\"main.tex\"}"}}"#];
      let mut arguments=String::new();for payload in chunks{let chunk=state.consume(payload).unwrap().unwrap();arguments.push_str(chunk.tool_call_deltas[0].arguments.as_deref().unwrap());}
      assert_eq!(serde_json::from_str::<Value>(&arguments).unwrap(),serde_json::json!({"path":"main.tex"}));assert!(state.consume(r#"{"type":"content_block_stop","index":3}"#).is_none());assert!(state.consume(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#).is_none());assert!(state.consume(r#"{"type":"message_stop"}"#).is_none());assert!(state.stopped);
    }
    #[test]
    fn empty_and_prefilled_tool_inputs_are_emitted_once_and_text_remains_text(){
      let mut state=EventState::default();state.consume(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"a","name":"list_files","input":{}}}"#).unwrap().unwrap();let end=state.consume(r#"{"type":"content_block_stop","index":0}"#).unwrap().unwrap();assert_eq!(end.tool_call_deltas[0].arguments.as_deref(),Some("{}"));
      let filled=state.consume(r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"b","name":"read_file","input":{"path":"main.tex"}}}"#).unwrap().unwrap();assert_eq!(filled.tool_call_deltas[0].arguments.as_deref(),Some(r#"{"path":"main.tex"}"#));assert!(state.consume(r#"{"type":"content_block_stop","index":1}"#).is_none());
      let text=state.consume(r#"{"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Done"}}"#).unwrap().unwrap();assert_eq!(text.text,"Done");assert!(text.tool_call_deltas.is_empty());assert!(state.consume(r#"{"type":"content_block_delta","index":4,"delta":{"type":"thinking_delta","thinking":"private"}}"#).is_none());
    }
    #[test]
    fn output_limit_and_malformed_native_transcript_fail_closed(){
      let mut state=EventState::default();assert!(matches!(state.consume(r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#),Some(Err(AdapterError::OutputLimit))));
      assert!(state.consume(r#"{"type":"content_block_delta","index":42,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#).unwrap().is_err());
      for arguments in ["not json","[]","null"]{let mut req=request(vec![],None);req.native_turns=vec![NativeToolTurn::Assistant{content:None,tool_calls:vec![crate::adapter::NativeToolCall{id:"a".into(),name:"read_file".into(),arguments:arguments.into()}]}];assert!(build_request(&req).is_err());}
    }
    #[tokio::test]
    async fn sse_byte_buffer_preserves_split_unicode_and_trailing_line(){
      let wire="data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"中文\"}}\r\ndata: {\"type\":\"message_stop\"}";
      let chunks=wire.as_bytes().iter().map(|byte|Ok::<_,AdapterError>(bytes::Bytes::from(vec![*byte]))).collect::<Vec<_>>();let lines=anthropic_sse(futures::stream::iter(chunks)).collect::<Vec<_>>().await;assert_eq!(lines.len(),2);let mut state=EventState::default();let chunk=state.consume(lines[0].as_ref().unwrap().strip_prefix("data: ").unwrap()).unwrap().unwrap();assert_eq!(chunk.text,"中文");state.consume(lines[1].as_ref().unwrap().strip_prefix("data: ").unwrap());assert!(state.stopped);
    }

}

#[cfg(test)]mod automatic_output_tests{
 use super::*;
 #[test]fn capability_ceiling_comes_from_the_exact_model_not_context_size(){
  let v=serde_json::json!({"data":[{"id":"other","max_output_tokens":1},{"id":"deepseek-v4-pro","context_window":1048576,"max_output_tokens":393216}]});
  assert_eq!(reported_output_ceiling(&v,"deepseek-v4-pro"),Some(393216));assert_eq!(reported_output_ceiling(&v,"missing"),None);
  assert_eq!(reported_output_ceiling(&serde_json::json!({"max_tokens":64000}),"alias"),Some(64000));
  assert_eq!(reported_output_ceiling(&serde_json::json!({"context_window":1000000}),"alias"),None);
 }
 #[test]fn automatic_requests_have_no_fabricated_default_but_explicit_budgets_are_preserved(){
  let mut req=CompleteRequest::system_then_user(scribe_shared::AIProvider::Anthropic,"fixture".into(),None,"k".into(),"system","user");
  let body=serde_json::to_value(build_request(&req).unwrap()).unwrap();assert!(body.get("max_tokens").is_none());
  req.max_tokens=Some(200);let body=serde_json::to_value(build_request(&req).unwrap()).unwrap();assert_eq!(body["max_tokens"],200);
 }
}

#[cfg(test)]mod activity_tests{use super::*;use futures::StreamExt;#[tokio::test]async fn thinking_and_ping_preserve_liveness_without_exposing_thinking(){let lines=futures::stream::iter(vec![Ok::<_,AdapterError>(": ping".into()),Ok(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"PRIVATE"}}"#.into()),Ok(r#"data: {"type":"ping"}"#.into()),Ok(r#"data: {"type":"message_stop"}"#.into())]);let chunks=parse_anthropic_events(lines).collect::<Vec<_>>().await;assert_eq!(chunks.len(),3);assert!(chunks.iter().all(|c|c.as_ref().is_ok_and(|c|c.text.is_empty()&&c.tool_call_deltas.is_empty()&&c.usage.is_none())));}}
