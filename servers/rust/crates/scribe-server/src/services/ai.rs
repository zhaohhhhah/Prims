//! AI configuration + completion dispatch.
//!
//! Mirrors `server/src/services/aiService.ts`. Config lives in
//! named per-user profiles; legacy users.ai_config is preserved as an encrypted backup.
//! The completion endpoint:
//!
//!   1. Loads the user's saved config (provider/model/baseUrl/key).
//!   2. Decrypts the API key.
//!   3. Builds the chat messages (system prompt + selection + history).
//!   4. Dispatches to the right [`scribe_ai::Adapter`] and returns its
//!      token stream straight back to the route handler.

use futures::{stream::BoxStream,StreamExt};
use scribe_ai::{adapter_for, system_prompt, AdapterError, CompleteRequest, CompletionChunk, CryptoBox};
use scribe_shared::{
    AICompleteInput, AIConfigPublic, AIConfigStored, AIFeature, AIPingResult,
    AIProvider, ApiError, ApiResult, ChatMessage, ChatRole, ErrorCode, UpdateAIConfigInput, UserId,
};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;

pub enum AIStreamStartError { Request(ApiError), Provider(AdapterError) }
impl From<ApiError> for AIStreamStartError {fn from(error:ApiError)->Self{Self::Request(error)}}
impl AIStreamStartError {
 pub fn into_api_error(self)->ApiError{match self{Self::Request(error)=>error,Self::Provider(AdapterError::OutputLimit)=>ApiError::validation(AdapterError::OutputLimit.to_string()),Self::Provider(_)=>ApiError::new(ErrorCode::ServiceUnavailable,"模型接口请求失败，请检查接口配置、模型权限及网络后重试")}}
 pub fn is_image_unsupported(&self)->bool{matches!(self,Self::Provider(error) if error.is_image_unsupported())}
}

#[derive(Clone)]
pub struct AIService {
    pool: PgPool,
    crypto: Arc<CryptoBox>,
}

impl AIService {
    pub fn new(pool: PgPool, crypto: Arc<CryptoBox>) -> Self {
        Self { pool, crypto }
    }

    /// Public-facing config for the current user. `None` if they haven't
    /// configured a provider yet.
    pub async fn get_config(&self, user: UserId) -> ApiResult<Option<AIConfigPublic>> {
        let stored = super::ai_profiles::AIProfiles::new(self.pool.clone(),self.crypto.clone()).resolve_default(user).await?;
        Ok(stored.map(|s| s.to_public()))
    }

    pub async fn update_config(
        &self,
        user: UserId,
        input: UpdateAIConfigInput,
    ) -> ApiResult<AIConfigPublic> {
        Ok(super::ai_profiles::AIProfiles::new(self.pool.clone(),self.crypto.clone()).compat_update(user,input).await?.to_public())
    }

