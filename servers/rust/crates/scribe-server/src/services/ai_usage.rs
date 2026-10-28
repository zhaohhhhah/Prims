//! Private provider-reported usage, one record per upstream model invocation.
use futures::{stream::BoxStream,StreamExt};
use scribe_ai::{adapter_for,AdapterError,CompleteRequest,CompletionChunk};
use scribe_ai::adapter::TokenUsage;
use scribe_shared::{UserId,ApiError,ApiResult};
use serde_json::{json,Value};
use sqlx::{PgPool,Row};
use uuid::Uuid;
#[derive(Clone)]pub struct UsageContext{pool:PgPool,user:Uuid,model:String,provider:String,purpose:String}
impl UsageContext{
 pub fn new(pool:PgPool,user:UserId,request:&CompleteRequest,purpose:&str)->Self{Self{pool,user:user.into_inner(),model:request.model.clone(),provider:request.provider.as_str().to_string(),purpose:purpose.into()}}
 pub async fn stream(self,request:CompleteRequest)->Result<BoxStream<'static,Result<CompletionChunk,AdapterError>>,AdapterError>{
  let mut guard=UsageGuard{context:Some(self),id:Uuid::new_v4(),started:chrono::Utc::now(),usage:TokenUsage::default(),status:"interrupted"};
  let stream=match adapter_for(request.provider).stream(request).await{Ok(stream)=>stream,Err(error)=>{guard.status="failed";guard.flush().await;return Err(error);}};
  Ok(Box::pin(futures::stream::unfold((stream,guard),|(mut stream,mut guard)|async move{
   match stream.next().await{Some(chunk)=>{match &chunk{Ok(chunk)=>if let Some(usage)=&chunk.usage{guard.usage.merge(usage)},Err(_)=>guard.status="failed"};Some((chunk,(stream,guard)))},None=>{if guard.status!="failed"{guard.status="completed";}guard.flush().await;None}}
  })))
 }
}
struct UsageGuard{context:Option<UsageContext>,id:Uuid,started:chrono::DateTime<chrono::Utc>,usage:TokenUsage,status:&'static str}
impl UsageGuard{
 async fn flush(&mut self){if let Some(context)=self.context.take(){persist(context,self.id,self.started,self.usage.clone(),self.status).await;}}
}
impl Drop for UsageGuard{fn drop(&mut self){if let Some(context)=self.context.take(){let(id,started,usage,status)=(self.id,self.started,self.usage.clone(),self.status);if let Ok(handle)=tokio::runtime::Handle::try_current(){handle.spawn(persist(context,id,started,usage,status));}}}}
async fn persist(context:UsageContext,id:Uuid,started:chrono::DateTime<chrono::Utc>,usage:TokenUsage,status:&str){
 let result=sqlx::query("insert into public.scribe_ai_usage(id,user_id,started_at,provider,model,purpose,input_tokens,output_tokens,total_tokens,cached_tokens,usage_reported,usage_complete,status) values($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) on conflict(id) do nothing")
 .bind(id).bind(context.user).bind(started).bind(context.provider).bind(context.model).bind(context.purpose).bind(usage.input.unwrap_or(0) as i64).bind(usage.output.unwrap_or(0) as i64).bind(usage.total() as i64).bind(usage.cached.unwrap_or(0) as i64).bind(usage.reported()).bind(usage.complete()).bind(status).execute(&context.pool).await;
 if result.is_err(){tracing::warn!("Unable to persist private model usage; no request content or credential logged");}
}
pub async fn summary(pool:&PgPool,user:UserId,days:u32,zone:&str,resolution:&str)->ApiResult<Value>{
 if !matches!(days,1|7|30|365)||zone.len()>80{return Err(ApiError::validation("请选择当天、近7天、近30天或近一年"));}
 let (trunc,step,format)=bucket_resolution(days,resolution)?;
 let valid:bool=sqlx::query_scalar("select exists(select 1 from pg_timezone_names where name=$1)").bind(zone).fetch_one(pool).await.map_err(db)?;if !valid{return Err(ApiError::validation("时区无效"));}
 let rows=sqlx::query("with bounds as (select (now() at time zone $3)::date-($2::int-1) as first_day,(now() at time zone $3)::date+1 as next_day),buckets as (select generate_series(first_day::timestamp,next_day::timestamp-$5::text::interval,$5::text::interval) as bucket from bounds),totals as (select date_bin($5::text::interval,date_trunc($4::text,started_at at time zone $3),timestamp '2000-01-01') as bucket,count(*)::bigint as requests,coalesce(sum(input_tokens),0)::bigint as input_tokens,coalesce(sum(output_tokens),0)::bigint as output_tokens,coalesce(sum(total_tokens),0)::bigint as total_tokens,coalesce(sum(cached_tokens),0)::bigint as cached_tokens,count(*) filter(where not usage_reported)::bigint as missing,count(*) filter(where usage_reported and not usage_complete)::bigint as partial from public.scribe_ai_usage,bounds where user_id=$1 and started_at>=first_day::timestamp at time zone $3 and started_at<next_day::timestamp at time zone $3 group by 1) select to_char(b.bucket,$6::text) as label,coalesce(t.requests,0) as requests,coalesce(t.input_tokens,0) as input_tokens,coalesce(t.output_tokens,0) as output_tokens,coalesce(t.total_tokens,0) as total_tokens,coalesce(t.cached_tokens,0) as cached_tokens,coalesce(t.missing,0) as missing,coalesce(t.partial,0) as partial from buckets b left join totals t on t.bucket=b.bucket order by b.bucket")
 .bind(user.into_inner()).bind(days as i32).bind(zone).bind(trunc).bind(step).bind(format).fetch_all(pool).await.map_err(db)?;
 let mut total=[0i64;7];let buckets=rows.iter().map(|row|{let values=[row.get::<i64,_>("input_tokens"),row.get("output_tokens"),row.get("total_tokens"),row.get("cached_tokens"),row.get("requests"),row.get("missing"),row.get("partial")];for(i,v)in values.iter().enumerate(){total[i]+=v;}json!({"label":row.get::<String,_>("label"),"inputTokens":values[0],"outputTokens":values[1],"totalTokens":values[2],"cachedTokens":values[3],"requests":values[4],"missingUsageRequests":values[5],"partialUsageRequests":values[6]})}).collect::<Vec<_>>();
 Ok(json!({"days":days,"timeZone":zone,"granularity":trunc,"buckets":buckets,"totals":{"inputTokens":total[0],"outputTokens":total[1],"totalTokens":total[2],"cachedTokens":total[3],"requests":total[4],"missingUsageRequests":total[5],"partialUsageRequests":total[6]},"source":"provider_reported"}))
}
fn db(error:sqlx::Error)->ApiError{tracing::error!(?error,"private usage database error");ApiError::internal("用量统计暂时不可用")}

fn bucket_resolution(days:u32,resolution:&str)->ApiResult<(&'static str,&'static str,&'static str)>{
 match (resolution,days){
  ("heatmap",1)=>Ok(("minute","6 minutes","YYYY-MM-DD HH24:MI")),
  ("heatmap",7|30)=>Ok(("hour","1 hour","YYYY-MM-DD HH24:00")),
  ("standard",1)=>Ok(("hour","1 hour","YYYY-MM-DD HH24:00")),
  ("standard",7|30|365)|("heatmap",365)=>Ok(("day","1 day","YYYY-MM-DD")),
  _=>Err(ApiError::validation("统计粒度无效")),
 }
}
#[cfg(test)]mod granularity_tests{use super::*;#[test]fn heatmap_resolution_changes_with_range_and_old_api_stays_compatible(){assert_eq!(bucket_resolution(1,"heatmap").unwrap().1,"6 minutes");assert_eq!(bucket_resolution(7,"heatmap").unwrap().1,"1 hour");assert_eq!(bucket_resolution(30,"heatmap").unwrap().1,"1 hour");assert_eq!(bucket_resolution(1,"standard").unwrap().0,"hour");assert_eq!(bucket_resolution(7,"standard").unwrap().0,"day");assert!(bucket_resolution(7,"arbitrary").is_err());}}
