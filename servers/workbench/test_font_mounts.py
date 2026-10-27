"""Pure optional-font mount contract checks. Starts no Docker jobs or services."""
import unittest
from unittest.mock import patch
import uuid
import broker
from worker import validate_job

class OptionalFontMounts(unittest.TestCase):
    def arguments(self, font_dir='', cache=''):
        with patch.object(broker,'FONT_DIR',font_dir),patch.object(broker,'FONT_CACHE',cache),patch.object(broker,'CACHE','/operator/public-tex-cache'),patch.object(broker,'IMAGE','sha256:'+'a'*64):
            return broker.container_options('fixture-only')
    def test_empty_pair_keeps_previous_cache_and_fixed_security(self):
        args=self.arguments()
        mounts=[args[i+1] for i,value in enumerate(args) if value=='--mount']
        self.assertEqual(mounts,['type=bind,source=/operator/public-tex-cache,target=/cache,readonly,bind-propagation=rprivate'])
        for flag in ('--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--network','none','--user','10001:10001','--memory','512m','--cpus','1','--pids-limit','64'):
            self.assertIn(flag,args)
    def test_both_public_font_resources_are_fixed_readonly_mounts(self):
        args=self.arguments('/operator/public-fonts','/operator/font-cache')
        mounts=[args[i+1] for i,value in enumerate(args) if value=='--mount']
        self.assertEqual(len(mounts),3)
        self.assertIn('source=/operator/public-fonts,target=/usr/local/share/fonts/prims-import,readonly,bind-propagation=rprivate',mounts[1])
        self.assertIn('source=/operator/font-cache,target=/var/cache/fontconfig,readonly,bind-propagation=rprivate',mounts[2])
        self.assertTrue(all(',readonly,' in value for value in mounts))
        self.assertNotIn('docker.sock',' '.join(args))
        self.assertNotIn(broker.DOCKER_HOST,' '.join(args))
    def test_half_configured_pair_fails_closed(self):
        for values in (('/operator/fonts',''),('','/operator/cache')):
            with self.subTest(values=values),self.assertRaises(ValueError):self.arguments(*values)
    def test_relative_comma_and_control_paths_are_rejected(self):
        for value in ('relative/fonts','/fonts,readonly=false','/fonts\nextra','/fonts\x00','/fonts\x7f'):
            for values in ((value,'/operator/cache'),('/operator/fonts',value)):
                with self.subTest(values=values),self.assertRaises(ValueError):self.arguments(*values)
    def test_job_fields_cannot_supply_any_font_mount_or_socket(self):
        for key in ('fontDir','fontCache','mounts','PRIMS_WORKBENCH_FONT_DIR','DOCKER_HOST'):
            value={'jobId':str(uuid.uuid4()),'task':'list_fonts','files':[],key:'/private/resource'}
            with self.subTest(key=key),self.assertRaises(ValueError):validate_job(value)
    def test_no_optional_resource_changes_worker_image_or_adds_privileges(self):
        original=self.arguments();extended=self.arguments('/operator/fonts','/operator/cache')
        self.assertEqual(original[-1],extended[-1])
        self.assertNotIn('--privileged',extended)
        self.assertFalse(any('seccomp=unconfined' in item or 'userns=host' in item for item in extended))
        self.assertEqual([original[i+1] for i,v in enumerate(original) if v=='--tmpfs'],[extended[i+1] for i,v in enumerate(extended) if v=='--tmpfs'])
if __name__=='__main__':unittest.main()
