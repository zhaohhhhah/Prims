#!/usr/bin/env python3
"""Private fixed-operation SMTP / Outlook controller. A single active Auth mail transport."""
import argparse
import base64
import importlib.util
import json
import os
import secrets
import time
from pathlib import Path
SPEC=importlib.util.spec_from_file_location('smtp_control',str(Path(__file__).with_name('mail-control.py')))
legacy=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(legacy)
# Product default only: saved sender names are intentionally preserved.
legacy.DEFAULT['senderName']='Prims'
def hook_uri():return legacy.loopback_url('PRIMS_APP_HOOK_URL','http://127.0.0.1:13010/api/mail/outlook/hook','/api/mail/outlook/hook')
HOOK_KEYS={'GOTRUE_HOOK_SEND_EMAIL_ENABLED','GOTRUE_HOOK_SEND_EMAIL_URI','GOTRUE_HOOK_SEND_EMAIL_SECRETS'}

def active():
    current=legacy.env()
    if current.get('GOTRUE_HOOK_SEND_EMAIL_ENABLED')=='true': return 'outlook'
    return 'smtp' if current.get('GOTRUE_SMTP_HOST','') else 'off'

def config():
    current=legacy.env()
    return {'enabled':active()=='outlook','activeChannel':active(),
        'requireConfirmation':current.get('GOTRUE_MAILER_AUTOCONFIRM','true')=='false',
        'hookSecretSet':bool(current.get('GOTRUE_HOOK_SEND_EMAIL_SECRETS'))}

def password():
    path=legacy.BASE/'smtp-password'
    return path.read_text() if path.exists() else legacy.env().get('GOTRUE_SMTP_PASS','')

def smtp_config():
    result=legacy.settings()
    if not result.get('senderName'):result['senderName']='Prims'
    result.update(enabled=active()=='smtp',passwordSet=bool(password()),activeChannel=active(),
        requireConfirmation=active()=='smtp' and legacy.env().get('GOTRUE_MAILER_AUTOCONFIRM','true')=='false')
    return result

def merge_env(values):
    path=legacy.BASE/'mail.env'
    lines=[line for line in path.read_text().splitlines() if line.split('=',1)[0] not in values]
    return ('\n'.join(lines)+'\n'+''.join(k+'='+v+'\n' for k,v in values.items())).encode()

def commit(after,artifacts=None):
    """Only fixed files and fixed auth service. Restore every byte on failure."""
    path=legacy.BASE/'mail.env';updates={path:after,**(artifacts or {})}
    before={p:p.read_bytes() if p.exists() else None for p in updates}
    try:
        for p,value in updates.items(): legacy.atomic(p,value)
        if before[path]!=after: legacy.apply_auth()
    except Exception:
        for p,value in before.items():
            if value is None:p.unlink(missing_ok=True)
            else:legacy.atomic(p,value)
        if before[path]!=after:
            try:legacy.apply_auth()
            except Exception:pass
        raise legacy.Unavailable('邮件配置应用失败，原配置及两通道凭据已恢复；请检查认证服务健康状态') from None

def apply(data):
    if not isinstance(data,dict) or set(data)!={'enabled','requireConfirmation'} or any(type(v) is not bool for v in data.values()):raise legacy.Invalid('仅支持邮件 Hook 与注册验证两个开关')
    if data['requireConfirmation'] and not data['enabled']:raise legacy.Invalid('启用并验证邮件后才能要求注册验证邮箱')
    if not data['enabled'] and active()!='outlook':return config()
    app=dict(line.split('=',1) for line in (legacy.BASE/'app.env').read_text().splitlines() if '=' in line)
    secret=app.get('SCRIBE_OUTLOOK_HOOK_SECRET','')
    if data['enabled'] and not secret:raise legacy.Invalid('尚未安装邮件 Hook 的共享签名密钥')
    values={'GOTRUE_HOOK_SEND_EMAIL_ENABLED':str(data['enabled']).lower(),
        'GOTRUE_HOOK_SEND_EMAIL_URI':hook_uri() if data['enabled'] else '',
        'GOTRUE_HOOK_SEND_EMAIL_SECRETS':'v1,whsec_'+secret if data['enabled'] else '',
        'GOTRUE_SMTP_HOST':'','GOTRUE_MAILER_AUTOCONFIRM':str(not data['requireConfirmation']).lower()}
    artifacts={}
    if password():artifacts[legacy.BASE/'smtp-password']=password().encode()
    commit(merge_env(values),artifacts)
    return config()

def normalize_smtp(data,complete=False):
    if not isinstance(data,dict):raise legacy.Invalid('邮件配置格式不正确')
    copy=dict(data)
    copy.pop('activeChannel',None)
    if not copy.get('password') and not copy.get('clearPassword',False):copy['password']=password()
    result,secret=legacy.normalize(copy,complete=complete)
    if not result['senderName']:result['senderName']='Prims'
    return result,secret

