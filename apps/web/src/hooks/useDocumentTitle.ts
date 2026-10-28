import { useEffect } from 'react';

const APP_TITLE = 'Prims — Compose with AI';

/** Use the project name in its editor and the site title on other routes. */
export function useDocumentTitle(title: string | null | undefined, project = false): void {
  useEffect(() => {
    document.title = project && title ? title : APP_TITLE;
    return () => { document.title = APP_TITLE; };
  }, [title, project]);
}
