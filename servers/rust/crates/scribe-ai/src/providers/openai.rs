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

use crate::adapter::{Adapter, AdapterError, CompleteRequest, CompletionChunk, NativeToolCallDelta, NativeToolTurn};

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<RequestTool<'a>>,
}

#[derive(Serialize)]
struct RequestMessage<'a> {
    role: &'a str,
    // Assistant tool calls use null content when the model produced no prose.
    content: Option<MessageContent<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<RequestToolCall<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Serialize)]
struct RequestTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: RequestFunctionDefinition<'a>,
}
#[derive(Serialize)]
struct RequestFunctionDefinition<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}
#[derive(Serialize)]
struct RequestToolCall<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    function: RequestFunctionCall<'a>,
}
#[derive(Serialize)]
struct RequestFunctionCall<'a> {
    name: &'a str,
    arguments: &'a str,
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
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolDelta>>,
}
#[derive(Deserialize)]
struct StreamToolDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunctionDelta>,
}
#[derive(Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

fn chunk_from_frame(frame:StreamFrame)->Option<Result<CompletionChunk,AdapterError>>{
    let choice=frame.choices.into_iter().next()?;
    if choice.finish_reason.as_deref()==Some("length"){return Some(Err(AdapterError::OutputLimit));}
    let delta=choice.delta?;
    let text=delta.content.unwrap_or_default();
    // Keep each index/id/name/arguments fragment intact. The bounded project
    // agent owns accumulation, validation and sequential execution, not this adapter.
    let tool_call_deltas=delta.tool_calls.unwrap_or_default().into_iter().map(|call| {
        let function=call.function;
        NativeToolCallDelta {index:call.index,id:call.id,name:function.as_ref().and_then(|f|f.name.clone()),arguments:function.and_then(|f|f.arguments)}
    }).collect::<Vec<_>>();
    if text.is_empty()&&tool_call_deltas.is_empty(){None}else{Some(Ok(CompletionChunk{text,tool_call_deltas}))}
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
            content: Some(MessageContent::Text(&m.content)),
            tool_calls: None,
            tool_call_id: None,
        })
        .collect();
    // Attach the image (if any) to the most recent user message using the
    // OpenAI multimodal `image_url` part with an inline data URL.
    let images=req.image.iter().chain(req.project_images.iter()).collect::<Vec<_>>();
    if !images.is_empty() {
        if let Some(msg)=messages.iter_mut().rev().find(|m|m.role=="user") {
            let text=match &msg.content {Some(MessageContent::Text(t))=>*t,_=>""};
            let mut parts=vec![ContentPart::Text{text}];
            for img in images {parts.push(ContentPart::ImageUrl{image_url:ImageUrl{url:format!("data:{};base64,{}",img.media_type,img.data)}});}
            msg.content=Some(MessageContent::Parts(parts));
        }
    }
    // Preserve provider-native conversation roles. A tool result must refer to
    // the actual call ID, rather than impersonating an ordinary user message.
    for turn in &req.native_turns {
        match turn {
            NativeToolTurn::Assistant {content,tool_calls} => messages.push(RequestMessage {
                role:"assistant",
                content:content.as_deref().map(MessageContent::Text),
                tool_calls:if tool_calls.is_empty(){None}else{Some(tool_calls.iter().map(|call|RequestToolCall {
                    id:&call.id,kind:"function",function:RequestFunctionCall {name:&call.name,arguments:&call.arguments},
                }).collect())},
                tool_call_id:None,
            }),
            NativeToolTurn::Tool {tool_call_id,content,..} => messages.push(RequestMessage {
                role:"tool",content:Some(MessageContent::Text(content)),tool_calls:None,tool_call_id:Some(tool_call_id),
            }),
        }
    }
    ChatRequest {
        model: &req.model,
        messages,
        stream: true,
        temperature: req.temperature.clamp(0.0, 2.0),
        max_tokens: req.max_tokens,
        // Omission preserves ordinary chat compatibility. With tools present,
        // Chat Completions defaults tool_choice to auto.
        tools:req.native_tools.iter().map(|tool|RequestTool {
            kind:"function",function:RequestFunctionDefinition {name:&tool.name,description:&tool.description,parameters:&tool.parameters},
        }).collect(),
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
        let stream = parse_sse(sse_lines(raw));
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

/// Native calls may execute only after a complete provider turn. Text-only
/// legacy streams retain their existing EOF behavior.
fn parse_sse<S>(lines:S)->impl Stream<Item=Result<CompletionChunk,AdapterError>>
where S:Stream<Item=Result<String,AdapterError>> {
    async_stream::stream! {
        let mut lines=std::pin::pin!(lines);
        let mut has_native_calls=false;
        let mut complete=false;
        while let Some(line_result)=lines.next().await {
            let line=match line_result {Ok(line)=>line,Err(error)=>{yield Err(error);return;}};
            let Some(payload)=line.strip_prefix("data:").map(str::trim_start) else {continue;};
            if payload.trim()=="[DONE]" {complete=true;break;}
            if let Ok(value)=serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(error)=value.get("error").filter(|error|!error.is_null()) {yield Err(AdapterError::Stream(error.to_string()));return;}
            }
            let frame:StreamFrame=match serde_json::from_str(payload){Ok(frame)=>frame,Err(_)=>continue};
            let finished=frame.choices.first().and_then(|choice|choice.finish_reason.as_deref()).is_some_and(|reason|matches!(reason,"tool_calls"|"stop"));
            if let Some(chunk)=chunk_from_frame(frame){match chunk {Ok(chunk)=>{has_native_calls|=!chunk.tool_call_deltas.is_empty();yield Ok(chunk);},Err(error)=>{yield Err(error);return;}}}
            if finished {complete=true;break;}
        }
        if has_native_calls&&!complete {yield Err(AdapterError::Stream("provider native tool stream ended before completion".into()));}
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
        // HTTP byte chunks may split a Unicode code point, including inside
        // streamed tool arguments. Decode complete SSE lines, never each chunk.
        const MAX_LINE_BYTES:usize=1_048_576;
        let mut buf:Vec<u8> = Vec::new();
        let mut stream = std::pin::pin!(stream);
        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(b) => b,
                Err(err) => {yield Err(err.into());return;}
            };
            buf.extend_from_slice(&chunk);
            while let Some(idx) = buf.iter().position(|byte|*byte==b'\n') {
                if idx>MAX_LINE_BYTES {yield Err(AdapterError::Stream("provider SSE line exceeds size limit".into()));return;}
                let line:Vec<u8>=buf.drain(..=idx).collect();
                let line=match String::from_utf8(line) {Ok(line)=>line,Err(_)=>{yield Err(AdapterError::Stream("provider SSE line is not valid UTF-8".into()));return;}};
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if !trimmed.is_empty() {yield Ok(trimmed.to_string());}
            }
            if buf.len()>MAX_LINE_BYTES {yield Err(AdapterError::Stream("provider SSE line exceeds size limit".into()));return;}
        }
        if !buf.is_empty() {
            match String::from_utf8(buf) {
                Ok(line)=>{let leftover=line.trim();if !leftover.is_empty(){yield Ok(leftover.to_string());}},
                Err(_)=>{yield Err(AdapterError::Stream("provider SSE line is not valid UTF-8".into()));}
            }
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
            native_tools: Vec::new(),
            native_turns: Vec::new(),
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
        let frame:StreamFrame=serde_json::from_str(r#"{"choices":[{"delta":{"content":"compatible text","tool_calls":null},"finish_reason":null}]}"#).unwrap();
        assert_eq!(chunk_from_frame(frame).unwrap().unwrap().text,"compatible text");
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
    #[test]
    fn native_function_schemas_and_real_tool_roles_match_chat_completions_wire() {
        use crate::adapter::{NativeToolCall, NativeToolDefinition};
        let mut req=request(vec![ChatMessage{role:ChatRole::User,content:"Read the project".into()}],None);
        req.native_tools=vec![NativeToolDefinition{name:"read_file".into(),description:"Read an authorized text file".into(),parameters:serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false})}];
        req.native_turns=vec![
            NativeToolTurn::Assistant{content:None,tool_calls:vec![NativeToolCall{id:"call_read_1".into(),name:"read_file".into(),arguments:r#"{"path":"main.tex"}"#.into()}]},
            NativeToolTurn::Tool{tool_call_id:"call_read_1".into(),content:"Original project text".into(),is_error:false},
            NativeToolTurn::Assistant{content:Some("I found the section.".into()),tool_calls:vec![]},
        ];
        let wire=serde_json::to_value(build_request(&req)).unwrap();
        assert_eq!(wire["tools"][0]["type"],"function");
        assert_eq!(wire["tools"][0]["function"]["name"],"read_file");
        assert_eq!(wire["tools"][0]["function"]["parameters"],req.native_tools[0].parameters);
        assert_eq!(wire["messages"][1],serde_json::json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_read_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"main.tex\"}"}}]}));
        assert_eq!(wire["messages"][2],serde_json::json!({"role":"tool","content":"Original project text","tool_call_id":"call_read_1"}));
        assert_eq!(wire["messages"][3],serde_json::json!({"role":"assistant","content":"I found the section."}));
        assert!(wire.get("parallel_tool_calls").is_none());
        assert!(wire.get("api_key").is_none());
    }

    #[test]
    fn ordinary_chat_omits_tool_fields_and_tool_errors_keep_their_original_id() {
        let mut req=request(vec![ChatMessage{role:ChatRole::User,content:"hi".into()}],None);
        let wire=serde_json::to_value(build_request(&req)).unwrap();
        assert!(wire.get("tools").is_none());
        assert!(wire["messages"][0].get("tool_calls").is_none());
        assert!(wire["messages"][0].get("tool_call_id").is_none());
        req.native_turns.push(NativeToolTurn::Tool{tool_call_id:"call_failed".into(),content:r#"{"error":"File unavailable"}"#.into(),is_error:true});
        let wire=serde_json::to_value(build_request(&req)).unwrap();
        assert_eq!(wire["messages"][1]["role"],"tool");
        assert_eq!(wire["messages"][1]["tool_call_id"],"call_failed");
        assert_eq!(wire["messages"][1]["content"],r#"{"error":"File unavailable"}"#);
        assert!(wire["messages"][1].get("is_error").is_none(),"OpenAI uses the tool content rather than Anthropic's is_error field");
    }

    #[test]
    fn interleaved_native_tool_deltas_preserve_indices_and_partial_arguments() {
        let frames=[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read_file","arguments":""}},{"index":1,"id":"call_b","type":"function","function":{"name":"search_project","arguments":""}}]},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"content":"Reading project","tool_calls":[{"index":1,"function":{"arguments":"{\"query\":\""}},{"index":0,"function":{"arguments":"{\"path\":\"main.tex\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"中文\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        ];
        let chunks=frames.iter().map(|raw|chunk_from_frame(serde_json::from_str(raw).unwrap()).unwrap().unwrap()).collect::<Vec<_>>();
        assert_eq!(chunks[0].tool_call_deltas.len(),2);
        assert_eq!(chunks[0].tool_call_deltas[0].id.as_deref(),Some("call_a"));
        assert_eq!(chunks[0].tool_call_deltas[1].name.as_deref(),Some("search_project"));
        assert_eq!(chunks[1].text,"Reading project");
        assert_eq!(chunks[1].tool_call_deltas[0].index,1);
        assert_eq!(chunks[1].tool_call_deltas[0].arguments.as_deref(),Some("{\"query\":\""));
        assert_eq!(chunks[1].tool_call_deltas[1].index,0);
        assert_eq!(chunks[2].tool_call_deltas[0].id,None);
        assert_eq!(chunks[2].tool_call_deltas[0].arguments.as_deref(),Some("中文\"}"));
        let finish:StreamFrame=serde_json::from_str(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#).unwrap();
        assert!(chunk_from_frame(finish).is_none(),"tool_calls is a normal turn boundary");
    }

    #[tokio::test]
    async fn byte_chunks_split_inside_unicode_tool_arguments_are_lossless() {
        let source="data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"中文\"}}]}}]}\r\n\r\ndata:[DONE]";
        let bytes=source.as_bytes();let split=source.find('中').unwrap()+1;
        let raw=futures::stream::iter(vec![Ok::<_,AdapterError>(bytes::Bytes::copy_from_slice(&bytes[..split])),Ok(bytes::Bytes::copy_from_slice(&bytes[split..split+1])),Ok(bytes::Bytes::copy_from_slice(&bytes[split+1..]))]);
        let lines=sse_lines(raw).collect::<Vec<_>>().await.into_iter().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(lines.len(),2);
        assert!(lines[0].contains("中文"));assert_eq!(lines[1],"data:[DONE]");
        let frame:StreamFrame=serde_json::from_str(lines[0].strip_prefix("data:").unwrap().trim_start()).unwrap();
        assert_eq!(chunk_from_frame(frame).unwrap().unwrap().tool_call_deltas[0].arguments.as_deref(),Some("中文"));
    }

    #[tokio::test]
    async fn invalid_utf8_and_oversized_sse_lines_fail_closed() {
        let invalid=futures::stream::iter(vec![Ok::<_,AdapterError>(bytes::Bytes::from_static(b"data: \xff\n"))]);
        let mut invalid_lines=Box::pin(sse_lines(invalid));assert!(matches!(invalid_lines.next().await,Some(Err(AdapterError::Stream(_)))));
        let oversized=futures::stream::iter(vec![Ok::<_,AdapterError>(bytes::Bytes::from(vec![b'x';1_048_577]))]);
        let mut oversized_lines=Box::pin(sse_lines(oversized));assert!(matches!(oversized_lines.next().await,Some(Err(AdapterError::Stream(_)))));
    }

    #[tokio::test]
    async fn native_tool_turn_requires_finish_or_done_but_plain_text_eof_is_compatible() {
        let call=r#"data:{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"list_files","arguments":"{}"}}]}}]}"#;
        for terminal in [Some("data:[DONE]"),Some(r#"data:{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#),Some(r#"data:{"choices":[{"delta":{},"finish_reason":"stop"}]}"#),None] {
            let lines=std::iter::once(Ok::<_,AdapterError>(call.to_string())).chain(terminal.map(|line|Ok(line.to_string()))).collect::<Vec<_>>();
            let chunks=parse_sse(futures::stream::iter(lines)).collect::<Vec<_>>().await;
            assert!(!chunks[0].as_ref().unwrap().tool_call_deltas.is_empty());
            if terminal.is_some(){assert!(chunks.iter().all(Result::is_ok));}else{assert!(matches!(chunks.last(),Some(Err(AdapterError::Stream(_)))));}
        }
        let lines=futures::stream::iter(vec![Ok::<_,AdapterError>(r#"data:{"choices":[{"delta":{"content":"plain answer"}}]}"#.into())]);
        let chunks=parse_sse(lines).collect::<Vec<_>>().await;assert_eq!(chunks.len(),1);assert_eq!(chunks[0].as_ref().unwrap().text,"plain answer");
    }

}
