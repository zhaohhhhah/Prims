#!/usr/bin/env python3
"""Private loopback SMTP settings controller. Fixed Auth service only; no public listener."""
import argparse
import hmac
import http.server
import json
import os
import re
import secrets
import smtplib
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.request
import urllib.parse
from email.message import EmailMessage
from email.utils import formataddr, formatdate, make_msgid
from pathlib import Path

BASE = Path(os.environ.get('PRIMS_MAIL_DEPLOYMENT_DIR', '/etc/prims/mail'))
LOCK = threading.RLock()
DEFAULT = dict(enabled=False, host='', port=587, username='', senderEmail='', senderName='Prims', requireConfirmation=False, updatedAt=None)

class Invalid(ValueError): pass
class Unavailable(RuntimeError): pass

def atomic(path, data):
    fd, name = tempfile.mkstemp(prefix='.'+path.name+'-', dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, 'wb') as stream:
            stream.write(data); stream.flush(); os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)

def loopback_url(name,default,path):
    value=os.environ.get(name,default);uri=urllib.parse.urlsplit(value)
    if uri.scheme not in ('http','https') or uri.hostname not in ('127.0.0.1','::1') or uri.username or uri.password or uri.query or uri.fragment or uri.path!=path:raise Invalid('Operator configuration must use the fixed loopback endpoint path')
    return value

def public_url():
    value=os.environ.get('PRIMS_PUBLIC_URL','')
    if not value:
        config=BASE/'app.env'
        if config.exists():value=next((line.split('=',1)[1] for line in config.read_text().splitlines() if line.startswith('PRIMS_PUBLIC_URL=')), '')
    uri=urllib.parse.urlsplit(value)
    if uri.scheme!='https' or not uri.hostname or uri.username or uri.password or uri.query or uri.fragment or uri.path not in ('','/'):raise Invalid('Set PRIMS_PUBLIC_URL to the trusted HTTPS site origin')
    return value.rstrip('/')

def env():
    path=BASE/'mail.env'
    return dict(line.split('=',1) for line in path.read_text().splitlines() if '=' in line) if path.exists() else {}

def settings():
    path=BASE/'mail-settings.json'
    value={**DEFAULT, **(json.loads(path.read_text()) if path.exists() else {})}
    value['passwordSet']=bool(env().get('GOTRUE_SMTP_PASS',''))
    return value

def email(value):
    return bool(re.fullmatch(r'[^\s@<>;,]+@[^\s@<>;,]+\.[^\s@<>;,]+', value))

def normalize(data, complete=False):
    if not isinstance(data,dict): raise Invalid('配置格式不正确')
    unknown=set(data)-set(DEFAULT)-{'password','clearPassword','recipient','passwordSet'}
    if unknown: raise Invalid('存在不支持的配置字段')
    current=settings()
    result={key:data.get(key,current[key]) for key in DEFAULT}
    for key in ('enabled','requireConfirmation'):
        if type(result[key]) is not bool: raise Invalid('开关格式不正确')
    if type(result['port']) is not int or not 1<=result['port']<=65535: raise Invalid('SMTP 端口需为 1–65535')
    for key,limit in [('host',253),('username',320),('senderEmail',320),('senderName',80)]:
        value=result[key]
        if not isinstance(value,str) or len(value)>limit or any(ord(char)<32 or ord(char)==127 for char in value): raise Invalid('邮件配置包含不支持的字符或长度')
        result[key]=value.strip()
    password=data.get('password','')
    if not isinstance(password,str) or len(password)>4096 or any(ord(char)<32 or ord(char)==127 for char in password): raise Invalid('密码／授权码格式不正确')
    clear=data.get('clearPassword',False)
    if type(clear) is not bool: raise Invalid('清除密码开关格式不正确')
    if clear and password: raise Invalid('不能同时清除和设置授权码')
    if not password and not clear: password=env().get('GOTRUE_SMTP_PASS','')
    if result['requireConfirmation'] and not result['enabled']: raise Invalid('启用邮件后才能要求注册验证邮箱')
    if result['enabled'] or complete:
        if not result['host'] or not re.fullmatch(r'[A-Za-z0-9_.:-]+',result['host']): raise Invalid('请填写正确的 SMTP 主机名，不要加协议或端口')
        if not email(result['senderEmail']): raise Invalid('请填写正确的发件邮箱')
        if result['username'] and not password: raise Invalid('已填写 SMTP 账号，请填写密码／邮箱授权码')
    return result,password

def smtp(config,password):
    connection=None
    try:
        context=ssl.create_default_context()
        if config['port']==465:
            connection=smtplib.SMTP_SSL(config['host'],config['port'],timeout=8,context=context)
            connection.ehlo()
        else:
            connection=smtplib.SMTP(config['host'],config['port'],timeout=8)
            connection.ehlo()
            if not connection.has_extn('starttls'): raise Invalid('服务器不支持 STARTTLS；SSL/TLS 请使用 465 端口')
            connection.starttls(context=context);connection.ehlo()
        if config['username']: connection.login(config['username'],password)
        return connection
    except Exception:
        if connection is not None: connection.close()
        raise

def mail_env(config,password):
    values={
        'GOTRUE_SMTP_HOST':config['host'] if config['enabled'] else '',
        'GOTRUE_SMTP_PORT':str(config['port']), 'GOTRUE_SMTP_USER':config['username'],
        'GOTRUE_SMTP_PASS':password,'GOTRUE_SMTP_ADMIN_EMAIL':config['senderEmail'],
        'GOTRUE_SMTP_SENDER_NAME':config['senderName'],'GOTRUE_SMTP_LOGGING_ENABLED':'false',
        'GOTRUE_MAILER_AUTOCONFIRM':'false' if config['requireConfirmation'] else 'true',
        'GOTRUE_SMTP_MAX_FREQUENCY':'60s',
        'GOTRUE_MAILER_URLPATHS_CONFIRMATION':'/verify','GOTRUE_MAILER_URLPATHS_RECOVERY':'/verify',
        'GOTRUE_MAILER_URLPATHS_INVITE':'/verify','GOTRUE_MAILER_URLPATHS_EMAIL_CHANGE':'/verify',
    }
    return ''.join(key+'='+value+'\n' for key,value in values.items()).encode()

