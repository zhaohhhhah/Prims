import { ArrowRight, ChevronUp, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { PrimsSignature } from './PrimsSignature';

import type { PointerEvent, ReactNode } from 'react';

interface AuthArtShellProps {
  readonly mode: 'login' | 'signup';
  readonly title?: string;
  readonly description?: string;
  readonly children: ReactNode;
  readonly footer?: ReactNode;
}

export function AuthArtShell({ mode, title, description, children, footer }: AuthArtShellProps) {
  const { i18n } = useTranslation();
  const chinese = i18n.language.startsWith('zh');
  const [helpOpen, setHelpOpen] = useState(false);
  const helpTrigger = useRef<HTMLButtonElement>(null);
  const helpDialog = useRef<HTMLElement>(null);
  const dragStart = useRef<number | null>(null);
  const suppressClick = useRef(false);

  useEffect(() => {
    if (!helpOpen) return;
    const dialog = helpDialog.current;
    const focusable = () => Array.from(dialog?.querySelectorAll<HTMLElement>('button, a[href], [tabindex="0"]') ?? []);
    focusable()[0]?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        setHelpOpen(false);
      }
      if (event.key === 'Tab') {
        const items = focusable();
        const first = items[0];
        const last = items[items.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      helpTrigger.current?.focus();
    };
  }, [helpOpen]);

  const finishDrag = (event: PointerEvent<HTMLButtonElement>) => {
    if (dragStart.current !== null && dragStart.current - event.clientY > 24) {
      suppressClick.current = true;
      setHelpOpen(true);
    }
    dragStart.current = null;
  };

  return (
      <div className="auth-art-pane" data-auth-mode={mode}>
        <div className="auth-art-content">
          <PrimsSignature />
          <div className="auth-art-form">
            {title !== undefined && <h1 className="auth-art-title">{title}</h1>}
            {description !== undefined && <p className="auth-art-description">{description}</p>}
            <div className="auth-art-fields">{children}</div>
            {footer !== undefined && <div className="auth-art-footer">{footer}</div>}
          </div>
          <button ref={helpTrigger} className="auth-help-trigger" type="button" aria-haspopup="dialog" aria-expanded={helpOpen}
            onPointerDown={(event) => { dragStart.current = event.clientY; event.currentTarget.setPointerCapture(event.pointerId); }}
            onPointerUp={finishDrag} onPointerCancel={() => { dragStart.current = null; }}
            onClick={() => { if (suppressClick.current) { suppressClick.current = false; return; } setHelpOpen(true); }}>
            <span className="auth-help-grip" />
            <span>{chinese ? '需要帮助？' : 'Need a hand?'}</span><ChevronUp size={13} />
          </button>
        </div>
        {helpOpen && <div className="auth-help-layer">
          <button className="auth-help-backdrop" type="button" tabIndex={-1} onClick={() => setHelpOpen(false)} aria-label={chinese ? '关闭帮助' : 'Close help'} />
          <section ref={helpDialog} role="dialog" aria-modal="true" aria-labelledby="auth-help-title" className="auth-help-dialog">
            <div className="auth-help-dialog-grip" />
            <button className="auth-help-close" type="button" onClick={() => setHelpOpen(false)} aria-label={chinese ? '关闭帮助' : 'Close help'}><X size={18} /></button>
            <h2 id="auth-help-title">{chinese ? '让灵感顺利开始' : 'A little help getting started'}</h2>
            <div className="auth-help-item"><h3>{chinese ? '邀请码' : 'Invitation code'}</h3><p>{chinese ? '注册需要有效的邀请码。请向邀请你的人索取，或联系管理员申请。' : 'Ask the person who invited you for a valid code, or contact the administrator to request one.'}</p></div>
            <div className="auth-help-item"><h3>{chinese ? '联系管理员' : 'Contact the administrator'}</h3><a href="mailto:970130431@qq.com">970130431@qq.com <ArrowRight size={14} /></a></div>
          </section>
        </div>}
      </div>
  );
}
