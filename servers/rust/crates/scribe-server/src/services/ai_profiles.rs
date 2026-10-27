//! Private named AI configurations; default users.ai_config remains unchanged.
use std::{sync::Arc,collections::HashSet};
use chrono::{DateTime,Utc};
use scribe_ai::CryptoBox;
use scribe_shared::{AIConfigStored,AIProvider,ApiError,ApiResult,UpdateAIConfigInput,UserId};
use serde::{Deserialize,Serialize};
use sqlx::{PgPool,Row,postgres::PgRow};
use uuid::Uuid;

pub struct ProfileInput {pub name:String,pub config:UpdateAIConfigInput,pub models:Vec<String>}
// Keep the legacy config shape while allowing a models-only connection payload.
impl<'de> Deserialize<'de> for ProfileInput{
 fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
  #[derive(Deserialize)]#[serde(rename_all="camelCase")]struct Wire{name:String,provider:AIProvider,#[serde(default)]model:String,#[serde(default)]models:Vec<String>,base_url:Option<String>,api_key:Option<String>}
  let value=Wire::deserialize(deserializer)?;Ok(Self{name:value.name,models:value.models,config:UpdateAIConfigInput{provider:value.provider,model:value.model,base_url:value.base_url,api_key:value.api_key}})
 }
}
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub struct ProfilePublic {pub id:Uuid,pub name:String,pub provider:AIProvider,pub model:String,pub models:Vec<String>,pub base_url:Option<String>,pub api_key_set:bool,pub ready:bool,pub api_key_preview:Option<String>,pub updated_at:DateTime<Utc>}
#[derive(Serialize)]pub struct ProfileList{pub profiles:Vec<ProfilePublic>}
#[derive(Clone)]pub struct AIProfiles{pool:PgPool,crypto:Arc<CryptoBox>}
impl AIProfiles{
 pub fn new(pool:PgPool,crypto:Arc<CryptoBox>)->Self{Self{pool,crypto}}
 pub async fn list(&self,user:UserId)->ApiResult<ProfileList>{let rows=sqlx::query("select id,name,provider,model,models,base_url,api_key_cipher is not null as api_key_set,updated_at from public.scribe_ai_model_profiles where user_id=$1 order by updated_at desc,id limit 20").bind(user.into_inner()).fetch_all(&self.pool).await.map_err(db)?;Ok(ProfileList{profiles:rows.into_iter().map(public).collect::<ApiResult<Vec<_>>>()?})}
 pub async fn create(&self,user:UserId,input:ProfileInput)->ApiResult<ProfilePublic>{
  let input=normalize(input)?;let cipher=self.encrypt_new_key(input.config.api_key.as_deref())?;
  let mut tx=self.pool.begin().await.map_err(db)?;
  // Serialize all creates for this user so concurrent requests cannot exceed the cap.
  sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,48190322))").bind(format!("prims-ai-profiles:{user}")).execute(&mut *tx).await.map_err(db)?;
  let count:i64=sqlx::query_scalar("select count(*) from public.scribe_ai_model_profiles where user_id=$1").bind(user.into_inner()).fetch_one(&mut *tx).await.map_err(db)?;
  if count>=20{return Err(ApiError::validation("每个账号最多保存 20 个模型配置"));}
  let row=sqlx::query("insert into public.scribe_ai_model_profiles(id,user_id,name,provider,model,models,base_url,api_key_cipher) values($1,$2,$3,$4,$5,$6,$7,$8) returning id,name,provider,model,models,base_url,api_key_cipher is not null as api_key_set,updated_at").bind(Uuid::new_v4()).bind(user.into_inner()).bind(input.name).bind(input.config.provider.as_str()).bind(input.config.model).bind(input.models).bind(input.config.base_url).bind(cipher).fetch_one(&mut *tx).await.map_err(db)?;
  tx.commit().await.map_err(db)?;public(row)
 }
 pub async fn update(&self,user:UserId,id:Uuid,input:ProfileInput)->ApiResult<ProfilePublic>{
  let input=normalize(input)?;let cipher=self.encrypt_new_key(input.config.api_key.as_deref())?;
  let row=sqlx::query("update public.scribe_ai_model_profiles set name=$3,provider=$4,model=$5,models=$6,base_url=$7,api_key_cipher=coalesce($8,api_key_cipher),updated_at=now() where user_id=$1 and id=$2 returning id,name,provider,model,models,base_url,api_key_cipher is not null as api_key_set,updated_at").bind(user.into_inner()).bind(id).bind(input.name).bind(input.config.provider.as_str()).bind(input.config.model).bind(input.models).bind(input.config.base_url).bind(cipher).fetch_optional(&self.pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("找不到此模型配置"))?;public(row)
 }
 pub async fn delete(&self,user:UserId,id:Uuid)->ApiResult<()>{let count=sqlx::query("delete from public.scribe_ai_model_profiles where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).execute(&self.pool).await.map_err(db)?.rows_affected();if count==0{return Err(ApiError::not_found("找不到此模型配置"));}Ok(())}
 fn encrypt_new_key(&self,key:Option<&str>)->ApiResult<Option<String>>{key.filter(|key|!key.is_empty()).map(|key|self.crypto.encrypt(key).map_err(|_|ApiError::internal("模型 Key 加密失败"))).transpose()}
 /// The caller's user id is mandatory: another account's UUID is never a selectable config.
 pub async fn load(&self,user:UserId,id:Uuid,model_name:Option<&str>)->ApiResult<AIConfigStored>{
  let row=sqlx::query("select provider,model,models,base_url,api_key_cipher,updated_at from public.scribe_ai_model_profiles where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).fetch_optional(&self.pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("模型配置已删除或不可用，请重新选择"))?;
  let legacy:String=row.get("model");let names=effective_models(&legacy,row.get("models"));let model=choose_model(&legacy,&names,model_name)?;
  Ok(AIConfigStored{provider:provider(&row)?,model,base_url:row.get("base_url"),api_key_encrypted:row.get("api_key_cipher"),api_key_preview:None,updated_at:Some(row.get::<DateTime<Utc>,_>("updated_at").to_rfc3339())})
 }
}
fn provider(row:&PgRow)->ApiResult<AIProvider>{AIProvider::parse(row.get::<String,_>("provider").as_str()).ok_or_else(||ApiError::internal("模型配置类型无效"))}
fn public(row:PgRow)->ApiResult<ProfilePublic>{let provider=provider(&row)?;let api_key_set=row.get("api_key_set");let legacy:String=row.get("model");let models=effective_models(&legacy,row.get("models"));let model=choose_model(&legacy,&models,None).unwrap_or_default();let ready=!models.is_empty()&&profile_ready(provider,api_key_set);Ok(ProfilePublic{id:row.get("id"),name:row.get("name"),provider,model,models,base_url:row.get("base_url"),api_key_set,ready,api_key_preview:None,updated_at:row.get("updated_at")})}
fn effective_models(legacy:&str,models:Vec<String>)->Vec<String>{if models.is_empty(){if legacy.trim().is_empty(){vec![]}else{vec![legacy.to_string()]}}else{models}}
fn choose_model(legacy:&str,models:&[String],requested:Option<&str>)->ApiResult<String>{
 if let Some(name)=requested{let name=name.trim();if name.is_empty()||!models.iter().any(|model|model==name){return Err(ApiError::validation("所选模型不在该连接的允许列表中，请重新选择"));}return Ok(name.to_string());}
 if models.iter().any(|name|name==legacy){return Ok(legacy.to_string());}
 models.first().cloned().ok_or_else(||ApiError::validation("此连接尚未配置模型名称，请先在设置中添加"))
}
pub fn profile_ready(provider:AIProvider,key_set:bool)->bool{key_set||matches!(provider,AIProvider::Ollama|AIProvider::Lmstudio)}
fn normalize(mut input:ProfileInput)->ApiResult<ProfileInput>{
 input.name=input.name.trim().to_string();input.config.model=input.config.model.trim().to_string();
 if input.name.is_empty()||input.name.chars().count()>80||input.name.chars().any(char::is_control){return Err(ApiError::validation("配置名称需要 1–80 字符，且不能包含控制字符"));}
 if input.models.len()>100{return Err(ApiError::validation("每个连接最多保存 100 个模型名称"));}
 if input.config.model.chars().count()>256||input.config.model.chars().any(char::is_control){return Err(ApiError::validation("模型名称最多 256 字符，且不能包含控制字符"));}
 let mut seen=HashSet::new();let mut models=vec![];
 for model in input.models{let model=model.trim();if model.is_empty()||model.chars().count()>256||model.chars().any(char::is_control){return Err(ApiError::validation("每个模型名称需要 1–256 字符，且不能包含控制字符"));}if seen.insert(model.to_string()){models.push(model.to_string());}}
 input.models=effective_models(&input.config.model,models);
 input.config.model=choose_model(&input.config.model,&input.models,None)?;
 input.config.base_url=input.config.base_url.as_deref().map(str::trim).filter(|v|!v.is_empty()).map(str::to_string);
 if input.config.provider.needs_base_url()&&input.config.base_url.is_none(){return Err(ApiError::validation("此模型接口需要 API 地址"));}
 if let Some(base)=input.config.base_url.as_deref(){let parsed=url::Url::parse(base).map_err(|_|ApiError::validation("API 地址必须是有效的 HTTP/HTTPS URL"))?;if base.chars().count()>2048||base.chars().any(char::is_control)||!matches!(parsed.scheme(),"http"|"https")||parsed.host_str().is_none()||!parsed.username().is_empty()||parsed.password().is_some()||parsed.query().is_some()||parsed.fragment().is_some(){return Err(ApiError::validation("API 地址仅支持 HTTP/HTTPS，不包含账号密码、查询参数或片段；Key 请填入专用栏"));}}
 input.config.api_key=input.config.api_key.as_deref().map(str::trim).filter(|v|!v.is_empty()).map(str::to_string);
 if input.config.api_key.as_deref().is_some_and(|key|key.chars().count()>8192||key.chars().any(char::is_control)){return Err(ApiError::validation("模型 Key 过长或含无效控制字符"));}
 Ok(input)
}
pub fn selected_profile_id(input:&scribe_shared::AICompleteInput)->ApiResult<Option<Uuid>>{let id=match input.options.as_ref().and_then(|options|options.get("modelProfileId")){None=>None,Some(value) if value=="default"=>None,Some(value)=>Some(Uuid::parse_str(value).map_err(|_|ApiError::validation("所选模型配置编号无效，请重新选择"))?)};if id.is_none()&&input.options.as_ref().is_some_and(|options|options.contains_key("modelName")){return Err(ApiError::validation("默认 API 仅使用已保存的单个模型；请通过命名连接选择其他模型"));}Ok(id)}
fn db(_:sqlx::Error)->ApiError{tracing::error!("Private model profile database operation failed");ApiError::internal("暂时无法读取或保存模型配置")}
#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 fn input()->ProfileInput{serde_json::from_value(json!({"name":" Named model ","provider":"openai-compatible","model":"custom-model","baseUrl":"http://127.0.0.1:19132/v1","apiKey":"fake-unit-key"})).unwrap()}
 #[test]fn normalizes_profiles_and_blank_keys_mean_retain(){let mut value=input();value.config.api_key=Some("  ".into());let value=normalize(value).unwrap();assert_eq!(value.name,"Named model");assert!(value.config.api_key.is_none());assert_eq!(value.config.base_url.as_deref(),Some("http://127.0.0.1:19132/v1"));}
 #[test]fn validates_names_model_limits_and_endpoint_without_secrets_in_errors(){for base in ["file:///etc/passwd","https://user:secret@example.com/v1","https://example.com?key=secret","https://example.com/#fragment","not a URL",""]{let mut value=input();value.config.base_url=Some(base.into());assert!(normalize(value).is_err());}for label in [String::new(),"界".repeat(81),"bad\nname".into()]{let mut value=input();value.name=label;assert!(normalize(value).is_err());}let mut value=input();value.config.model="m".repeat(257);assert!(normalize(value).is_err());}
 #[test]fn request_model_selection_is_explicit_and_default_is_unchanged(){let mut value:scribe_shared::AICompleteInput=serde_json::from_value(json!({"feature":"chat"})).unwrap();assert!(selected_profile_id(&value).unwrap().is_none());value.options=Some([("modelProfileId".into(),"default".into())].into());assert!(selected_profile_id(&value).unwrap().is_none());let id=Uuid::new_v4();value.options.as_mut().unwrap().insert("modelProfileId".into(),id.to_string());assert_eq!(selected_profile_id(&value).unwrap(),Some(id));value.options.as_mut().unwrap().insert("modelProfileId".into(),"invalid".into());assert!(selected_profile_id(&value).is_err());}
 #[test]fn only_local_providers_can_be_ready_without_a_key(){for provider in [AIProvider::Ollama,AIProvider::Lmstudio]{assert!(profile_ready(provider,false));}for provider in [AIProvider::Openai,AIProvider::Anthropic,AIProvider::Gemini,AIProvider::OpenaiCompatible]{assert!(!profile_ready(provider,false));assert!(profile_ready(provider,true));}}
 #[test]fn one_connection_normalizes_multiple_models_and_keeps_legacy_preference(){let mut value=input();value.config.model="second".into();value.models=vec![" first ".into(),"second".into(),"first".into(),"First".into()];let value=normalize(value).unwrap();assert_eq!(value.models,vec!["first","second","First"]);assert_eq!(value.config.model,"second");assert_eq!(choose_model("second",&value.models,Some("first")).unwrap(),"first");assert!(choose_model("second",&value.models,Some("outsider")).is_err());let value:ProfileInput=serde_json::from_value(json!({"name":"models only","provider":"openai","models":["a","b"]})).unwrap();assert_eq!(normalize(value).unwrap().config.model,"a");assert_eq!(effective_models("legacy",vec![]),vec!["legacy"]);}
 #[test]fn model_lists_are_bounded_and_default_cannot_override_to_unknown_model(){for models in [vec!["".into()],vec!["x".repeat(257)],(0..101).map(|n|format!("model-{n}")).collect()]{let mut value=input();value.models=models;assert!(normalize(value).is_err());}let value:scribe_shared::AICompleteInput=serde_json::from_value(json!({"feature":"chat","options":{"modelProfileId":"default","modelName":"other-model"}})).unwrap();assert!(selected_profile_id(&value).is_err());}
 #[test]fn public_profile_dto_never_serializes_cipher_or_cleartext_key(){let value=ProfilePublic{id:Uuid::new_v4(),name:"demo".into(),provider:AIProvider::Openai,model:"custom".into(),models:vec!["custom".into()],base_url:None,api_key_set:true,ready:true,api_key_preview:None,updated_at:Utc::now()};let json=serde_json::to_value(value).unwrap();assert_eq!(json["apiKeySet"],true);assert!(json.get("apiKey").is_none()&&json.get("apiKeyCipher").is_none());}
}
