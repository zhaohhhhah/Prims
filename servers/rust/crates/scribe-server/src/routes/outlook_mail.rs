//! Site-wide personal Outlook sender. OAuth credentials never leave the server.
use axum::{body::Bytes,extract::{Json,Query,State},http::{HeaderMap,HeaderValue},middleware::from_fn,response::{IntoResponse,Redirect,Response},routing::{get,post},Router};
use base64::{engine::general_purpose::{STANDARD,URL_SAFE_NO_PAD},Engine};
use chrono::Utc;
use scribe_ai::CryptoBox;
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::{ApiError,ApiResult,ErrorCode};
use serde::Deserialize;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use sqlx::{PgPool,Row};
use std::{sync::Arc,time::Duration};
use uuid::Uuid;
use crate::state::AppState;
/// One trusted operator-set origin; never derive mail links from Host or request JSON.
fn parse_public_site(value:&str)->ApiResult<String>{
 if value.len()>2048||value.chars().any(char::is_control){return Err(ApiError::validation("PRIMS_PUBLIC_URL 格式无效"));}
 let uri=url::Url::parse(value).map_err(|_|ApiError::validation("PRIMS_PUBLIC_URL 必须是有效的 HTTPS 站点地址"))?;
 if uri.scheme()!="https"||!uri.has_host()||!uri.username().is_empty()||uri.password().is_some()||uri.query().is_some()||uri.fragment().is_some()||uri.path()!="/"{return Err(ApiError::validation("PRIMS_PUBLIC_URL 只允许 HTTPS 域名与可选端口，不允许账号、路径、查询或片段"));}
 Ok(uri.origin().ascii_serialization())
}
fn public_site()->ApiResult<String>{parse_public_site(&std::env::var("PRIMS_PUBLIC_URL").map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"管理员需先配置 PRIMS_PUBLIC_URL，邮件功能保持关闭"))?)}
fn outlook_callback_uri(site:&str)->ApiResult<String>{
 let uri=url::Url::parse(site).map_err(|_|ApiError::validation("站点地址无效"))?;
 if uri.host_str().is_some_and(|host|host.split('.').any(|part|part.starts_with("xn--"))){return Err(ApiError::validation("Microsoft Entra 不支持 IDN 回调域名，请配置纯英文域名后再连接 Outlook"));}
 Ok(format!("{site}/api/mail/outlook/callback"))
}
const SCOPES:&str="openid email offline_access https://graph.microsoft.com/Mail.Send";
const TOKEN_URL:&str="https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

