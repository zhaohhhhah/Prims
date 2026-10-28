//! Private named AI configurations; default users.ai_config remains unchanged.
use std::{sync::Arc,collections::{HashSet,BTreeMap}};
use chrono::{DateTime,Utc};
use scribe_ai::CryptoBox;
use scribe_shared::{AIConfigStored,AIProvider,ApiError,ApiResult,UpdateAIConfigInput,UserId};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use sqlx::{PgPool,Row,postgres::PgRow};
use uuid::Uuid;

pub struct ProfileInput {pub name:String,pub config:UpdateAIConfigInput,pub models:Vec<String>,pub context_windows:Option<BTreeMap<String,u32>>}
// Keep the legacy config shape while allowing a models-only connection payload.
impl<'de> Deserialize<'de> for ProfileInput{
 fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
  #[derive(Deserialize)]#[serde(rename_all="camelCase")]struct Wire{name:String,provider:AIProvider,#[serde(default)]model:String,#[serde(default)]models:Vec<String>,base_url:Option<String>,api_key:Option<String>,#[serde(default)]context_windows:Option<BTreeMap<String,u32>>}
  let value=Wire::deserialize(deserializer)?;Ok(Self{name:value.name,models:value.models,context_windows:value.context_windows,config:UpdateAIConfigInput{provider:value.provider,model:value.model,base_url:value.base_url,api_key:value.api_key}})
 }
}
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub struct ProfilePublic {pub id:Uuid,pub name:String,pub provider:AIProvider,pub model:String,pub models:Vec<String>,pub context_windows:BTreeMap<String,u32>,pub base_url:Option<String>,pub api_key_set:bool,pub ready:bool,pub api_key_preview:Option<String>,pub updated_at:DateTime<Utc>}
#[derive(Serialize)]#[serde(rename_all="camelCase")]pub struct ProfileList{pub profiles:Vec<ProfilePublic>,pub default_profile_id:Option<Uuid>,pub default_model:Option<String>,pub migration:MigrationStatus}
#[derive(Serialize)]pub struct MigrationStatus{pub status:String,pub message:Option<String>}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]pub struct SetDefaultInput{pub profile_id:Uuid,pub model_name:Option<String>}
#[derive(Clone)]pub struct AIProfiles{pool:PgPool,crypto:Arc<CryptoBox>}
impl AIProfiles{
 pub fn new(pool:PgPool,crypto:Arc<CryptoBox>)->Self{Self{pool,crypto}}
 pub async fn list(&self,user:UserId)->ApiResult<ProfileList>{let migration=self.ensure_legacy(user).await?;let rows=sqlx::query("select id,name,provider,model,models,context_windows,base_url,api_key_cipher is not null as api_key_set,updated_at from public.scribe_ai_model_profiles where user_id=$1 order by updated_at desc,id").bind(user.into_inner()).fetch_all(&self.pool).await.map_err(db)?;let pref=sqlx::query("select default_profile_id,default_model from public.scribe_ai_profile_preferences where user_id=$1").bind(user.into_inner()).fetch_one(&self.pool).await.map_err(db)?;Ok(ProfileList{profiles:rows.into_iter().map(public).collect::<ApiResult<Vec<_>>>()?,default_profile_id:pref.get("default_profile_id"),default_model:pref.get("default_model"),migration})}
 async fn ensure_legacy(&self,user:UserId)->ApiResult<MigrationStatus>{
  let mut tx=self.pool.begin().await.map_err(db)?;sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,48190322))").bind(format!("prims-ai-profiles:{user}")).execute(&mut *tx).await.map_err(db)?;
  sqlx::query("insert into public.scribe_ai_profile_preferences(user_id) values($1) on conflict do nothing").bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;
  let pref=sqlx::query("select legacy_migrated_at,migration_status from public.scribe_ai_profile_preferences where user_id=$1 for update").bind(user.into_inner()).fetch_one(&mut *tx).await.map_err(db)?;
  if pref.get::<Option<DateTime<Utc>>,_>("legacy_migrated_at").is_some(){return Ok(migration_status(pref.get("migration_status")));}
  let raw:Option<Value>=sqlx::query_scalar("select ai_config from public.users where id=$1").bind(user.into_inner()).fetch_optional(&mut *tx).await.map_err(db)?.flatten();
  let Some(raw)=raw.filter(|v|!v.is_null())else{sqlx::query("update public.scribe_ai_profile_preferences set migration_status='none',legacy_migrated_at=now(),updated_at=now() where user_id=$1").bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;return Ok(migration_status("none".into()));};
  let legacy=match serde_json::from_value::<AIConfigStored>(raw){Ok(value)=>value,Err(_)=>{sqlx::query("update public.scribe_ai_profile_preferences set migration_status='invalid',updated_at=now() where user_id=$1").bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;return Ok(migration_status("invalid".into()));}};
  let input=ProfileInput{name:"原默认连接".into(),models:vec![legacy.model.clone()],context_windows:None,config:UpdateAIConfigInput{provider:legacy.provider,model:legacy.model,base_url:legacy.base_url,api_key:None}};
  let input=match normalize(input){Ok(value)=>value,Err(_)=>{sqlx::query("update public.scribe_ai_profile_preferences set migration_status='invalid',updated_at=now() where user_id=$1").bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;return Ok(migration_status("invalid".into()));}};
  // Never emit the cleartext, ciphertext, preview or provider response in a migration error.
  let clear=match legacy.api_key_encrypted.filter(|v|!v.is_empty()).map(|cipher|self.crypto.decrypt(&cipher)).transpose(){Ok(value)=>value,Err(_)=>{sqlx::query("update public.scribe_ai_profile_preferences set migration_status='decrypt_failed',updated_at=now() where user_id=$1").bind(user.into_inner()).execute(&mut *tx).await.map_err(db)?;tx.commit().await.map_err(db)?;return Ok(migration_status("decrypt_failed".into()));}};
  let clear=clear.filter(|key|!key.is_empty());
  let encrypted=clear.as_deref().map(|key|self.crypto.encrypt(key).map_err(|_|ApiError::internal("旧配置迁移加密失败，原配置保持不变"))).transpose()?;
  let candidates=sqlx::query("select id,model,models,api_key_cipher from public.scribe_ai_model_profiles where user_id=$1 and provider=$2 and base_url is not distinct from $3 order by id for update").bind(user.into_inner()).bind(input.config.provider.as_str()).bind(&input.config.base_url).fetch_all(&mut *tx).await.map_err(db)?;
  let mut reused=None;
  for row in candidates{let existing:Option<String>=row.get("api_key_cipher");let old=match existing.map(|cipher|self.crypto.decrypt(&cipher)).transpose(){Ok(value)=>value,Err(_)=>continue};if old!=clear{continue;}let mut models=effective_models(&row.get::<String,_>("model"),row.get("models"));if !models.contains(&input.config.model){models.push(input.config.model.clone());}let id:Uuid=row.get("id");sqlx::query("update public.scribe_ai_model_profiles set models=$3 where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).bind(models).execute(&mut *tx).await.map_err(db)?;reused=Some(id);break;}
  let id=if let Some(id)=reused{id}else{let id=Uuid::new_v4();sqlx::query("insert into public.scribe_ai_model_profiles(id,user_id,name,provider,model,models,base_url,api_key_cipher) values($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(user.into_inner()).bind(input.name).bind(input.config.provider.as_str()).bind(&input.config.model).bind(input.models).bind(input.config.base_url).bind(encrypted).execute(&mut *tx).await.map_err(db)?;id};
  let ready=profile_ready(input.config.provider,clear.as_ref().is_some_and(|key|!key.is_empty()));
  sqlx::query("update public.scribe_ai_profile_preferences set legacy_profile_id=$2,legacy_migrated_at=now(),migration_status='migrated',default_profile_id=case when default_profile_id is null and $4 then $2 else default_profile_id end,default_model=case when default_profile_id is null and $4 then $3 else default_model end,updated_at=now() where user_id=$1").bind(user.into_inner()).bind(id).bind(input.config.model).bind(ready).execute(&mut *tx).await.map_err(db)?;
  // users.ai_config remains an unchanged encrypted backup; repeat GETs cannot create another copy.
  tx.commit().await.map_err(db)?;Ok(migration_status("migrated".into()))
 }
 pub async fn set_default(&self,user:UserId,input:SetDefaultInput)->ApiResult<ProfileList>{
  self.ensure_legacy(user).await?;let stored=self.load(user,input.profile_id,input.model_name.as_deref()).await?;if !profile_ready(stored.provider,stored.api_key_encrypted.is_some()){return Err(ApiError::validation("请先填写此云连接的 API Key，再设为默认"));}
  sqlx::query("update public.scribe_ai_profile_preferences set default_profile_id=$2,default_model=$3,updated_at=now() where user_id=$1").bind(user.into_inner()).bind(input.profile_id).bind(stored.model).execute(&self.pool).await.map_err(db)?;self.list(user).await
 }
 pub async fn resolve_default(&self,user:UserId)->ApiResult<Option<AIConfigStored>>{
  self.ensure_legacy(user).await?;let row=sqlx::query("select default_profile_id,default_model from public.scribe_ai_profile_preferences where user_id=$1").bind(user.into_inner()).fetch_one(&self.pool).await.map_err(db)?;let id:Option<Uuid>=row.get("default_profile_id");let Some(id)=id else{return Ok(None)};let model:Option<String>=row.get("default_model");let mut stored=self.load(user,id,model.as_deref()).await?;stored.api_key_preview=stored.api_key_encrypted.as_ref().map(|_|"已配置".into());Ok(Some(stored))
 }
 /// Backward-compatible API writes address the real named default, never a disconnected shadow form.
 pub async fn compat_update(&self,user:UserId,config:UpdateAIConfigInput)->ApiResult<AIConfigStored>{
  self.ensure_legacy(user).await?;let preferred=config.model.clone();let id:Option<Uuid>=sqlx::query_scalar("select default_profile_id from public.scribe_ai_profile_preferences where user_id=$1").bind(user.into_inner()).fetch_one(&self.pool).await.map_err(db)?;
  let profile=if let Some(id)=id{let row=sqlx::query("select name,model,models from public.scribe_ai_model_profiles where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).fetch_one(&self.pool).await.map_err(db)?;let mut models=effective_models(&row.get::<String,_>("model"),row.get("models"));if !models.contains(&preferred){models.push(preferred.clone());}self.update(user,id,ProfileInput{name:row.get("name"),models,context_windows:None,config}).await?}else{self.create(user,ProfileInput{name:"默认连接".into(),models:vec![preferred.clone()],context_windows:None,config}).await?};
  self.set_default(user,SetDefaultInput{profile_id:profile.id,model_name:Some(preferred)}).await?;self.resolve_default(user).await?.ok_or_else(||ApiError::validation("请设置默认模型"))
 }
 pub async fn create(&self,user:UserId,input:ProfileInput)->ApiResult<ProfilePublic>{
  self.ensure_legacy(user).await?;let input=normalize(input)?;let cipher=self.encrypt_new_key(input.config.api_key.as_deref())?;
  let mut tx=self.pool.begin().await.map_err(db)?;
  // Serialize creates and default selection for this account.
  sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,48190322))").bind(format!("prims-ai-profiles:{user}")).execute(&mut *tx).await.map_err(db)?;
  let row=sqlx::query("insert into public.scribe_ai_model_profiles(id,user_id,name,provider,model,models,base_url,api_key_cipher,context_windows) values($1,$2,$3,$4,$5,$6,$7,$8,$9) returning id,name,provider,model,models,context_windows,base_url,api_key_cipher is not null as api_key_set,updated_at").bind(Uuid::new_v4()).bind(user.into_inner()).bind(input.name).bind(input.config.provider.as_str()).bind(input.config.model).bind(input.models).bind(input.config.base_url).bind(cipher).bind(serde_json::to_value(input.context_windows.unwrap_or_default()).unwrap()).fetch_one(&mut *tx).await.map_err(db)?;
  let value=public(row)?;if value.ready{sqlx::query("update public.scribe_ai_profile_preferences set default_profile_id=$2,default_model=$3,updated_at=now() where user_id=$1 and default_profile_id is null").bind(user.into_inner()).bind(value.id).bind(&value.model).execute(&mut *tx).await.map_err(db)?;}tx.commit().await.map_err(db)?;Ok(value)
 }
 pub async fn update(&self,user:UserId,id:Uuid,input:ProfileInput)->ApiResult<ProfilePublic>{
  let input=normalize(input)?;let cipher=self.encrypt_new_key(input.config.api_key.as_deref())?;
  let row=sqlx::query("update public.scribe_ai_model_profiles set name=$3,provider=$4,model=$5,models=$6,base_url=$7,api_key_cipher=coalesce($8,api_key_cipher),context_windows=coalesce($9,context_windows),updated_at=now() where user_id=$1 and id=$2 returning id,name,provider,model,models,context_windows,base_url,api_key_cipher is not null as api_key_set,updated_at").bind(user.into_inner()).bind(id).bind(input.name).bind(input.config.provider.as_str()).bind(input.config.model).bind(input.models).bind(input.config.base_url).bind(cipher).bind(input.context_windows.map(|v|serde_json::to_value(v).unwrap())).fetch_optional(&self.pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("找不到此模型配置"))?;let value=public(row)?;sqlx::query("update public.scribe_ai_profile_preferences set default_model=$3,updated_at=now() where user_id=$1 and default_profile_id=$2 and not(coalesce(default_model,'')=any($4))").bind(user.into_inner()).bind(id).bind(&value.model).bind(&value.models).execute(&self.pool).await.map_err(db)?;Ok(value)
 }
 pub async fn delete(&self,user:UserId,id:Uuid)->ApiResult<()>{let count=sqlx::query("delete from public.scribe_ai_model_profiles where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).execute(&self.pool).await.map_err(db)?.rows_affected();if count==0{return Err(ApiError::not_found("找不到此模型配置"));}sqlx::query("update public.scribe_ai_profile_preferences set default_model=null where user_id=$1 and default_profile_id is null").bind(user.into_inner()).execute(&self.pool).await.map_err(db)?;Ok(())}
 fn encrypt_new_key(&self,key:Option<&str>)->ApiResult<Option<String>>{key.filter(|key|!key.is_empty()).map(|key|self.crypto.encrypt(key).map_err(|_|ApiError::internal("模型 Key 加密失败"))).transpose()}
 pub async fn update_context_window(&self,user:UserId,id:Uuid,model:&str,tokens:u32)->ApiResult<ProfilePublic>{
  if !((super::context_policy::MIN_WINDOW as u32)..=(super::context_policy::MAX_WINDOW as u32)).contains(&tokens){return Err(ApiError::validation("上下文长度需为16384至2097152的整数"));}
  let row=sqlx::query("update public.scribe_ai_model_profiles set context_windows=jsonb_set(context_windows,array[$3]::text[],to_jsonb($4::int),true),updated_at=now() where user_id=$1 and id=$2 and ($3=any(models) or (cardinality(models)=0 and $3=model)) returning id,name,provider,model,models,context_windows,base_url,api_key_cipher is not null as api_key_set,updated_at").bind(user.into_inner()).bind(id).bind(model).bind(tokens as i32).fetch_optional(&self.pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("此模型不属于你的连接"))?;public(row)
 }
 /// Read only the authenticated account's selected model limit; credentials never enter this path.
 pub async fn context_policy(&self,user:UserId,input:&scribe_shared::AICompleteInput)->ApiResult<super::context_policy::ContextPolicy>{
  self.ensure_legacy(user).await?;
  let explicit=selected_profile_id(input)?;
  let (id,requested)=if let Some(id)=explicit{(id,input.options.as_ref().and_then(|o|o.get("modelName")).cloned())}else{
   let row=sqlx::query("select default_profile_id,default_model from public.scribe_ai_profile_preferences where user_id=$1").bind(user.into_inner()).fetch_one(&self.pool).await.map_err(db)?;
   let id:Option<Uuid>=row.get("default_profile_id");let Some(id)=id else{return Ok(super::context_policy::ContextPolicy::default())};(id,row.get("default_model"))
  };
  let row=sqlx::query("select model,models,context_windows from public.scribe_ai_model_profiles where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).fetch_optional(&self.pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("找不到此模型配置"))?;
  let legacy:String=row.get("model");let models=effective_models(&legacy,row.get("models"));let model=choose_model(&legacy,&models,requested.as_deref())?;
  let windows:Value=row.get("context_windows");let tokens=windows.get(&model).and_then(Value::as_u64).map(|v|v as usize).unwrap_or(super::context_policy::DEFAULT_WINDOW);
  Ok(super::context_policy::ContextPolicy::new(tokens))
 }
 /// The caller's user id is mandatory: another account's UUID is never a selectable config.
 pub async fn load(&self,user:UserId,id:Uuid,model_name:Option<&str>)->ApiResult<AIConfigStored>{
  let row=sqlx::query("select provider,model,models,base_url,api_key_cipher,updated_at from public.scribe_ai_model_profiles where user_id=$1 and id=$2").bind(user.into_inner()).bind(id).fetch_optional(&self.pool).await.map_err(db)?.ok_or_else(||ApiError::not_found("模型配置已删除或不可用，请重新选择"))?;
  let legacy:String=row.get("model");let names=effective_models(&legacy,row.get("models"));let model=choose_model(&legacy,&names,model_name)?;
  Ok(AIConfigStored{provider:provider(&row)?,model,base_url:row.get("base_url"),api_key_encrypted:row.get("api_key_cipher"),api_key_preview:None,updated_at:Some(row.get::<DateTime<Utc>,_>("updated_at").to_rfc3339())})
 }
}
fn migration_status(status:String)->MigrationStatus{let message=match status.as_str(){"invalid"=>Some("旧默认配置格式无法直接迁移，备份保持不变；请创建一个有效连接并设为默认。"),"decrypt_failed"=>Some("旧配置密钥暂时无法解密，备份保持不变；请管理员检查加密配置。"),_=>None};MigrationStatus{status,message:message.map(str::to_string)}}
fn provider(row:&PgRow)->ApiResult<AIProvider>{AIProvider::parse(row.get::<String,_>("provider").as_str()).ok_or_else(||ApiError::internal("模型配置类型无效"))}
fn public(row:PgRow)->ApiResult<ProfilePublic>{let provider=provider(&row)?;let api_key_set=row.get("api_key_set");let legacy:String=row.get("model");let models=effective_models(&legacy,row.get("models"));let model=choose_model(&legacy,&models,None).unwrap_or_default();let ready=!models.is_empty()&&profile_ready(provider,api_key_set);Ok(ProfilePublic{id:row.get("id"),name:row.get("name"),provider,model,models,context_windows:serde_json::from_value(row.get("context_windows")).map_err(|_|ApiError::internal("上下文设置格式无效"))?,base_url:row.get("base_url"),api_key_set,ready,api_key_preview:None,updated_at:row.get("updated_at")})}
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
 if input.config.model.chars().count()>256||input.config.model.chars().any(char::is_control){return Err(ApiError::validation("模型名称最多 256 字符，且不能包含控制字符"));}
 let mut seen=HashSet::new();let mut models=vec![];
 for model in input.models{let model=model.trim();if model.is_empty()||model.chars().count()>256||model.chars().any(char::is_control){return Err(ApiError::validation("每个模型名称需要 1–256 字符，且不能包含控制字符"));}if seen.insert(model.to_string()){models.push(model.to_string());}}
 input.models=effective_models(&input.config.model,models);
 input.config.model=choose_model(&input.config.model,&input.models,None)?;
 if let Some(windows)=input.context_windows.as_ref(){
  if windows.iter().any(|(model,tokens)|!input.models.contains(model)||!((super::context_policy::MIN_WINDOW as u32)..=(super::context_policy::MAX_WINDOW as u32)).contains(tokens)){return Err(ApiError::validation("上下文长度需为16384至2097152的整数，并对应已配置的模型名称"));}
 }

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
 #[test]fn model_names_are_validated_and_default_cannot_override_to_unknown_model(){for models in [vec!["".into()],vec!["x".repeat(257)]]{let mut value=input();value.models=models;assert!(normalize(value).is_err());}let value:scribe_shared::AICompleteInput=serde_json::from_value(json!({"feature":"chat","options":{"modelProfileId":"default","modelName":"other-model"}})).unwrap();assert!(selected_profile_id(&value).is_err());}
 #[test]fn large_model_catalogs_and_their_context_windows_are_preserved(){let mut value=input();value.models=(0..250).map(|n|format!("model-{n}")).collect();value.context_windows=Some(value.models.iter().map(|name|(name.clone(),262144)).collect());let result=normalize(value).unwrap();assert_eq!(result.models.len(),250);assert_eq!(result.context_windows.unwrap().len(),250);}
 #[test]fn public_profile_dto_never_serializes_cipher_or_cleartext_key(){let value=ProfilePublic{id:Uuid::new_v4(),name:"demo".into(),provider:AIProvider::Openai,model:"custom".into(),models:vec!["custom".into()],context_windows:BTreeMap::new(),base_url:None,api_key_set:true,ready:true,api_key_preview:None,updated_at:Utc::now()};let json=serde_json::to_value(value).unwrap();assert_eq!(json["apiKeySet"],true);assert!(json.get("apiKey").is_none()&&json.get("apiKeyCipher").is_none());}
}

#[cfg(test)]mod context_window_tests{use super::*;#[test]fn limits_are_per_model_validated_and_legacy_omission_is_preserved(){let make=|windows:Value|serde_json::from_value::<ProfileInput>(serde_json::json!({"name":"model limits","provider":"openai","models":["a","b"],"contextWindows":windows}));let valid=normalize(make(serde_json::json!({"a":262144,"b":65536})).unwrap()).unwrap();assert_eq!(valid.context_windows.unwrap()["a"],262144);for value in [serde_json::json!({"a":100}),serde_json::json!({"a":2097153}),serde_json::json!({"unknown":65536})]{assert!(normalize(make(value).unwrap()).is_err());}for value in [serde_json::json!({"a":1.5}),serde_json::json!({"a":-1})]{assert!(make(value).is_err());}let old:ProfileInput=serde_json::from_value(serde_json::json!({"name":"old","provider":"openai","model":"a"})).unwrap();assert!(old.context_windows.is_none());}}
