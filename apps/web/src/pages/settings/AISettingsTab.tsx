import {AIUsagePanel} from './AIUsagePanel';
import { VersionSummarySettings } from './VersionSummarySettings';
import { ModelProfilesSettings } from './ModelProfilesSettings';
/** One editor for all named connections, including the migrated former default. */
export function AISettingsTab({onSaved,onboarding=false}:{onSaved?:()=>void;onboarding?:boolean}={}){
 return <div className="space-y-4">{!onboarding&&<div><h2 className="text-lg font-medium">模型配置</h2><p className="mt-1 text-sm text-muted-foreground">管理接口连接、模型列表和默认模型。</p></div>}<ModelProfilesSettings {...(onSaved?{onSaved}:{})}/>{!onboarding&&<><VersionSummarySettings/><AIUsagePanel/></>}</div>;
}