pub fn router()->Router<AppState>{
 let admin=Router::new().route("/api/admin/mail/outlook",get(config).put(save))
 .route("/api/admin/mail/outlook/connect",post(connect)).route("/api/admin/mail/outlook/disconnect",post(disconnect))
 .route("/api/admin/mail/outlook/hook-config",post(apply_hook)).route("/api/admin/mail/outlook/test",post(test_mail)).layer(from_fn(require_auth));
 Router::new().merge(admin).route("/api/mail/outlook/callback",get(callback)).route("/api/mail/outlook/hook",post(hook))
}
fn db_err(_:sqlx::Error)->ApiError{ApiError::internal("邮件配置数据库暂时不可用")}
fn crypto(state:&AppState)->ApiResult<Arc<CryptoBox>>{state.inner.ai_crypto.clone().ok_or_else(||ApiError::internal("服务器加密功能未配置"))}
fn enc(c:&CryptoBox,s:&str)->ApiResult<String>{c.encrypt(s).map_err(|_|ApiError::internal("邮件凭据加密失败"))}
fn dec(c:&CryptoBox,s:&str)->ApiResult<String>{c.decrypt(s).map_err(|_|ApiError::internal("邮件凭据无法解密，请重新配置"))}
fn pool(state:&AppState)->ApiResult<PgPool>{state.db().map(|d|d.pool().clone()).ok_or_else(||ApiError::internal("数据库未配置"))}
async fn admin(state:&AppState,user:Uuid)->ApiResult<PgPool>{let p=pool(state)?;let ok:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins where user_id=$1)").bind(user).fetch_one(&p).await.map_err(db_err)?;if !ok{return Err(ApiError::forbidden("仅管理员可配置发信邮箱"));}Ok(p)}
fn digest(s:&[u8])->String{Sha256::digest(s).iter().map(|b|format!("{b:02x}")).collect()}
fn random()->String{format!("{}{}",Uuid::new_v4().simple(),Uuid::new_v4().simple())}
fn http()->ApiResult<reqwest::Client>{reqwest::Client::builder().timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|ApiError::internal("邮件网络组件不可用"))}
async fn public(p:&PgPool)->ApiResult<Value>{
 let r=sqlx::query("select * from public.scribe_outlook_mail where singleton=true").fetch_one(p).await.map_err(db_err)?;
 let site=public_site()?;let callback=format!("{site}/api/mail/outlook/callback");
 let hook=control(None).await.ok();
 let refresh:Option<String>=r.get("refresh_cipher");let reauth:bool=r.get("needs_reauth");let mailbox:Option<String>=r.get("mailbox");
 Ok(json!({"clientId":r.get::<String,_>("client_id"),"clientSecretSet":r.get::<Option<String>,_>("client_secret_cipher").is_some(),"mailbox":mailbox,"connected":refresh.is_some()&&!reauth,"enabled":r.get::<bool,_>("enabled"),"needsReauth":reauth,"lastError":r.get::<Option<String>,_>("last_error"),"tokenExpiresAt":r.get::<Option<i64>,_>("expires_at"),"redirectUri":callback,"scopes":SCOPES,"hookConfigured":hook.as_ref().and_then(|v|v["enabled"].as_bool()).unwrap_or(false),"hookAvailable":hook.is_some(),"requireConfirmation":hook.as_ref().and_then(|v|v["requireConfirmation"].as_bool()).unwrap_or(false)}))
}
async fn control(body:Option<Value>)->ApiResult<Value>{
 let token=std::env::var("SCRIBE_MAIL_CONTROL_TOKEN").map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"私有邮件控制器尚未安装"))?;
 let client=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(if body.is_some(){110}else{3})).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|ApiError::internal("私有邮件控制器不可用"))?;
 let url="http://127.0.0.1:19101/outlook-config";
 let request=if let Some(value)=body{client.post(url).json(&value)}else{client.get(url)};
 let response=request.bearer_auth(token).send().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"私有邮件控制器不可用"))?;
 if !response.status().is_success(){return Err(ApiError::new(ErrorCode::ServiceUnavailable,"邮件 Hook 应用失败，请检查私有控制器及认证服务；原配置已尝试恢复"));}
 response.json().await.map_err(|_|ApiError::internal("邮件控制器响应无效"))
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]struct HookConfig{enabled:bool,require_confirmation:bool}
async fn apply_hook(State(s):State<AppState>,Authenticated(u):Authenticated,Json(input):Json<HookConfig>)->ApiResult<Json<Value>>{
 let p=admin(&s,u.id.into_inner()).await?;
 if input.enabled{let ready:bool=sqlx::query_scalar("select enabled and refresh_cipher is not null and not needs_reauth from public.scribe_outlook_mail where singleton=true").fetch_one(&p).await.map_err(db_err)?;if !ready{return Err(ApiError::validation("请先连接 Outlook、完成测试并允许发信"));}}
 control(Some(json!({"enabled":input.enabled,"requireConfirmation":input.require_confirmation}))).await?;
 Ok(Json(public(&p).await?))
}
async fn config(State(s):State<AppState>,Authenticated(u):Authenticated)->ApiResult<Json<Value>>{let p=admin(&s,u.id.into_inner()).await?;Ok(Json(public(&p).await?))}
#[derive(Deserialize)]#[serde(rename_all="camelCase")]struct Save{client_id:String,client_secret:Option<String>,enabled:bool}
async fn save(State(s):State<AppState>,Authenticated(u):Authenticated,Json(input):Json<Save>)->ApiResult<Json<Value>>{
 let p=admin(&s,u.id.into_inner()).await?;let c=crypto(&s)?;
 let id=input.client_id.trim();if !id.is_empty()&&Uuid::parse_str(id).is_err(){return Err(ApiError::validation("Client ID 应为微软应用的 UUID"));}
 if input.client_secret.as_ref().is_some_and(|v|v.len()>4096||v.chars().any(char::is_control)){return Err(ApiError::validation("Client Secret 格式或长度不正确"));}
 if !input.enabled&&control(None).await.ok().is_some_and(|v|v["enabled"]==true){return Err(ApiError::validation("请先在下方关闭认证邮件接线，再关闭发信"));}
 let old=sqlx::query("select client_id,refresh_cipher,needs_reauth from public.scribe_outlook_mail where singleton=true").fetch_one(&p).await.map_err(db_err)?;
 let changed=old.get::<String,_>("client_id")!=id;
 if input.enabled&&(changed||old.get::<Option<String>,_>("refresh_cipher").is_none()||old.get::<bool,_>("needs_reauth")){return Err(ApiError::validation("请先连接并授权 Outlook 邮箱，再允许发信"));}
 let secret=input.client_secret.filter(|v|!v.trim().is_empty()).map(|v|enc(&c,v.trim())).transpose()?;
 sqlx::query("update public.scribe_outlook_mail set client_id=$1,client_secret_cipher=coalesce($2,case when $4 then null else client_secret_cipher end),enabled=$3,refresh_cipher=case when $4 then null else refresh_cipher end,access_cipher=case when $4 then null else access_cipher end,mailbox=case when $4 then null else mailbox end,expires_at=case when $4 then null else expires_at end,needs_reauth=case when $4 then false else needs_reauth end,last_error=null,version=version+1,updated_at=now() where singleton=true")
 .bind(id).bind(secret).bind(input.enabled).bind(changed).execute(&p).await.map_err(db_err)?;
 sqlx::query("delete from public.scribe_outlook_oauth_states where user_id=$1 or expires_at<now()").bind(u.id.into_inner()).execute(&p).await.map_err(db_err)?;
 Ok(Json(public(&p).await?))
}
async fn connect(State(s):State<AppState>,Authenticated(u):Authenticated)->ApiResult<Response>{
 let p=admin(&s,u.id.into_inner()).await?;let c=crypto(&s)?;let callback=outlook_callback_uri(&public_site()?)?;
 let r=sqlx::query("select client_id,client_secret_cipher,version from public.scribe_outlook_mail where singleton=true").fetch_one(&p).await.map_err(db_err)?;
 let id:String=r.get("client_id");if id.is_empty()||r.get::<Option<String>,_>("client_secret_cipher").is_none(){return Err(ApiError::validation("先填写并保存 Entra Web 应用的 Client ID 和 Client Secret"));}
 let state=random();let cookie=random();let verifier=random();let nonce=random();let challenge=URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
 sqlx::query("delete from public.scribe_outlook_oauth_states where user_id=$1 or expires_at<now()").bind(u.id.into_inner()).execute(&p).await.map_err(db_err)?;
 sqlx::query("insert into public.scribe_outlook_oauth_states(state_hash,cookie_hash,user_id,verifier_cipher,nonce,config_version,expires_at) values($1,$2,$3,$4,$5,$6,now()+interval '10 minutes')")
 .bind(digest(state.as_bytes())).bind(digest(cookie.as_bytes())).bind(u.id.into_inner()).bind(enc(&c,&verifier)?).bind(nonce.clone()).bind(r.get::<i64,_>("version")).execute(&p).await.map_err(db_err)?;
 let mut url=url::Url::parse("https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize").unwrap();url.query_pairs_mut().extend_pairs([("client_id",id.as_str()),("response_type","code"),("redirect_uri",callback.as_str()),("response_mode","query"),("scope",SCOPES),("state",state.as_str()),("nonce",nonce.as_str()),("code_challenge",challenge.as_str()),("code_challenge_method","S256"),("prompt","select_account")]);
 let mut response=Json(json!({"authorizationUrl":url.to_string()})).into_response();response.headers_mut().insert("set-cookie",HeaderValue::from_str(&format!("scribe_outlook_oauth={cookie}; Path=/api/mail/outlook/callback; HttpOnly; Secure; SameSite=Lax; Max-Age=600")).map_err(|_|ApiError::internal("授权状态创建失败"))?);Ok(response)
}
#[derive(Deserialize)]struct Callback{state:Option<String>,code:Option<String>,error:Option<String>}
async fn callback(State(s):State<AppState>,headers:HeaderMap,Query(q):Query<Callback>)->Response{
 let site=match public_site(){Ok(site)=>site,Err(error)=>return error.into_response()};
 let result=finish_callback(&s,&headers,q).await;
 let suffix=if result.is_ok(){"connected"}else{"failed"};
 let mut response=Redirect::to(&format!("{site}/settings?tab=admin&outlook={suffix}")).into_response();
 response.headers_mut().insert("set-cookie",HeaderValue::from_static("scribe_outlook_oauth=; Path=/api/mail/outlook/callback; HttpOnly; Secure; SameSite=Lax; Max-Age=0"));response.headers_mut().insert("cache-control",HeaderValue::from_static("no-store"));response.headers_mut().insert("referrer-policy",HeaderValue::from_static("no-referrer"));response
}
async fn finish_callback(s:&AppState,h:&HeaderMap,q:Callback)->ApiResult<()>{
 let callback=outlook_callback_uri(&public_site()?)?;
 let state=q.state.filter(|s|s.len()<=128).ok_or_else(||ApiError::validation("授权状态无效"))?;
 let cookie=h.get("cookie").and_then(|v|v.to_str().ok()).and_then(|v|v.split(';').map(str::trim).find_map(|v|v.strip_prefix("scribe_outlook_oauth="))).ok_or_else(||ApiError::forbidden("授权浏览器状态已失效"))?;
 let p=pool(s)?;let c=crypto(s)?;
 let row=sqlx::query("delete from public.scribe_outlook_oauth_states where state_hash=$1 and cookie_hash=$2 and expires_at>now() returning user_id,verifier_cipher,nonce,config_version")
 .bind(digest(state.as_bytes())).bind(digest(cookie.as_bytes())).fetch_optional(&p).await.map_err(db_err)?.ok_or_else(||ApiError::forbidden("授权已过期或已使用"))?;
 admin(s,row.get::<Uuid,_>("user_id")).await?;
 if q.error.is_some(){return Err(ApiError::validation("微软授权已取消或被拒绝"));}
 let code=q.code.filter(|v|v.len()<=10000).ok_or_else(||ApiError::validation("授权码缺失"))?;
 let cfg=sqlx::query("select client_id,client_secret_cipher,version from public.scribe_outlook_mail where singleton=true").fetch_one(&p).await.map_err(db_err)?;
 if cfg.get::<i64,_>("version")!=row.get::<i64,_>("config_version"){return Err(ApiError::validation("配置已改变，请重新授权"));}
 let client_id:String=cfg.get("client_id");let secret=dec(&c,&cfg.get::<Option<String>,_>("client_secret_cipher").ok_or_else(||ApiError::validation("缺少应用凭据"))?)?;
 let verifier=dec(&c,&row.get::<String,_>("verifier_cipher"))?;
 let form=url::form_urlencoded::Serializer::new(String::new()).extend_pairs([("client_id",client_id.as_str()),("client_secret",secret.as_str()),("grant_type","authorization_code"),("code",code.as_str()),("redirect_uri",callback.as_str()),("code_verifier",verifier.as_str()),("scope",SCOPES)]).finish();
 let response=http()?.post(TOKEN_URL).header("Content-Type","application/x-www-form-urlencoded").body(form).send().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"微软授权网络请求失败"))?;
 if !response.status().is_success(){return Err(ApiError::validation("微软令牌交换失败，请检查应用类型、Secret 与回调地址"));}
 let value:Value=response.json().await.map_err(|_|ApiError::validation("微软返回格式无效"))?;require_mail_scope(&value)?;
 // Verify Microsoft RSA signature plus nonce/audience/expiry/consumer issuer; never use this as a Scribe login token.
 let mailbox=id_token_mailbox(value["id_token"].as_str().unwrap_or(""),&client_id,&row.get::<String,_>("nonce")).await?;
 let access=value["access_token"].as_str().ok_or_else(||ApiError::validation("微软未返回 access token"))?;
 let refresh=value["refresh_token"].as_str().ok_or_else(||ApiError::validation("微软未授予离线发信授权，请重试"))?;
 let expires=Utc::now().timestamp()+value["expires_in"].as_i64().unwrap_or(3600).clamp(60,86400);
 let saved=sqlx::query("update public.scribe_outlook_mail set mailbox=$1,access_cipher=$2,refresh_cipher=$3,expires_at=$4,enabled=false,needs_reauth=false,last_error=null,updated_at=now() where singleton=true and version=$5")
 .bind(mailbox).bind(enc(&c,access)?).bind(enc(&c,refresh)?).bind(expires).bind(cfg.get::<i64,_>("version")).execute(&p).await.map_err(db_err)?;
 if saved.rows_affected()!=1{return Err(ApiError::validation("配置已改变，请重新授权"));}
 Ok(())
}
async fn id_token_mailbox(token:&str,client:&str,nonce:&str)->ApiResult<String>{
 use jsonwebtoken::{decode,decode_header,Algorithm,DecodingKey,Validation};
 let invalid=||ApiError::validation("微软个人账号身份校验失败");
 let header=decode_header(token).map_err(|_|invalid())?;
 if header.alg!=Algorithm::RS256{return Err(invalid());}
 let kid=header.kid.ok_or_else(invalid)?;
 let response=http()?.get("https://login.microsoftonline.com/consumers/discovery/v2.0/keys").send().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"微软身份签名验证暂不可用"))?;
 if !response.status().is_success(){return Err(invalid());}
 let jwks:Value=response.json().await.map_err(|_|invalid())?;
 let key=jwks["keys"].as_array().and_then(|keys|keys.iter().find(|key|key["kid"].as_str()==Some(kid.as_str())&&key["kty"]=="RSA")).ok_or_else(invalid)?;
 let public=DecodingKey::from_rsa_components(key["n"].as_str().ok_or_else(invalid)?,key["e"].as_str().ok_or_else(invalid)?).map_err(|_|invalid())?;
 let consumer="9188040d-6c67-4c5b-b112-36a304b66dad";
 let issuer=format!("https://login.microsoftonline.com/{consumer}/v2.0");
 let mut validation=Validation::new(Algorithm::RS256);validation.set_audience(&[client]);validation.set_issuer(&[issuer.as_str()]);validation.leeway=30;
 let payload=decode::<Value>(token,&public,&validation).map_err(|_|invalid())?.claims;
 if payload["nonce"]!=nonce||payload["tid"]!=consumer{return Err(invalid());}
 let email=payload["email"].as_str().or_else(||payload["preferred_username"].as_str()).filter(|v|valid_email(v)).ok_or_else(||ApiError::validation("微软未返回可用的邮箱地址"))?;Ok(email.to_string())
}
async fn disconnect(State(s):State<AppState>,Authenticated(u):Authenticated)->ApiResult<Json<Value>>{let p=admin(&s,u.id.into_inner()).await?;if control(None).await.ok().is_some_and(|v|v["enabled"]==true){return Err(ApiError::validation("请先关闭认证邮件接线，再断开 Outlook"));}
 sqlx::query("update public.scribe_outlook_mail set enabled=false,mailbox=null,access_cipher=null,refresh_cipher=null,expires_at=null,needs_reauth=false,last_error=null,version=version+1,updated_at=now() where singleton=true").execute(&p).await.map_err(db_err)?;
 sqlx::query("delete from public.scribe_outlook_oauth_states").execute(&p).await.map_err(db_err)?;Ok(Json(public(&p).await?))}
