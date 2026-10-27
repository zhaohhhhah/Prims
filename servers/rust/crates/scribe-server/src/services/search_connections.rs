//! Per-user search credentials. They never enter model messages or tool arguments.
use std::{net::{IpAddr,Ipv4Addr,SocketAddr},sync::Arc,time::Duration};
use scribe_ai::{CryptoBox,CompleteRequest};
use scribe_shared::{ApiError,ApiResult,UserId};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sqlx::{PgPool,Row};
use sha2::{Digest,Sha256};

#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="lowercase")]
pub enum SearchProvider{Doubao,Deepseek}
impl SearchProvider{
 pub fn endpoint(self)->&'static str{match self{Self::Doubao=>"https://open.feedcoopapi.com/search_api/web_search",Self::Deepseek=>"https://api.deepseek.com/v1/tools/search"}}
 pub fn name(self)->&'static str{match self{Self::Doubao=>"doubao",Self::Deepseek=>"deepseek"}}
}
/// Deliberately not Debug or Serialize: clear credentials are server-local only.
#[derive(Clone)]pub struct SearchConnection{pub provider:SearchProvider,pub endpoint:String,pub api_key:String,pub revision:i64}
impl SearchConnection{
 pub fn cache_identity(&self)->String{let mut hash=Sha256::new();hash.update(self.provider.name());hash.update([0]);hash.update(&self.endpoint);hash.update([0]);hash.update(self.revision.to_le_bytes());hash.update(self.api_key.as_bytes());format!("{:x}",hash.finalize())}
}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SearchConfigInput{pub provider:SearchProvider,pub endpoint:Option<String>,pub api_key:String}
#[derive(Clone)]pub struct SearchConnections{pool:PgPool,crypto:Arc<CryptoBox>}
impl SearchConnections{
 pub fn new(pool:PgPool,crypto:Arc<CryptoBox>)->Self{Self{pool,crypto}}
 pub async fn public(&self,user:UserId)->ApiResult<Option<Value>>{
  let row=sqlx::query("select provider,endpoint,api_key_cipher is not null as has_key,updated_at from public.scribe_search_connections where user_id=$1").bind(user.into_inner()).fetch_optional(&self.pool).await.map_err(db)?;
  Ok(row.map(|row|json!({"provider":row.get::<String,_>("provider"),"endpoint":row.get::<String,_>("endpoint"),"hasApiKey":row.get::<bool,_>("has_key"),"updatedAt":row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")})))
 }
 pub async fn load(&self,user:UserId)->ApiResult<Option<SearchConnection>>{
  let row=sqlx::query("select provider,endpoint,api_key_cipher,revision from public.scribe_search_connections where user_id=$1").bind(user.into_inner()).fetch_optional(&self.pool).await.map_err(db)?;
  row.map(|row|{let name:String=row.get("provider");let provider=match name.as_str(){"doubao"=>SearchProvider::Doubao,"deepseek"=>SearchProvider::Deepseek,_=>return Err(ApiError::internal("搜索配置类型无效"))};let endpoint:String=row.get("endpoint");validate_endpoint(&endpoint)?;let cipher:String=row.get("api_key_cipher");let key=self.crypto.decrypt(&cipher).map_err(|_|ApiError::internal("搜索密钥无法解密，请重新配置"))?;Ok(SearchConnection{provider,endpoint,api_key:key,revision:row.get("revision")})}).transpose()
 }
 pub async fn save(&self,user:UserId,input:SearchConfigInput)->ApiResult<()> {
  let endpoint=input.endpoint.as_deref().map(str::trim).filter(|url|!url.is_empty()).unwrap_or(input.provider.endpoint());let endpoint=validate_endpoint(endpoint)?.to_string();
  // Check/pin again at execution. This preflight validates custom DNS but sends no key or search request.
  resolve_target(input.provider,&endpoint).await?;
  if input.api_key.len()>16384||input.api_key.chars().any(char::is_control){return Err(ApiError::validation("搜索API Key格式无效"));}
  let key=input.api_key.trim();let mut tx=self.pool.begin().await.map_err(db)?;
  sqlx::query("select pg_advisory_xact_lock(hashtextextended($1,827301))").bind(user.to_string()).execute(&mut *tx).await.map_err(db)?;
  let old=sqlx::query("select provider,endpoint,api_key_cipher from public.scribe_search_connections where user_id=$1 for update").bind(user.into_inner()).fetch_optional(&mut *tx).await.map_err(db)?;
  let cipher=if key.is_empty(){let old=old.ok_or_else(||ApiError::validation("请填写独立搜索API Key"))?;let old_provider:String=old.get("provider");let old_endpoint:String=old.get("endpoint");if old_provider!=input.provider.name()||old_endpoint!=endpoint{return Err(ApiError::validation("切换搜索服务或地址时必须填写新API Key"));}old.get::<String,_>("api_key_cipher")}else{self.crypto.encrypt(key).map_err(|_|ApiError::internal("搜索密钥加密失败"))?};
  sqlx::query("insert into public.scribe_search_connections(user_id,provider,endpoint,api_key_cipher) values($1,$2,$3,$4) on conflict(user_id) do update set provider=$2,endpoint=$3,api_key_cipher=$4,revision=scribe_search_connections.revision+1,updated_at=now()")
   .bind(user.into_inner()).bind(input.provider.name()).bind(endpoint).bind(cipher).execute(&mut *tx).await.map_err(db)?;
  tx.commit().await.map_err(db)?;Ok(())
 }
 pub async fn delete(&self,user:UserId)->ApiResult<()> {sqlx::query("delete from public.scribe_search_connections where user_id=$1").bind(user.into_inner()).execute(&self.pool).await.map_err(db)?;Ok(())}
}
fn db(_:sqlx::Error)->ApiError{ApiError::internal("独立搜索配置暂不可用")}

pub fn validate_endpoint(raw:&str)->ApiResult<url::Url>{
 if raw.len()>2048||raw.chars().any(char::is_control){return Err(ApiError::validation("搜索地址格式无效"));}
 let url=url::Url::parse(raw).map_err(|_|ApiError::validation("搜索地址必须是HTTPS URL"))?;
 if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return Err(ApiError::validation("搜索地址仅允许不带凭据、查询参数或片段的HTTPS URL"));}
 let host=url.host_str().unwrap().trim_matches(['[',']']).trim_end_matches('.').to_ascii_lowercase();
 if let Ok(ip)=host.parse::<IpAddr>(){if !public_ip(ip){return Err(ApiError::validation("搜索地址不能指向私网、回环、链路本地或元数据服务"));}}
 else if !host.contains('.')||[".localhost",".local",".internal",".lan",".test",".invalid"].iter().any(|suffix|host.ends_with(suffix)){return Err(ApiError::validation("搜索地址必须使用公网主机名"));}
 Ok(url)
}
fn public_v4(ip:Ipv4Addr)->bool{let o=ip.octets();!(ip.is_private()||ip.is_loopback()||ip.is_link_local()||ip.is_unspecified()||ip.is_broadcast()||ip.is_multicast()||o[0]==0||o[0]>=240||(o[0]==100&&(64..=127).contains(&o[1]))||(o[0]==198&&(o[1]==18||o[1]==19))||o[..3]==[192,0,0]||o[..3]==[192,0,2]||o[..3]==[198,51,100]||o[..3]==[203,0,113])}
pub fn public_ip(ip:IpAddr)->bool{match ip{
 IpAddr::V4(ip)=>public_v4(ip),
 IpAddr::V6(ip)=>{
  let s=ip.segments();
  if let Some(v4)=ip.to_ipv4_mapped(){return public_v4(v4);}
  if s[0]==0x2002{return public_v4(Ipv4Addr::new((s[1]>>8) as u8,s[1] as u8,(s[2]>>8) as u8,s[2] as u8));}
  if s[..6]==[0x64,0xff9b,0,0,0,0]{return public_v4(Ipv4Addr::new((s[6]>>8) as u8,s[6] as u8,(s[7]>>8) as u8,s[7] as u8));}
  // Ordinary addresses must be global unicast, not IPv4-compatible/reserved/local space.
  (s[0]&0xe000)==0x2000
   && !(s[0]==0x2001&&s[1]==0xdb8)
   && !(s[0]==0x3fff&&(s[1]&0xf000)==0)
   && !(s[0]==0x2001&&s[1]==0) // Teredo is an IPv4 transition tunnel, not an ordinary public target.
 }
}}

pub struct SearchTarget{pub url:url::Url,pub official:bool,pub addresses:Vec<SocketAddr>}
pub async fn resolve_target(provider:SearchProvider,endpoint:&str)->ApiResult<SearchTarget>{
 let url=validate_endpoint(endpoint)?;let official=url.as_str()==provider.endpoint();
 if official{return Ok(SearchTarget{url,official:true,addresses:vec![]});}
 let host=url.host_str().unwrap().trim_matches(['[',']']);let port=url.port_or_known_default().unwrap_or(443);
 let addresses=tokio::time::timeout(Duration::from_secs(3),tokio::net::lookup_host((host,port))).await.map_err(|_|ApiError::validation("搜索地址DNS解析超时"))?.map_err(|_|ApiError::validation("搜索地址DNS解析失败"))?.collect::<Vec<_>>();
 if addresses.is_empty()||addresses.len()>32||addresses.iter().any(|address|!public_ip(address.ip())){return Err(ApiError::validation("搜索地址解析包含非公网目标，已拒绝"));}
 Ok(SearchTarget{url,official:false,addresses})
}
pub fn legacy_connection(config:&CompleteRequest)->Option<SearchConnection>{super::web_search::deepseek_search_available(config).then(||SearchConnection{provider:SearchProvider::Deepseek,endpoint:SearchProvider::Deepseek.endpoint().into(),api_key:config.api_key.clone(),revision:0})}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn fixed_provider_endpoints_and_strict_dto(){assert!(serde_json::from_value::<SearchConfigInput>(json!({"provider":"doubao","apiKey":"fake"})).is_ok());for body in [json!({"provider":"tavily","apiKey":"fake"}),json!({"provider":"doubao","apiKey":"fake","headers":{}})]{assert!(serde_json::from_value::<SearchConfigInput>(body).is_err());}for provider in [SearchProvider::Doubao,SearchProvider::Deepseek]{assert!(validate_endpoint(provider.endpoint()).is_ok());}}
 #[test]fn endpoint_blocks_ssrf_and_credentials(){for url in ["http://example.com/search","https://127.0.0.1/","https://169.254.169.254/latest/meta-data","https://[fd00:ec2::254]/","https://user:pass@example.com/","https://example.com/?key=secret","https://example.com/#token","https://nas.local/","https://[::ffff:10.0.0.1]/","https://[64:ff9b::a00:1]/"]{assert!(validate_endpoint(url).is_err(),"{url}");}}
 #[test]fn ipv6_requires_public_global_unicast_or_verified_public_ipv4_transition(){
  for text in ["::2","::10.0.0.1","fec0::1","fc00::1","fe80::1","ff02::1","2001:db8::1","3fff::1","2001::1","64:ff9b::a00:1","64:ff9b::a9fe:a9fe","2002:a00:1::1","2002:a9fe:a9fe::1","::ffff:169.254.169.254"]{assert!(!public_ip(text.parse().unwrap()),"{text}");}
  for text in ["2606:4700:4700::1111","2001:4860:4860::8888","64:ff9b::808:808","2002:808:808::1","::ffff:8.8.8.8"]{assert!(public_ip(text.parse().unwrap()),"{text}");}
 }
 #[test]fn provider_endpoint_and_key_isolate_cache(){let a=SearchConnection{provider:SearchProvider::Doubao,endpoint:SearchProvider::Doubao.endpoint().into(),api_key:"fake-A".into(),revision:1};let mut b=a.clone();b.provider=SearchProvider::Deepseek;assert_ne!(a.cache_identity(),b.cache_identity());b=a.clone();b.api_key="fake-B".into();assert_ne!(a.cache_identity(),b.cache_identity());b=a.clone();b.revision+=1;assert_ne!(a.cache_identity(),b.cache_identity());}
}