def save_smtp(data):
    result,secret=normalize_smtp(data)
    if result['enabled']:
        connection=legacy.smtp(result,secret);connection.quit()
    values=dict(line.split('=',1) for line in legacy.mail_env(result,secret).decode().splitlines() if '=' in line)
    if result['enabled']:
        values.update(GOTRUE_HOOK_SEND_EMAIL_ENABLED='false',GOTRUE_HOOK_SEND_EMAIL_URI='',GOTRUE_HOOK_SEND_EMAIL_SECRETS='')
    elif active()=='outlook':
        # Saving an inactive SMTP draft must preserve the working Outlook path and confirmation policy.
        values.pop('GOTRUE_MAILER_AUTOCONFIRM',None)
    result['updatedAt']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    commit(merge_env(values),{legacy.BASE/'mail-settings.json':json.dumps(result,ensure_ascii=False).encode(),legacy.BASE/'smtp-password':secret.encode()})
    return smtp_config()

def test_smtp(data):
    result,secret=normalize_smtp(data,complete=True)
    recipient=data.get('recipient','')
    if not isinstance(recipient,str) or len(recipient)>320 or not legacy.email(recipient) or any(ord(c)<32 for c in recipient):raise legacy.Invalid('请填写单个正确的测试收件邮箱')
    message=legacy.EmailMessage()
    message['From']=legacy.formataddr((result['senderName'],result['senderEmail']))
    message['To']=recipient;message['Subject']='Prims 邮件配置测试'
    message['Date']=legacy.formatdate(localtime=False);message['Message-ID']=legacy.make_msgid()
    message.set_content('这是一封由管理员手动发送的 Prims SMTP 测试邮件。收到这封邮件，说明 SMTP 连接、登录和投递已被服务器接受。\n\n网站：'+legacy.public_url()+'\n')
    with legacy.smtp(result,secret) as connection:connection.send_message(message)
    return {'ok':True,'message':'测试邮件已交给 SMTP 服务器，请检查收件箱和垃圾邮件；草稿配置尚未保存。'}

class Handler(legacy.Handler):
    def do_GET(self):
        if self.path not in ('/outlook-config','/smtp-config','/config'):return super().do_GET()
        if not self.authorized():return
        with legacy.LOCK:self.answer(200,config() if self.path=='/outlook-config' else smtp_config())
    def do_POST(self):
        if self.path not in ('/outlook-config','/smtp-config','/smtp-test','/config','/test'):
            if not self.authorized():return
            self.answer(404,{'message':'Not found'});return
        if not self.authorized():return
        try:
            size=int(self.headers.get('Content-Length','0'))
            if not 0<size<=16384:raise legacy.Invalid('请求过大或为空')
            data=json.loads(self.rfile.read(size))
            with legacy.LOCK:
                if self.path=='/outlook-config':result=apply(data)
                elif self.path in ('/smtp-test','/test'):result=test_smtp(data)
                else:result=save_smtp(data)
            self.answer(200,result)
        except (ValueError,json.JSONDecodeError) as error:self.answer(400,{'message':str(error) if isinstance(error,legacy.Invalid) else '请求格式不正确'})
        except Exception as error:
            status,message=legacy.safe_error(error);self.answer(status,{'message':message})

def prepare(base):
    """Operator only: no Auth mutation/recreation and no secret output."""
    legacy.initialize(base);path=legacy.BASE/'app.env';data=path.read_text()
    existing=dict(line.split('=',1) for line in data.splitlines() if '=' in line);add={}
    if not existing.get('SCRIBE_OUTLOOK_HOOK_SECRET'):add['SCRIBE_OUTLOOK_HOOK_SECRET']=base64.b64encode(secrets.token_bytes(32)).decode()
    if not existing.get('SCRIBE_MAIL_CONTROL_TOKEN'):add['SCRIBE_MAIL_CONTROL_TOKEN']=(legacy.BASE/'mail-control-token').read_text().strip()
    if add:legacy.atomic(path,(data.rstrip('\n')+'\n'+''.join(k+'='+v+'\n' for k,v in add.items())).encode())
    print('Private app mail env prepared. Values hidden; no services restarted; Auth settings unchanged.')
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--base',default=str(legacy.BASE));p.add_argument('--port',type=int,default=19101);p.add_argument('--prepare',action='store_true');a=p.parse_args();os.umask(0o077)
    if a.prepare:prepare(a.base)
    else:
        legacy.initialize(a.base);legacy.http.server.ThreadingHTTPServer(('127.0.0.1',a.port),Handler).serve_forever()
