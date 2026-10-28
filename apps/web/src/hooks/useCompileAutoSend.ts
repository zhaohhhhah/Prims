import { useCallback, useEffect, useRef, useState } from 'react';

interface CompileAutoSendOptions {
  readonly ownerKey: string;
  readonly blocked: boolean;
  readonly contextReady: boolean;
  readonly contextLoading: boolean;
  readonly contextError: boolean;
  readonly modelsLoading: boolean;
  readonly modelsError: boolean;
  readonly modelReady: boolean;
  readonly send: (prompt: string) => void | Promise<void>;
  readonly onGuide: (message: string) => void;
}

/** Wait for the existing assistant to be ready, then consume one user-click request. */
export function useCompileAutoSend(options: CompileAutoSendOptions) {
  const current = useRef(options);
  current.current = options;
  const sequence = useRef(0);
  const started = useRef(0);
  const [request, setRequest] = useState<{ id: number; ownerKey: string; prompt: string } | null>(null);
  const enqueue = useCallback((prompt: string) => {
    setRequest({ id: ++sequence.current, ownerKey: current.current.ownerKey, prompt });
  }, []);
  const cancel = useCallback(() => setRequest(null), []);

  useEffect(() => {
    if (!request) return;
    const state = current.current;
    if (request.ownerKey !== state.ownerKey) { setRequest(null); return; }
    const guide = (message: string) => { setRequest(null); state.onGuide(message); };
    if (state.contextError) { guide('讨论加载失败，请先重试加载，再启动 AI 调试。'); return; }
    if (state.modelsError) { guide('模型列表加载失败，请重试加载模型后启动 AI 调试。'); return; }
    if (state.contextLoading || state.modelsLoading) return;
    if (!state.modelReady) { guide('请选择一个可用模型，或到设置中添加连接并设为默认'); return; }
    if (state.blocked || !state.contextReady) return;
    if (started.current === request.id) return;
    started.current = request.id;
    setRequest(null);
    void Promise.resolve(state.send(request.prompt)).catch((error: unknown) => {
      if (current.current.ownerKey === request.ownerKey) current.current.onGuide(error instanceof Error ? error.message : 'AI 调试未能启动，请重试。');
    });
  }, [request, options.ownerKey, options.blocked, options.contextReady, options.contextLoading, options.contextError, options.modelsLoading, options.modelsError, options.modelReady]);

  return { enqueue, cancel, pending: request !== null && request.ownerKey === options.ownerKey };
}
