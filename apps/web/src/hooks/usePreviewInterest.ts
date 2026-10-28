import type { ProjectId } from '@scribe/shared';
import { useEffect } from 'react';
import { previewInterest } from '../lib/dashboard';

/** A short server lease, renewed only while this editor is actually visible. */
export function usePreviewInterest(projectId: ProjectId | null) {
  useEffect(() => {
    if (!projectId) return;
    const controller = new AbortController();
    let pending = false;
    const heartbeat = () => {
      if (document.visibilityState !== 'visible' || pending || controller.signal.aborted) return;
      pending = true;
      void previewInterest(projectId, 'editor', controller.signal)
        .catch(() => { /* Preview scheduling never interrupts editing or offline work. */ })
        .finally(() => { pending = false; });
    };
    heartbeat();
    const timer = window.setInterval(heartbeat, 30_000);
    document.addEventListener('visibilitychange', heartbeat);
    return () => {
      controller.abort();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', heartbeat);
    };
  }, [projectId]);
}