def apply_auth():
    command=['docker','-H',os.environ.get('PRIMS_DOCKER_HOST','unix:///var/run/docker.sock'),'compose','-f',str(BASE/'compose.yaml'),'up','-d','--no-build','--no-deps','auth']
    subprocess.run(command,cwd=BASE,check=True,capture_output=True,timeout=90)
    for _ in range(40):
        try:
            with urllib.request.urlopen(loopback_url('PRIMS_AUTH_HEALTH_URL','http://127.0.0.1:19999/health','/health'),timeout=2) as response:
                if response.status==200:return
        except (OSError,ValueError):pass
        time.sleep(.5)
    raise Unavailable('认证邮件服务未能恢复，已尝试回滚')

def save(data):
    config,password=normalize(data)
    if config['enabled']:
        connection=smtp(config,password)
        connection.quit()
    path=BASE/'mail.env';before=path.read_bytes();after=mail_env(config,password)
    if before!=after:
        atomic(path,after)
        try:apply_auth()
        except Exception:
            atomic(path,before)
            try:apply_auth()
            except Exception:pass
            raise Unavailable('邮件配置应用失败，原配置已恢复，请稍后重试') from None
    config['updatedAt']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
    atomic(BASE/'mail-settings.json',json.dumps(config,ensure_ascii=False).encode())
    return settings()

def send_test(data):
    config,password=normalize(data,complete=True)
    recipient=data.get('recipient','')
    if not isinstance(recipient,str) or not email(recipient) or len(recipient)>320 or any(ord(char)<32 for char in recipient): raise Invalid('请填写单个正确的测试收件邮箱')
    message=EmailMessage()
    message['From']=formataddr((config['senderName'],config['senderEmail']))
    message['To']=recipient;message['Subject']='Prims 邮件配置测试'
    message['Date']=formatdate(localtime=False);message['Message-ID']=make_msgid()
    message.set_content('This is an administrator-requested Prims SMTP test. Acceptance by the server does not guarantee delivery.\n\n'+public_url()+'\n')
    with smtp(config,password) as connection:connection.send_message(message)
    return {'ok':True,'message':'测试邮件已交给 SMTP 服务器，请检查收件箱和垃圾邮件；草稿配置尚未保存。'}

def safe_error(error):
    if isinstance(error,Invalid):return 400,str(error)
    if isinstance(error,smtplib.SMTPAuthenticationError):return 400,'SMTP 登录失败，请检查账号和邮箱授权码'
    if isinstance(error,ssl.SSLCertVerificationError):return 400,'SMTP TLS 证书校验失败，请检查主机名和证书'
    if isinstance(error,(TimeoutError,socket.timeout)):return 502,'SMTP 连接超时，请检查服务器地址、端口及 NAS 出站网络'
    if isinstance(error,smtplib.SMTPException):return 502,'SMTP 服务器拒绝请求，请检查发件邮箱、授权码和收件地址'
    if isinstance(error,OSError):return 502,'无法连接邮件服务器，请检查地址、端口和网络'
    if isinstance(error,Unavailable):return 503,str(error)
    return 500,'邮件配置服务暂时不可用'

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def answer(self,status,result):
        body=json.dumps(result,ensure_ascii=False).encode()
        self.send_response(status);self.send_header('Content-Type','application/json; charset=utf-8')
        self.send_header('Cache-Control','no-store');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def authorized(self):
        expected=(BASE/'mail-control-token').read_text().strip()
        if not hmac.compare_digest(self.headers.get('Authorization',''),'Bearer '+expected):
            self.answer(403,{'message':'Forbidden'});return False
        return True
    def do_GET(self):
        if not self.authorized():return
        if self.path!='/config':self.answer(404,{'message':'Not found'});return
        with LOCK:self.answer(200,settings())
    def do_POST(self):
        if not self.authorized():return
        if self.path not in ('/config','/test'):self.answer(404,{'message':'Not found'});return
        try:
            size=int(self.headers.get('Content-Length','0'))
            if not 0<size<=16384:raise Invalid('请求过大或为空')
            data=json.loads(self.rfile.read(size))
            with LOCK: result=save(data) if self.path=='/config' else send_test(data)
            self.answer(200,result)
        except (json.JSONDecodeError,ValueError) as error:
            self.answer(400,{'message':str(error) if isinstance(error,Invalid) else '请求格式不正确'})
        except Exception as error:
            status,message=safe_error(error);self.answer(status,{'message':message})

def initialize(base):
    global BASE
    BASE=Path(base);BASE.mkdir(exist_ok=True,parents=True)
    os.umask(0o077)
    if not (BASE/'mail-control-token').exists():atomic(BASE/'mail-control-token',secrets.token_hex(32).encode())
    if not (BASE/'mail.env').exists():atomic(BASE/'mail.env',mail_env(DEFAULT,''))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--base',default=str(BASE));parser.add_argument('--port',type=int,default=19101);parser.add_argument('--initialize',action='store_true')
    args=parser.parse_args();initialize(args.base)
    if not args.initialize:
        http.server.ThreadingHTTPServer(('127.0.0.1',args.port),Handler).serve_forever()
