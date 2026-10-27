#!/usr/bin/env python3
"""No Docker/network/mail calls: preservation and rollback tests in disposable directories."""
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

class ControlTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.base = Path(self.tmp.name)
        shutil.copy(Path(__file__).with_name('mail-control.py'), self.base/'mail-control.py')
        shutil.copy(Path(__file__).with_name('mail-control-outlook.py'), self.base/'mail-control-outlook.py')
        spec=importlib.util.spec_from_file_location('control_test',self.base/'mail-control-outlook.py'); self.mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(self.mod)
        self.mod.legacy.BASE=self.base
        (self.base/'mail.env').write_text('UNRELATED=keep\nGOTRUE_SMTP_HOST=example.test\nGOTRUE_MAILER_AUTOCONFIRM=true\n')
        (self.base/'app.env').write_text('SCRIBE_OUTLOOK_HOOK_SECRET='+'A'*44+'\n')
    def tearDown(self): self.tmp.cleanup()
    def test_narrow_fields_and_default_confirmation(self):
        self.mod.legacy.apply_auth=lambda:None
        for data in ({'enabled':True,'requireConfirmation':False,'url':'http://evil'}, {'enabled':'true','requireConfirmation':False}):
            with self.assertRaises(self.mod.legacy.Invalid): self.mod.apply(data)
        result=self.mod.apply({'enabled':True,'requireConfirmation':False})
        self.assertTrue(result['enabled']); self.assertFalse(result['requireConfirmation'])
        self.assertIn('UNRELATED=keep\n', (self.base/'mail.env').read_text())
        self.assertEqual(self.mod.legacy.env().get('GOTRUE_SMTP_HOST'), '')
        self.assertIn(self.mod.hook_uri(),(self.base/'mail.env').read_text())
    def test_health_failure_restores_exact_bytes(self):
        before=(self.base/'mail.env').read_bytes()
        def fail(): raise RuntimeError('fake unhealthy')
        self.mod.legacy.apply_auth=fail
        with self.assertRaises(self.mod.legacy.Unavailable): self.mod.apply({'enabled':True,'requireConfirmation':False})
        self.assertEqual(before,(self.base/'mail.env').read_bytes())
    def fake_smtp(self):
        class Connected:
            def quit(self): pass
        self.mod.legacy.smtp=lambda *_:Connected()
        self.mod.legacy.apply_auth=lambda:None
    def draft(self, **extra):
        return dict(enabled=True,host='smtp.qq.com',port=465,username='owner@qq.com',password='FAKE-AUTH-CODE',senderEmail='owner@qq.com',senderName='Scribe',requireConfirmation=False,**extra)
    def test_switches_are_single_active_and_keep_both_secrets(self):
        self.fake_smtp();original_app=(self.base/'app.env').read_bytes()
        self.mod.save_smtp(self.draft())
        self.assertEqual(self.mod.active(),'smtp');self.assertEqual(self.mod.password(),'FAKE-AUTH-CODE')
        self.mod.apply({'enabled':True,'requireConfirmation':False})
        self.assertEqual(self.mod.active(),'outlook');self.assertEqual(self.mod.legacy.env()['GOTRUE_SMTP_HOST'],'')
        self.assertEqual(self.mod.password(),'FAKE-AUTH-CODE')
        draft=self.draft();draft['password']='';self.mod.save_smtp(draft)
        self.assertEqual(self.mod.active(),'smtp');self.assertEqual(self.mod.legacy.env()['GOTRUE_HOOK_SEND_EMAIL_ENABLED'],'false')
        self.assertTrue(self.mod.smtp_config()['passwordSet']);self.assertNotIn('password',self.mod.smtp_config())
        self.assertEqual(original_app,(self.base/'app.env').read_bytes())
    def test_smtp_failed_apply_restores_credentials_and_channel(self):
        self.fake_smtp();self.mod.save_smtp(self.draft());self.mod.apply({'enabled':True,'requireConfirmation':False})
        paths=['mail.env','mail-settings.json','smtp-password'];before={p:(self.base/p).read_bytes() for p in paths}
        def fail():raise RuntimeError('fake auth unhealthy')
        self.mod.legacy.apply_auth=fail;draft=self.draft();draft['password']='CHANGED-FAKE-CODE'
        with self.assertRaises(self.mod.legacy.Unavailable):self.mod.save_smtp(draft)
        self.assertEqual(self.mod.active(),'outlook')
        for p in paths:self.assertEqual(before[p],(self.base/p).read_bytes())
    def test_no_secret_return_and_disabled_defaults(self):
        self.assertEqual(self.mod.active(),'smtp') # fixture starts with an old SMTP host
        draft=self.draft();draft['enabled']=False;self.fake_smtp();self.mod.save_smtp(draft)
        self.assertEqual(self.mod.active(),'off');self.assertFalse(self.mod.smtp_config()['requireConfirmation'])
        (self.base/'mail-settings.json').unlink();self.assertEqual(self.mod.smtp_config()['senderName'],'Prims')
        (self.base/'mail-settings.json').write_text(json.dumps({'senderName':'Custom preserved'}));self.assertEqual(self.mod.smtp_config()['senderName'],'Custom preserved')
        rendered=json.dumps(self.mod.smtp_config());self.assertNotIn('FAKE-AUTH-CODE',rendered)
    def test_qq_465_implicit_tls_and_587_starttls_validate_certificates(self):
        class Connection:
            def ehlo(self):pass
            def login(self,*args):pass
            def close(self):pass
            def has_extn(self,_):return True
            def starttls(self,context):self.context=context
        secure=Connection()
        with patch.object(self.mod.legacy.smtplib,'SMTP_SSL',return_value=secure) as factory:
            self.mod.legacy.smtp(self.draft(),'FAKE')
            context=factory.call_args.kwargs['context']
            self.assertTrue(context.check_hostname);self.assertEqual(context.verify_mode,self.mod.legacy.ssl.CERT_REQUIRED)
        upgrade=Connection();draft=self.draft();draft['port']=587
        with patch.object(self.mod.legacy.smtplib,'SMTP',return_value=upgrade):
            self.mod.legacy.smtp(draft,'FAKE')
            self.assertTrue(upgrade.context.check_hostname);self.assertEqual(upgrade.context.verify_mode,self.mod.legacy.ssl.CERT_REQUIRED)
if __name__=='__main__': unittest.main()
