//! Admin-only SMTP settings proxy to the fixed private loopback controller.
use axum::{extract::State,middleware::from_fn,routing::{get,post},Json,Router};
use scribe_auth::{require_auth,Authenticated};
use scribe_shared::{ApiError,ApiResult,ErrorCode};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;
use crate::state::AppState;

pub fn router()->Router<AppState>{Router::new().route("/api/admin/mail/smtp",get(config).put(save)).route("/api/admin/mail/smtp/test",post(test)).layer(from_fn(require_auth))}
async fn require_admin(state:&AppState,user:Uuid)->ApiResult<()>{
 let pool=state.db().ok_or_else(||ApiError::internal("数据库未配置"))?.pool();
 let allowed:bool=sqlx::query_scalar("select exists(select 1 from public.scribe_site_admins where user_id=$1)").bind(user).fetch_one(pool).await.map_err(|_|ApiError::internal("管理权限暂时无法检查"))?;
 if !allowed{return Err(ApiError::forbidden("仅管理员可配置站点邮件"));}Ok(())
}
async fn control(path:&str,body:Option<Value>)->ApiResult<Value>{
 let token=std::env::var("SCRIBE_MAIL_CONTROL_TOKEN").ok().filter(|v|!v.is_empty()).ok_or_else(||ApiError::new(ErrorCode::ServiceUnavailable,"私有邮件控制器尚未安装"))?;
 if body.as_ref().is_some_and(|v|v.to_string().len()>16384){return Err(ApiError::validation("邮件配置请求过大"));}
 let client=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(if body.is_some(){110}else{3})).redirect(reqwest::redirect::Policy::none()).build().map_err(|_|ApiError::internal("邮件网络组件不可用"))?;
 let url=format!("http://127.0.0.1:19101{path}");
 let request=if let Some(value)=body{client.post(url).json(&value)}else{client.get(url)};
 let response=request.bearer_auth(token).send().await.map_err(|_|ApiError::new(ErrorCode::ServiceUnavailable,"私有邮件控制器连接失败"))?;
 let success=response.status().is_success();let value:Value=response.json().await.map_err(|_|ApiError::internal("邮件控制器响应无效"))?;
 if !success{return Err(ApiError::validation(value["message"].as_str().unwrap_or("邮件配置操作失败")));}
 Ok(value)
}
async fn config(State(state):State<AppState>,Authenticated(user):Authenticated)->ApiResult<Json<Value>>{require_admin(&state,user.id.into_inner()).await?;Ok(Json(control("/smtp-config",None).await?))}
async fn save(State(state):State<AppState>,Authenticated(user):Authenticated,Json(body):Json<Value>)->ApiResult<Json<Value>>{require_admin(&state,user.id.into_inner()).await?;Ok(Json(control("/smtp-config",Some(body)).await?))}
async fn test(State(state):State<AppState>,Authenticated(user):Authenticated,Json(body):Json<Value>)->ApiResult<Json<Value>>{require_admin(&state,user.id.into_inner()).await?;Ok(Json(control("/smtp-test",Some(body)).await?))}
