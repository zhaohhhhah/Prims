export const TUTORIAL_PROJECT_NAME = '纸上微光 · Prims 写作手册';
export type InitialCompile = {status: string; pdfKey?: string | null};

/** Poll only the first tutorial preview; stop on a real terminal result. */
export function initialPdfPollInterval(name: string | undefined, hasPdf: boolean, job: InitialCompile | undefined, elapsedMs: number): number | false {
  if (name !== TUTORIAL_PROJECT_NAME || hasPdf || elapsedMs >= 180_000) return false;
  if (!job) return elapsedMs < 30_000 ? 2000 : false;
  return job.status === 'queued' || job.status === 'running' ? 2000 : false;
}

export function isCompilePending(job: InitialCompile): boolean {
  return job.status === 'queued' || job.status === 'running';
}
