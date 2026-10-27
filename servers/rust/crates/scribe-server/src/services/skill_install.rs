//! Untrusted SKILL.md previews only. This module deliberately has no database,
//! user-library writer, subprocess runner, or installation side effects.
use scribe_shared::{ApiError, ApiResult};
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

pub const MAX_BYTES: usize = 48 * 1024;
const MAX_PROPOSAL_BYTES: usize = 60_000;

/// Ambiguous slash-containing GitHub refs must be supplied as an explicit raw URL.
pub fn public_skill_url(value: &str) -> ApiResult<reqwest::Url> {
    if value.len() > 2048 || value.bytes().any(|c| c.is_ascii_control() || c == b' ') {
        return Err(ApiError::validation("技能地址无效"));
    }
    let url = reqwest::Url::parse(value).map_err(|_| ApiError::validation("技能地址必须是完整HTTPS地址"))?;
    if url.scheme() != "https" || url.port_or_known_default() != Some(443)
        || !url.username().is_empty() || url.password().is_some() || url.query().is_some()
        || url.fragment().is_some() || value.contains('%') || value.contains('\\')
    {
        return Err(ApiError::validation("仅支持无凭据、查询参数和片段的公开GitHub HTTPS技能地址"));
    }
    // Reject dot components in the original input before URL normalization can erase them.
    let raw_path = value.split_once("://").map(|(_, rest)| rest.split_once('/').map(|(_, path)| path).unwrap_or("")).unwrap_or("");
    if raw_path.split('/').any(|part| part.is_empty() || matches!(part, "." | "..")) {
        return Err(ApiError::validation("技能地址路径不安全"));
    }
    let parts = url.path().trim_start_matches('/').split('/').collect::<Vec<_>>();
    if parts.iter().any(|part| part.is_empty() || !part.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')))
        || parts.last() != Some(&"SKILL.md")
    {
        return Err(ApiError::validation("地址必须指向安全路径下的SKILL.md文件"));
    }
    match url.host_str() {
        Some("raw.githubusercontent.com") if parts.len() >= 4 => Ok(url),
        Some("github.com") if parts.len() >= 5 && parts[2] == "blob" => {
            let mut raw = vec![parts[0], parts[1]];
            raw.extend_from_slice(&parts[3..]);
            reqwest::Url::parse(&format!("https://raw.githubusercontent.com/{}", raw.join("/")))
                .map_err(|_| ApiError::validation("无法解析公开技能地址"))
        }
        _ => Err(ApiError::validation("仅支持github.com文件页或raw.githubusercontent.com的公开SKILL.md；不使用私有仓库凭据")),
    }
}

pub fn validate_call(call: &Value) -> ApiResult<()> {
    let object = call.as_object().ok_or_else(|| ApiError::validation("install_skill需要完整对象"))?;
    if call.get("tool").and_then(Value::as_str) != Some("install_skill") { return Err(ApiError::validation("未授权技能工具")); }
    if object.keys().any(|key| !matches!(key.as_str(), "tool" | "content" | "url")) {
        return Err(ApiError::validation("技能建议只能提供content或url，不能添加执行命令或权限"));
    }
    match (object.get("content"), object.get("url")) {
        (Some(Value::String(content)), None) => { super::agent_library::validate_skill(content)?; Ok(()) }
        (None, Some(Value::String(url))) => { public_skill_url(url)?; Ok(()) }
        _ => Err(ApiError::validation("请仅提供content技能文本或公开SKILL.md的url，二者选一")),
    }
}

pub fn proposal(content: &str, source: Option<&str>) -> ApiResult<Value> {
    let (name, description) = super::agent_library::validate_skill(content)?;
    Ok(json!({"id":Uuid::new_v4(),"name":name,"description":description,
        "content":content,"source":source,"status":"pending_review","installed":false,
        "untrusted":true,"permissionsGranted":false}))
}

async fn fetch_public(url: reqwest::Url) -> ApiResult<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(15))
        .user_agent("Prims-Skill-Preview/1")
        .build().map_err(|_| ApiError::validation("公开技能读取暂不可用"))?;
    let mut response = client.get(url).send().await.map_err(|_| ApiError::validation("公开技能读取失败或超时"))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(ApiError::validation("公开技能读取失败，不跟随重定向，也不使用私有仓库凭据"));
    }
    if response.content_length().is_some_and(|size| size > MAX_BYTES as u64) {
        return Err(ApiError::validation("SKILL.md最多48KiB"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ApiError::validation("技能文件未完整读取"))? {
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES { return Err(ApiError::validation("SKILL.md最多48KiB")); }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| ApiError::validation("SKILL.md必须是UTF-8文本"))
}

#[derive(Default)]
pub struct SkillInstallQueue { attempts: usize, pub items: Vec<Value> }
impl SkillInstallQueue {
    pub fn bytes(&self) -> usize { self.items.iter().map(|item| item.to_string().len()).sum() }
    pub fn metadata(&self) -> Value { json!(self.items.iter().map(|item| json!({"id":item["id"],"name":item["name"],"status":"pending_review","installed":false})).collect::<Vec<_>>()) }
    pub async fn prepare(&mut self, call: &Value, text_bytes: usize) -> ApiResult<(Value, bool)> {
        if self.attempts >= 3 { return Err(ApiError::validation("本轮最多准备3次技能安装建议")); }
        self.attempts += 1;
        validate_call(call)?;
        let (content, source) = if let Some(content) = call.get("content").and_then(Value::as_str) {
            (content.to_string(), None)
        } else {
            let url = public_skill_url(call["url"].as_str().unwrap())?;
            let source = url.to_string();
            let content = tokio::time::timeout(Duration::from_secs(15), fetch_public(url)).await
                .map_err(|_| ApiError::validation("公开技能读取超时"))??;
            (content, Some(source))
        };
        self.submit(&content, source.as_deref(), text_bytes)
    }
    fn submit(&mut self, content: &str, source: Option<&str>, text_bytes: usize) -> ApiResult<(Value, bool)> {
        if let Some(item) = self.items.iter().find(|item| item["content"] == content) { return Ok((item.clone(), false)); }
        let item = proposal(content, source)?;
        if self.bytes().saturating_add(item.to_string().len()).saturating_add(text_bytes) > MAX_PROPOSAL_BYTES {
            return Err(ApiError::validation("本轮待审建议总量已达上限，请先处理后再分批安装技能"));
        }
        self.items.push(item.clone());
        Ok((item, true))
    }
    pub fn merge(&self, output: &mut Value) { output["skillProposals"] = json!(self.items); }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DOC: &str = "---\nname: safe-workflow\ndescription: Review the writing.\n---\nKeep citations intact.\n";
    #[test] fn public_hosts_and_paths_are_exact_and_github_blob_is_converted() {
        assert_eq!(public_skill_url("https://github.com/example/project/blob/main/skills/review/SKILL.md").unwrap().as_str(), "https://raw.githubusercontent.com/example/project/main/skills/review/SKILL.md");
        assert!(public_skill_url("https://raw.githubusercontent.com/example/project/abc123/SKILL.md").is_ok());
        for url in ["http://github.com/a/b/blob/main/SKILL.md", "https://github.com:444/a/b/blob/main/SKILL.md", "https://x:y@github.com/a/b/blob/main/SKILL.md", "https://github.com.evil.test/a/b/blob/main/SKILL.md", "https://github.com/a/b/raw/main/SKILL.md", "https://github.com/a/b/blob/main/install.sh", "https://github.com/a/b/blob/main/../SKILL.md", "https://github.com/a/b/blob/main/%2e%2e/SKILL.md", "https://raw.githubusercontent.com/a/b/main/SKILL.md?token=x", "https://raw.githubusercontent.com/a/b/main/SKILL.md#x"] { assert!(public_skill_url(url).is_err(), "{url}"); }
    }
    #[test] fn only_one_text_or_public_url_and_no_executable_arguments() {
        assert!(validate_call(&json!({"tool":"install_skill","content":DOC})).is_ok());
        for call in [json!({"tool":"install_skill","content":DOC,"url":"https://raw.githubusercontent.com/a/b/main/SKILL.md"}), json!({"tool":"install_skill","command":"install.sh"}), json!({"tool":"install_skill","content":DOC,"enabled":true}), json!({"tool":"install_skill","content":"x".repeat(MAX_BYTES+1)})] { assert!(validate_call(&call).is_err()); }
    }
    #[test] fn preparing_deduplicates_and_never_claims_installation_or_permissions() {
        let mut queue = SkillInstallQueue::default();
        let (first, added) = queue.submit(DOC, None, 0).unwrap(); assert!(added);
        assert_eq!(first["status"], "pending_review"); assert_eq!(first["installed"], false); assert_eq!(first["permissionsGranted"], false);
        let (same, added) = queue.submit(DOC, None, 0).unwrap(); assert!(!added); assert_eq!(first["id"], same["id"]);
        assert!(queue.submit(&DOC.replace("safe-workflow", "another-workflow"), None, 60_000).is_err());
        assert_eq!(queue.items.len(), 1);
    }
    #[tokio::test] async fn three_attempts_are_bounded_without_any_http_or_database() {
        let mut queue = SkillInstallQueue::default();
        for _ in 0..3 { queue.prepare(&json!({"tool":"install_skill","content":DOC}), 0).await.unwrap(); }
        assert!(queue.prepare(&json!({"tool":"install_skill","content":DOC}), 0).await.is_err());
        assert_eq!(queue.items.len(), 1);
    }
}