    /// Verify the user's saved config by talking to the provider. Doesn't
    /// throw — failures land in the `error` field of the returned struct.
    pub async fn ping(&self, user: UserId) -> ApiResult<AIPingResult> {
        let Some(stored) = super::ai_profiles::AIProfiles::new(self.pool.clone(),self.crypto.clone()).resolve_default(user).await? else {
            return Ok(AIPingResult {
                ok: false,
                model: None,
                latency_ms: 0,
                error: Some("AI not configured".into()),
            });
        };
        let api_key=match self.decrypt_key(&stored)?{Some(key)=>key,None if super::ai_profiles::profile_ready(stored.provider,false)=>String::new(),None=>return Ok(AIPingResult{ok:false,model:Some(stored.model),latency_ms:0,error:Some("请先配置默认连接的 API Key".into())})};
        let req = CompleteRequest {
            provider: stored.provider,
            model: stored.model.clone(),
            base_url: stored.base_url.clone(),
            api_key,
            messages: Vec::new(),
            image: None,
            project_images: Vec::new(),
            temperature: 0.0,
            max_tokens: Some(1),
        };
        let start = Instant::now();
        let adapter = adapter_for(stored.provider);
        let result = adapter.ping(req).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(model) => Ok(AIPingResult { ok: true, model, latency_ms, error: None }),
            Err(err) => Ok(AIPingResult {
                ok: false,
                model: Some(stored.model),
                latency_ms,
                error: Some(err.to_string()),
            }),
        }
    }

    /// Build a token stream for the requested completion. The route
    /// handler converts this into SSE frames.
    pub async fn complete_stream(
        &self,
        user: UserId,
        input: AICompleteInput,
    ) -> ApiResult<BoxStream<'static, Result<CompletionChunk, AdapterError>>> {
        let stream=self.complete_stream_for_agent(user,input).await.map_err(AIStreamStartError::into_api_error)?;
        Ok(Box::pin(stream.map(|chunk|chunk.map_err(|error|match error{AdapterError::OutputLimit=>AdapterError::OutputLimit,_=>AdapterError::Stream("模型接口返回错误，请检查接口配置、权限及网络后重试".into())}))))
    }

    pub async fn complete_stream_for_agent(
        &self,
        user: UserId,
        input: AICompleteInput,
    ) -> Result<BoxStream<'static, Result<CompletionChunk, AdapterError>>,AIStreamStartError> {
        let profile_id=super::ai_profiles::selected_profile_id(&input)?;
        let stored=if let Some(id)=profile_id{
            super::ai_profiles::AIProfiles::new(self.pool.clone(),self.crypto.clone()).load(user,id,input.options.as_ref().and_then(|options|options.get("modelName")).map(String::as_str)).await?
        }else{
            super::ai_profiles::AIProfiles::new(self.pool.clone(),self.crypto.clone()).resolve_default(user).await?.ok_or_else(||ApiError::new(ErrorCode::Forbidden,"请在设置中选择一个可用的默认模型"))?
        };
        let api_key=match self.decrypt_key(&stored)?{
            Some(key)=>key,
            None if super::ai_profiles::profile_ready(stored.provider,false)=>String::new(),
            None=>return Err(ApiError::new(ErrorCode::Forbidden,"所选模型尚未填写 API Key，请先在设置中完成配置").into()),
        };

        let inline = super::inline_prompt::is_inline(&input);
        let system = if inline && input.options.as_ref().and_then(|o|o.get("projectTools")).is_some_and(|v|v=="enabled") { super::project_context::AGENT_SYSTEM } else if inline { super::inline_prompt::INLINE_SYSTEM } else { system_prompt(input.feature) };
        // Cap the history we forward to the upstream LLM. Long chat
        // sessions can otherwise drag many MB of context through every
        // request — costs money and risks hitting provider limits.
        // Keep the most recent N turns (trim from the front).
        const MAX_HISTORY_MESSAGES: usize = 40;
        let inline_text = inline.then(|| super::inline_prompt::inline_message(&input));
        let history = input.history.unwrap_or_default();
        let history_skip = history.len().saturating_sub(MAX_HISTORY_MESSAGES);
        let mut messages: Vec<ChatMessage> =
            Vec::with_capacity(history.len().min(MAX_HISTORY_MESSAGES) + 2);
        messages.push(ChatMessage { role: ChatRole::System, content: system.to_string() });
        messages.extend(history.into_iter().skip(history_skip).filter(|message| message.role != ChatRole::System));
        let user_text = if inline {
            inline_text.unwrap_or_default()
        } else if !input.selection.is_empty() {
            input.selection
        } else {
            input.options.as_ref().and_then(|o| o.get("message")).cloned().unwrap_or_default()
        };
        if !user_text.is_empty() {
            messages.push(ChatMessage { role: ChatRole::User, content: user_text });
        } else if input.image.is_some() {
            // Vision features (e.g. handwriting OCR) may carry no text
            // selection. The adapters attach the image to the last user
            // message, so synthesise a minimal instruction to hang it on.
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: "Transcribe the mathematics in this image into a single LaTeX expression."
                    .to_string(),
            });
        }

        // Only project_agent sets this after stripping the client-supplied option. Keep
        // tool execution results after the original question, so this is a real continuation.
        if inline && input.options.as_ref().and_then(|o|o.get("projectTools")).is_some_and(|v|v=="enabled") {
            if let Some(turns)=input.options.as_ref().and_then(|o|o.get("projectContinuation")).and_then(|raw|serde_json::from_str::<Vec<ChatMessage>>(raw).ok()) {
                messages.extend(turns.into_iter().filter(|message|matches!(message.role,ChatRole::User|ChatRole::Assistant)).take(12));
            }
        }

        // OCR wants determinism and only needs a short reply; the other
        // features keep the conversational defaults.
        let (temperature, max_tokens) = match input.feature {
            AIFeature::RecognizeEquation => (0.0, Some(512)),
            _ if inline => (0.3, Some(4096)),
            _ => (0.4, Some(2048)),
        };

        let req = CompleteRequest {
            provider: stored.provider,
            model: stored.model,
            base_url: stored.base_url,
            api_key,
            messages,
            image: input.image,
            project_images: input.project_images,
            temperature,
            max_tokens,
        };
        let adapter = adapter_for(stored.provider);
        adapter
            .stream(req)
            .await
            .map_err(AIStreamStartError::Provider)
    }

    fn decrypt_key(&self, stored: &AIConfigStored) -> ApiResult<Option<String>> {
        let Some(encrypted) = stored.api_key_encrypted.as_deref() else {
            return Ok(None);
        };
        if encrypted.is_empty() {
            return Ok(None);
        }
        match self.crypto.decrypt(encrypted) {
            Ok(plain) => Ok(Some(plain)),
            Err(_) => Err(ApiError::new(ErrorCode::Internal,"模型密钥无法解密，请检查服务器加密配置")),
        }
    }
}

/// Provider used by the route layer so the AppState doesn't need to
/// hold every adapter — just the crypto box + db pool. (Apparently
/// unused here, but exported for symmetry with the other services.)
#[allow(dead_code)]
pub fn providers() -> &'static [AIProvider] {
    &[
        AIProvider::Openai,
        AIProvider::Anthropic,
        AIProvider::Gemini,
        AIProvider::Ollama,
        AIProvider::Lmstudio,
        AIProvider::OpenaiCompatible,
    ]
}
