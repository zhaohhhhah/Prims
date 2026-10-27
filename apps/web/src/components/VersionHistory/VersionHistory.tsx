import { type ProjectId, type ProjectVersion, type VersionId } from '@scribe/shared';
import { Button, Skeleton } from '@scribe/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { diff_match_patch as DiffMatchPatch } from 'diff-match-patch';
import { CameraIcon, History, Loader2, RotateCcw, X } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';

import { api, type ApiError } from '../../lib/api';
import { projectActionsApi, type RestorePlan } from '../../lib/project-actions';
import { VersionSummaryRow } from './VersionSummaryRow';
import {useVersionSummaries} from '../../lib/version-summaries';
import { openProjectAssistant } from '../../lib/assistant-events';

function versionTimestamp(value:string):{label:string;title:string;valid:boolean}{
  const parsed=/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.exec(value);
  const date=new Date(value);const year=Number(parsed?.[1]),month=Number(parsed?.[2]),day=Number(parsed?.[3]);
  const days=month>=1&&month<=12?new Date(Date.UTC(year,month,0)).getUTCDate():0;
  if(!parsed||!Number.isFinite(date.getTime())||day<1||day>days||Number(parsed[4])>23||Number(parsed[5])>59||Number(parsed[6])>59)return {label:'时间未知',title:'版本记录的日期无效',valid:false};
  const pad=(n:number)=>String(n).padStart(2,'0');const label=`${String(date.getFullYear()).padStart(4,'0')}-${pad(date.getMonth()+1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return {label,title:`${label}:${pad(date.getSeconds())} · ${Intl.DateTimeFormat().resolvedOptions().timeZone}`,valid:true};
}

interface VersionHistoryProps {
  readonly projectId: ProjectId;
  readonly onClose: () => void;
  readonly readOnly?:boolean;
  readonly beforeReview?:()=>Promise<void>;
}

export function VersionHistory({ projectId, onClose,readOnly=false,beforeReview }: VersionHistoryProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selectedVersionId, setSelectedVersionId] = useState<VersionId | null>(null);
  const [diffFile, setDiffFile] = useState<string | null>(null);
  const owner=useRef({projectId});if(owner.current.projectId!==projectId)owner.current={projectId};
  const selectedRef=useRef(selectedVersionId);selectedRef.current=selectedVersionId;
  const [review,setReview]=useState<{projectId:string;versionId:string;plan:RestorePlan}|null>(null);
  useEffect(()=>{setSelectedVersionId(null);setDiffFile(null);setReview(null);},[projectId]);
  const visibleReview=review?.projectId===projectId&&review.versionId===selectedVersionId?review.plan:null;

  const versionsQuery = useQuery<ProjectVersion[], ApiError>({
    queryKey: ['versions', projectId],
    queryFn: () => api.versions.list(projectId),
    refetchInterval:15000,
  });

  const summaries=useVersionSummaries(projectId);
  const versionIds=versionsQuery.data?.map(version=>version.id).join(',')??'';
  useEffect(()=>{if(versionIds)void summaries.refetch();},[versionIds]);

  const snapshotMutation = useMutation<ProjectVersion, ApiError>({
    mutationFn: () => api.versions.snapshot(projectId, {}),
    onSuccess: async (version) => {
      toast.success(t('history.snapshotCreated', { count: version.fileCount }));
      await queryClient.invalidateQueries({ queryKey: ['versions', projectId] });
    },
    onError: (err) => {
      toast.error(err.body.message);
    },
  });

  const restoreMutation=useMutation({
    mutationFn:async(versionId:VersionId)=>{if(readOnly)throw new Error('当前项目为只读');const scope=owner.current;await beforeReview?.();if(owner.current!==scope)throw new Error('项目已切换');await api.versions.snapshot(projectId,{label:'恢复前自动备份'});const plan=await projectActionsApi.restorePlan(projectId,versionId);return {scope,versionId,plan};},
    onSuccess:async(result)=>{if(owner.current!==result.scope||selectedRef.current!==result.versionId)return;setReview({projectId,versionId:result.versionId,plan:result.plan});await queryClient.invalidateQueries({queryKey:['versions',projectId]});},
    onError:(error:Error)=>toast.error(error.message),
  });

  const versionPayloadQuery = useQuery({
    queryKey: ['version-payload', projectId, selectedVersionId],
    enabled: selectedVersionId !== null,
    queryFn: () => {
      if (selectedVersionId === null) throw new Error('no version');
      return api.versions.get(projectId, selectedVersionId);
    },
  });

  const diffHtml = useMemo(() => {
    if (versionPayloadQuery.data === undefined || diffFile === null) return null;
    const file = versionPayloadQuery.data.files.find((f) => f.path === diffFile);
    if (file === undefined) return null;
    // Compare snapshot to "" baseline; in a richer UI we'd diff against the
    // current Storage content. v1 just shows the snapshot itself with line
    // breaks preserved.
    const dmp = new DiffMatchPatch();
    const diffs = dmp.diff_main('', file.content);
    dmp.diff_cleanupSemantic(diffs);
    return diffs
      .map(([op, text]) => {
        const safe = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        if (op === 1) return `<ins class="bg-emerald-100 dark:bg-emerald-900/30">${safe}</ins>`;
        if (op === -1) return `<del class="bg-rose-100 dark:bg-rose-900/30">${safe}</del>`;
        return safe;
      })
      .join('');
  }, [versionPayloadQuery.data, diffFile]);

  return (
    <div className="flex h-full w-full flex-col bg-background">
      <div className="flex items-center justify-between border-b px-3 py-2">
        <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          <History className="h-3.5 w-3.5" aria-hidden="true" />
          {t('history.title')}
        </h3>
        <Button
          variant="ghost"
          size="icon"
          aria-label={t('common.close')}
          className="h-6 w-6"
          onClick={onClose}
        >
          <X className="h-3 w-3" aria-hidden="true" />
        </Button>
      </div>
      <div className="border-b p-3">
        <Button
          size="sm"
          className="w-full gap-1.5"
          onClick={() => {
            snapshotMutation.mutate();
          }}
          disabled={snapshotMutation.isPending||readOnly}
        >
          {snapshotMutation.isPending ? (
            <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
          ) : (
            <CameraIcon className="h-3 w-3" aria-hidden="true" />
          )}
          {t('history.snapshotNow')}
        </Button>
      </div>
      {summaries.isError&&<p role="alert" className="border-b px-3 py-2 text-xs text-muted-foreground">摘要状态暂未加载。<button type="button" className="ml-2 underline" onClick={()=>{void summaries.refetch();}}>重试</button></p>}
      <div className="flex-1 overflow-auto">
        {versionsQuery.isLoading ? (
          <div className="p-3">
            <Skeleton className="h-10 w-full" />
          </div>
        ) : (versionsQuery.data ?? []).length === 0 ? (
          <p className="p-3 text-xs text-muted-foreground">{t('history.empty')}</p>
        ) : (
          <ul className="space-y-px p-2">
            {(versionsQuery.data ?? []).map((v) => {
              const isSelected = selectedVersionId === v.id;
              const timestamp=versionTimestamp(v.createdAt);
              return (
                <li key={v.id}>
                  <button
                    type="button"
                    className={`w-full rounded px-2 py-1.5 text-left text-xs ${
                      isSelected ? 'bg-accent' : 'hover:bg-accent/60'
                    }`}
                    onClick={() => {
                      setSelectedVersionId(v.id);
                      setDiffFile(null);setReview(null);
                    }}
                  >
                    {v.label&&<div className="font-medium">{v.label}</div>}
                    <time className="block font-medium" dateTime={timestamp.valid?v.createdAt:undefined} title={timestamp.title}>{timestamp.label}</time>
                    <div className="text-[10px] text-muted-foreground">
                      {t('history.fileCount', { count: v.fileCount })}
                      {v.authorDisplayName !== null ? ` · ${v.authorDisplayName}` : ''}
                    </div>
                  </button>
                  <VersionSummaryRow projectId={projectId} versionId={v.id} item={summaries.data?.summaries.find(summary=>summary.versionId===v.id)} readOnly={readOnly}/>
                </li>
              );
            })}
          </ul>
        )}
      </div>
      {selectedVersionId !== null ? (
        <div className="flex flex-col border-t" style={{ maxHeight: '50%' }}>
          <div className="flex items-center justify-between border-b p-2">
            <span className="text-xs font-medium">
              {versionPayloadQuery.data?.files.length ?? 0} {t('history.filesInVersion')}
            </span>
            <Button
              size="sm"
              variant="outline"
              className="gap-1.5"
              onClick={() => {
                restoreMutation.mutate(selectedVersionId);
              }}
              disabled={restoreMutation.isPending||readOnly}
            >
              {restoreMutation.isPending ? (
                <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
              ) : (
                <RotateCcw className="h-3 w-3" aria-hidden="true" />
              )}
              预览恢复
            </Button>
          </div>
          {visibleReview&&<div className="max-h-56 space-y-2 overflow-auto border-b p-2 text-xs"><p>按文件生成恢复预览，接受后才写入正文。</p>{visibleReview.edits.map(edit=><div key={edit.fileId} className="flex items-center justify-between gap-2"><span className="min-w-0 truncate" title={edit.path}>{edit.path}</span><Button size="sm" variant="outline" disabled={readOnly} onClick={()=>openProjectAssistant({projectId,source:'restore',restore:{filePath:edit.path,before:edit.before,after:edit.after,versionId:visibleReview.versionId}})}>预览此文件</Button></div>)}{visibleReview.edits.length===0&&<p className="text-muted-foreground">当前文本没有可预览的差异。</p>}{visibleReview.missingPaths.length>0&&<p className="text-muted-foreground">未恢复缺失路径：{visibleReview.missingPaths.join('、')}</p>}{visibleReview.unsupportedPaths.length>0&&<p className="text-muted-foreground">本轮未恢复：{visibleReview.unsupportedPaths.join('、')}</p>}</div>}
          <div className="flex-1 overflow-auto">
            {versionPayloadQuery.isLoading ? (
              <div className="p-3">
                <Loader2
                  className="h-4 w-4 animate-spin text-muted-foreground"
                  aria-hidden="true"
                />
              </div>
            ) : (
              <div className="grid grid-cols-2 h-full">
                <ul className="space-y-px overflow-auto border-r p-2 text-xs">
                  {(versionPayloadQuery.data?.files ?? []).map((f) => (
                    <li key={f.path}>
                      <button
                        type="button"
                        className={`w-full rounded px-2 py-1 text-left ${
                          diffFile === f.path ? 'bg-accent' : 'hover:bg-accent/60'
                        }`}
                        onClick={() => {
                          setDiffFile(f.path);
                        }}
                      >
                        {f.path}
                      </button>
                    </li>
                  ))}
                </ul>
                <div className="overflow-auto p-2 font-mono text-[10px]">
                  {diffFile !== null && diffHtml !== null ? (
                    <pre
                      className="whitespace-pre-wrap break-words"
                      dangerouslySetInnerHTML={{ __html: diffHtml }}
                    />
                  ) : (
                    <p className="text-muted-foreground">{t('history.selectFile')}</p>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}
