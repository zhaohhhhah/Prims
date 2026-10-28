/** The visible workspace flushes its live editor before file-tree operations. */
type Guard = (write: boolean) => Promise<() => void>;
const guards = new Map<string, Guard>();
export function registerFileOperationGuard(projectId: string, guard: Guard): () => void {
  guards.set(projectId, guard);
  return () => { if (guards.get(projectId) === guard) guards.delete(projectId); };
}
export async function withFileOperationGuard<T>(projectId: string, write: boolean, operation: () => Promise<T>): Promise<T> {
  const guard = guards.get(projectId);
  if (!guard) throw new Error('编辑器尚未就绪，请稍后再试 / Editor is not ready');
  const release = await guard(write);
  try { return await operation(); } finally { release(); }
}
