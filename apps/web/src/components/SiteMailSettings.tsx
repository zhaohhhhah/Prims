import { useState } from 'react';
import { OutlookMailSettings } from './OutlookMailSettings';
import { SMTPMailSettings } from './SMTPMailSettings';
export function SiteMailSettings(){
 const [channel,setChannel]=useState<'smtp'|'outlook'>('smtp');
 return <div className="space-y-4"><section className="rounded-lg border p-5 space-y-2"><h2 className="text-lg font-medium">站点邮件</h2><label className="flex items-center gap-3 text-sm">配置方式<select value={channel} onChange={e=>setChannel(e.target.value as 'smtp'|'outlook')} className="rounded-md border bg-background p-2"><option value="smtp">SMTP（QQ 邮箱等）</option><option value="outlook">Microsoft Outlook OAuth（备选）</option></select></label><p className="text-xs text-muted-foreground">切换此处仅查看设置；填写后点击“保存并应用”才更换发信通道。同一时间只有一个通道启用，已有账号和个人 API 配置保留。</p></section>{channel==='smtp'?<SMTPMailSettings />:<OutlookMailSettings />}</div>;
}