async fn access(p:&PgPool,c:&CryptoBox)->ApiResult<String>{
 let mut tx=p.begin().await.map_err(db_err)?;let row=sqlx::query("select * from public.scribe_outlook_mail where singleton=true for update").fetch_one(&mut *tx).await.map_err(db_err)?;
 if row.get::<bool,_>("needs_reauth"){return Err(ApiError::validation("Outlook 授权已失效，请重新连接"));}
 if row.get::<Option<i64>,_>("expires_at").unwrap_or(0)>Utc::now().timestamp()+90{if let Some(a)=row.get::<Option<String>,_>("access_cipher"){return dec(c,&a);}}
 let refresh=dec(c,&row.get::<Option<String>,_>("refresh_cipher").ok_or_else(||ApiError::validation("尚未连接 Outlook"))?)?;
 let secret=dec(c,&row.get::<Option<String>,_>("client_secret_cipher").ok_or_else(||ApiError::validation("缺少应用 Secret"))?)?;
 let id:String=row.get("client_id");let body=url::form_urlencoded::Serializer::new(String::new()).extend_pairs([("client_id",id.as_str()),("client_secret",secret.as_str()),("grant_type","refresh_token"),("refresh_token",refresh.as_str()),("scope",SCOPES)]).finish();
 let response=http()?.post(TOKEN_URL).header("Content-Type","application/x-www-form-urlencoded").body(body).send().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"微软令牌刷新暂时失败"))?;
 let status=response.status().as_u16();
 let v:Value=response.json().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"微软刷新响应暂不可用"))?;
 if !(200..300).contains(&status){
  if refresh_needs_reauth(status,v["error"].as_str().unwrap_or("")){
   sqlx::query("update public.scribe_outlook_mail set needs_reauth=true,enabled=false,last_error='Outlook 授权需要重新连接' where singleton=true").execute(&mut *tx).await.map_err(db_err)?;tx.commit().await.map_err(db_err)?;
   return Err(ApiError::validation("Outlook 授权需要重新连接"));
  }
  return Err(ApiError::new(ErrorCode::ServiceUnavailable,"微软令牌刷新暂时失败，授权已保留，请稍后重试"));
 }
 if require_mail_scope(&v).is_err(){
  sqlx::query("update public.scribe_outlook_mail set needs_reauth=true,enabled=false,last_error='Outlook 缺少 Mail.Send 权限，需要重新授权' where singleton=true").execute(&mut *tx).await.map_err(db_err)?;tx.commit().await.map_err(db_err)?;
  return Err(ApiError::validation("Outlook 缺少 Mail.Send 权限，请重新授权"));
 }
 let a=v["access_token"].as_str().filter(|v|!v.is_empty()).ok_or_else(||ApiError::validation("微软未返回令牌"))?;
 sqlx::query("update public.scribe_outlook_mail set access_cipher=$1,refresh_cipher=coalesce($2,refresh_cipher),expires_at=$3,last_error=null,updated_at=now() where singleton=true")
 .bind(enc(c,a)?).bind(v["refresh_token"].as_str().map(|v|enc(c,v)).transpose()?).bind(Utc::now().timestamp()+v["expires_in"].as_i64().unwrap_or(3600).clamp(60,86400)).execute(&mut *tx).await.map_err(db_err)?;tx.commit().await.map_err(db_err)?;Ok(a.to_string())
}
fn require_mail_scope(value:&Value)->ApiResult<()>{
 let present=value["scope"].as_str().is_some_and(|scope|scope.split_whitespace().any(|part|part.eq_ignore_ascii_case("Mail.Send")||part.eq_ignore_ascii_case("https://graph.microsoft.com/Mail.Send")));
 if !present{return Err(ApiError::validation("微软未授予 Mail.Send 权限，请重新连接并同意发信授权"));}Ok(())
}
fn refresh_needs_reauth(status:u16,error:&str)->bool{matches!(status,400|401)&&matches!(error,"invalid_grant"|"invalid_client"|"unauthorized_client"|"interaction_required"|"consent_required"|"invalid_scope")}
fn valid_email(s:&str)->bool{s.matches('@').count()==1&&s.len()<=320&&!s.chars().any(|c|c.is_whitespace()||c.is_control()||"<>;,".contains(c))&&s.split_once('@').is_some_and(|(a,b)|!a.is_empty()&&b.contains('.'))}
async fn send(p:&PgPool,c:&CryptoBox,to:&str,subject:&str,text:&str)->ApiResult<()>{
 let token=access(p,c).await?;let response=http()?.post("https://graph.microsoft.com/v1.0/me/sendMail").bearer_auth(token).json(&json!({"message":{"subject":subject,"body":{"contentType":"Text","content":text},"toRecipients":[{"emailAddress":{"address":to}}]},"saveToSentItems":true})).send().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"微软发信请求未能确认，请查看已发送邮件后再重试"))?;
 if response.status().as_u16()!=202{return Err(ApiError::new(ErrorCode::ServiceUnavailable,format!("微软未接受发信请求（{}），请检查授权与邮箱状态",response.status().as_u16())));}Ok(())
}
#[derive(Deserialize)]struct Test{recipient:String}
async fn test_mail(State(s):State<AppState>,Authenticated(u):Authenticated,Json(body):Json<Test>)->ApiResult<Json<Value>>{let p=admin(&s,u.id.into_inner()).await?;if !valid_email(&body.recipient){return Err(ApiError::validation("请填写一个正确的测试收件地址"));}
 let site=public_site()?;let c=crypto(&s)?;send(&p,&c,&body.recipient,"Prims Outlook 发信测试",&format!("这是管理员手动发出的 Prims 测试邮件。微软已接受请求不等于最终投递成功，请检查收件箱和垃圾邮件。\n\n{site}")).await?;Ok(Json(json!({"ok":true,"message":"微软已接受测试邮件，请检查收件箱和垃圾邮件。"})))}
