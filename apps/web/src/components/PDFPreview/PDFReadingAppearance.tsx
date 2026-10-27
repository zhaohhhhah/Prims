import { Button } from '@scribe/ui';
import { Check, Eye, Moon, Sun, X } from 'lucide-react';
import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import type { PdfAppearance, PdfReadingMode } from '../../lib/pdf-appearance';

interface Props {
  readonly appearance: PdfAppearance;
  readonly onChange: (value: PdfAppearance) => void;
}

/** Compact control lives in the existing PDF/log tab row, never in general appearance settings. */
export function PDFReadingAppearance({ appearance, onChange }: Props) {
  const { i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.startsWith('zh') ?? false;
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const title = zh ? 'PDF 阅读外观' : 'PDF reading appearance';
  const modes: readonly { value: PdfReadingMode; label: string }[] = [
    { value: 'original', label: zh ? '原色' : 'Original' },
    { value: 'soft', label: zh ? '柔光' : 'Soft' },
    { value: 'night', label: zh ? '夜读' : 'Night' },
  ];

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener('pointerdown', onPointer);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onPointer);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  return (
    <div ref={root} className="relative ml-auto shrink-0">
      <Button
        ref={trigger}
        variant="ghost"
        size="icon"
        className="h-7 w-7"
        aria-label={title}
        title={title}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => { setOpen((previous) => !previous); }}
      >
        <Eye className="h-3.5 w-3.5" aria-hidden="true" />
      </Button>
      {open ? (
        <div id={id} role="dialog" aria-label={title} className="absolute right-0 top-full z-50 mt-1 w-60 rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg">
          <div className="mb-3 flex items-center justify-between gap-2 text-xs font-medium">
            <span>{title}</span>
            <Button variant="ghost" size="icon" className="h-5 w-5" aria-label={zh ? '关闭' : 'Close'} onClick={() => { setOpen(false); trigger.current?.focus(); }}>
              <X className="h-3 w-3" aria-hidden="true" />
            </Button>
          </div>
          <div role="group" aria-label={zh ? '阅读模式' : 'Reading mode'} className="grid grid-cols-3 gap-1">
            {modes.map((mode) => (
              <Button key={mode.value} variant={appearance.mode === mode.value ? 'secondary' : 'ghost'} size="sm" className="h-8 gap-1 px-1 text-xs" aria-pressed={appearance.mode === mode.value} onClick={() => { onChange({ mode: mode.value, brightness: mode.value === 'original' ? 100 : appearance.brightness }); }}>
                {appearance.mode === mode.value ? <Check className="h-3 w-3" aria-hidden="true" /> : mode.value === 'night' ? <Moon className="h-3 w-3" aria-hidden="true" /> : <Sun className="h-3 w-3" aria-hidden="true" />}
                {mode.label}
              </Button>
            ))}
          </div>
          <label htmlFor={`${id}-brightness`} className="mb-1 mt-3 flex items-center justify-between text-xs">
            <span>{zh ? '亮度' : 'Brightness'}</span><span className="tabular-nums">{appearance.brightness}%</span>
          </label>
          <input id={`${id}-brightness`} className="block w-full accent-primary" type="range" min={60} max={120} step={1} value={appearance.brightness} onChange={(event) => { onChange({ ...appearance, brightness: Number(event.target.value) }); }} />
        </div>
      ) : null}
    </div>
  );
}