fn hmac256(key:&[u8],body:&[u8])->Vec<u8>{let mut k=[0u8;64];if key.len()>64{k[..32].copy_from_slice(&Sha256::digest(key));}else{k[..key.len()].copy_from_slice(key);}let mut inner=Vec::with_capacity(64+body.len());inner.extend(k.iter().map(|v|v^0x36));inner.extend_from_slice(body);let first=Sha256::digest(&inner);let mut outer=Vec::with_capacity(96);outer.extend(k.iter().map(|v|v^0x5c));outer.extend_from_slice(&first);Sha256::digest(&outer).to_vec()}
fn constant_eq(a:&[u8],b:&[u8])->bool{if a.len()!=b.len(){return false;}a.iter().zip(b).fold(0u8,|v,(a,b)|v|(a^b))==0}
fn verify_hook(h:&HeaderMap,body:&[u8],secret:&str,now:i64)->ApiResult<String>{
 let get=|name:&str|h.get(name).and_then(|v|v.to_str().ok()).ok_or_else(||ApiError::forbidden("邮件 Hook 签名缺失"));
 let id=get("webhook-id")?;if id.len()>200{return Err(ApiError::forbidden("邮件 Hook 标识无效"));}
 let ts=get("webhook-timestamp")?;let time=ts.parse::<i64>().map_err(|_|ApiError::forbidden("邮件 Hook 时间无效"))?;if now.abs_diff(time)>300{return Err(ApiError::forbidden("邮件 Hook 已过期"));}
 let key=STANDARD.decode(secret.trim().strip_prefix("v1,whsec_").or_else(||secret.trim().strip_prefix("whsec_")).unwrap_or(secret.trim())).map_err(|_|ApiError::internal("邮件 Hook 密钥未正确配置"))?;if key.len()<32{return Err(ApiError::internal("邮件 Hook 密钥长度不足"));}
 let mut message=format!("{id}.{ts}.").into_bytes();message.extend_from_slice(body);let expected=hmac256(&key,&message);
 let ok=get("webhook-signature")?.split_whitespace().filter_map(|s|s.strip_prefix("v1,")).filter_map(|s|STANDARD.decode(s).ok()).any(|v|constant_eq(&v,&expected));if !ok{return Err(ApiError::forbidden("邮件 Hook 签名无效"));}Ok(id.to_string())
}
fn email_messages(v:&Value)->ApiResult<Vec<(String,String,String)>>{email_messages_at(v,&public_site()?)}
fn email_messages_at(v:&Value,site:&str)->ApiResult<Vec<(String,String,String)>>{
 let user=&v["user"];let e=&v["email_data"];let action=e["email_action_type"].as_str().unwrap_or("");
 let recipient=user["email"].as_str().filter(|v|valid_email(v)).ok_or_else(||ApiError::validation("邮件收件人无效"))?;
 let pairs=if action=="email_change"{let new=user["new_email"].as_str().filter(|v|valid_email(v)).ok_or_else(||ApiError::validation("新邮件地址无效"))?;if !e["token_hash_new"].as_str().unwrap_or("").is_empty(){vec![(recipient,e["token_hash_new"].as_str().unwrap_or("")),(new,e["token_hash"].as_str().unwrap_or(""))]}else{vec![(new,e["token_hash"].as_str().unwrap_or(""))]}}else{vec![(recipient,e["token_hash"].as_str().unwrap_or(""))]};
 let (subject,title)=match action{"signup"=>("验证你的 Prims 邮箱","请确认你的邮箱"),"recovery"=>("重置 Prims 密码","请设置新密码"),"magiclink"=>("登录 Prims","你的登录链接"),"invite"=>("Prims 账号邀请","接受账号邀请"),"email_change"=>("确认 Prims 邮箱变更","请确认邮箱变更"),_=>return Err(ApiError::validation("暂不支持此类认证邮件"))};
 let redirect=if action=="recovery"{format!("{site}/auth/callback?next=/auth/reset-password")}else{format!("{site}/auth/callback")};
 pairs.into_iter().map(|(to,hash)|{if hash.is_empty()||hash.len()>1024{return Err(ApiError::validation("邮件验证信息缺失"));}let mut url=url::Url::parse(&format!("{site}/supabase/auth/v1/verify")).unwrap();url.query_pairs_mut().extend_pairs([("token",hash),("type",action),("redirect_to",redirect.as_str())]);Ok((to.to_string(),subject.to_string(),format!("{title}\n\n请在浏览器打开以下链接：\n{}\n\n如果不是你发起的操作，请忽略这封邮件。不要把此链接转发给他人。",url)))}).collect()
}
async fn hook(State(s):State<AppState>,headers:HeaderMap,body:Bytes)->ApiResult<Json<Value>>{
 if body.len()>65536{return Err(ApiError::validation("邮件 Hook 请求过大"));}
 let secret=std::env::var("SCRIBE_OUTLOOK_HOOK_SECRET").map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"邮件发信通道尚未配置"))?;
 let id=verify_hook(&headers,&body,&secret,Utc::now().timestamp())?;let p=pool(&s)?;
 let enabled:bool=sqlx::query_scalar("select enabled and not needs_reauth from public.scribe_outlook_mail where singleton=true").fetch_one(&p).await.map_err(db_err)?;if !enabled{return Err(ApiError::new(ErrorCode::ServiceUnavailable,"Outlook 发信尚未启用"));}
 let value:Value=serde_json::from_slice(&body).map_err(|_|ApiError::validation("邮件 Hook 格式无效"))?;let messages=email_messages(&value)?;let hash=digest(&body);
 let inserted=sqlx::query("insert into public.scribe_outlook_mail_events(id,body_hash,status) values($1,$2,'processing') on conflict do nothing").bind(&id).bind(&hash).execute(&p).await.map_err(db_err)?.rows_affected();
 if inserted==0{let r=sqlx::query("select body_hash,status from public.scribe_outlook_mail_events where id=$1").bind(&id).fetch_one(&p).await.map_err(db_err)?;if r.get::<String,_>("body_hash")!=hash{return Err(ApiError::forbidden("邮件 Hook 标识冲突"));}if r.get::<String,_>("status")=="sent"{return Ok(Json(json!({})));}return Err(ApiError::new(ErrorCode::ServiceUnavailable,"此邮件请求已处理中或结果未确认，请管理员检查已发送邮件"));}
 let c=crypto(&s)?;for (to,subject,text) in messages{if let Err(err)=send(&p,&c,&to,&subject,&text).await{sqlx::query("update public.scribe_outlook_mail_events set status='failed',updated_at=now() where id=$1").bind(&id).execute(&p).await.map_err(db_err)?;return Err(err);}}
 sqlx::query("update public.scribe_outlook_mail_events set status='sent',updated_at=now() where id=$1").bind(id).execute(&p).await.map_err(db_err)?;Ok(Json(json!({})))
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn public_site_is_operator_controlled_https_origin(){assert_eq!(parse_public_site("https://prims.example.com:8443/").unwrap(),"https://prims.example.com:8443");for bad in ["http://prims.example.com","https://user:password@prims.example.com","https://prims.example.com/path","https://prims.example.com/?x=y","https://prims.example.com/#fragment","//prims.example.com","https://"]{assert!(parse_public_site(bad).is_err());}assert!(outlook_callback_uri("https://xn--fiqs8s.example.com").is_err());assert_eq!(outlook_callback_uri("https://prims.example.com").unwrap(),"https://prims.example.com/api/mail/outlook/callback");}
 #[test]fn refresh_transient_errors_keep_authorization(){assert!(!refresh_needs_reauth(429,"invalid_grant"));assert!(!refresh_needs_reauth(503,"invalid_grant"));assert!(!refresh_needs_reauth(400,"temporarily_unavailable"));assert!(refresh_needs_reauth(400,"invalid_grant"));assert!(refresh_needs_reauth(401,"invalid_client"));}
 #[test]fn mail_send_scope_is_required(){assert!(require_mail_scope(&json!({"scope":"openid Mail.Send"})).is_ok());assert!(require_mail_scope(&json!({"scope":"https://graph.microsoft.com/Mail.Send offline_access"})).is_ok());assert!(require_mail_scope(&json!({"scope":"openid email"})).is_err());assert!(require_mail_scope(&json!({})).is_err());}
 #[test]fn hmac_matches_rfc4231(){assert_eq!(hmac256(&[0x0b;20],b"Hi There").iter().map(|b|format!("{b:02x}")).collect::<String>(),"b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");}
 #[test]fn webhook_checks_signature_and_age(){let body=b"{}";let key=[3u8;32];let mut h=HeaderMap::new();h.insert("webhook-id",HeaderValue::from_static("evt1"));h.insert("webhook-timestamp",HeaderValue::from_static("1000"));let sig=STANDARD.encode(hmac256(&key,b"evt1.1000.{}"));h.insert("webhook-signature",HeaderValue::from_str(&format!("v1,{sig}")).unwrap());assert!(verify_hook(&h,body,&STANDARD.encode(key),1001).is_ok());assert!(verify_hook(&h,b"bad",&STANDARD.encode(key),1001).is_err());assert!(verify_hook(&h,body,&STANDARD.encode(key),1400).is_err());}
 #[test]fn recovery_uses_own_site_and_email_change_maps_hashes(){let v=json!({"user":{"email":"old@example.test","new_email":"new@example.test"},"email_data":{"email_action_type":"email_change","token_hash":"NEW_HASH","token_hash_new":"OLD_HASH","redirect_to":"https://evil.invalid"}});let m=email_messages_at(&v,"https://prims.example.com").unwrap();assert_eq!(m[0].0,"old@example.test");assert!(m[0].2.contains("token=OLD_HASH"));assert_eq!(m[1].0,"new@example.test");assert!(m[1].2.contains("token=NEW_HASH"));assert!(!m[1].2.contains("evil.invalid"));let mut r=v;r["email_data"]["email_action_type"]=json!("recovery");assert!(email_messages_at(&r,"https://prims.example.com").unwrap()[0].2.contains("reset-password"));}
}
