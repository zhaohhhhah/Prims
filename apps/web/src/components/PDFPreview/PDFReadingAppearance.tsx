import { Sun } from 'lucide-react';
import { useId } from 'react';
import { useTranslation } from 'react-i18next';

import type { PdfAppearance, PdfReadingMode } from '../../lib/pdf-appearance';

interface Props {
  readonly appearance: PdfAppearance;
  readonly onChange: (value: PdfAppearance) => void;
}

/** Theme selection stays in the PDF toolbar, next to the always-visible brightness control. */
export function PDFReadingAppearance({ appearance, onChange }: Props) {
  const { i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.startsWith('zh') ?? false;
  const id = useId();
  const modes: readonly { value: PdfReadingMode; label: string }[] = [
    { value: 'original', label: zh ? '原色' : 'Original' },
    { value: 'soft', label: zh ? '柔光' : 'Soft' },
    { value: 'night', label: zh ? '夜读' : 'Night' },
    { value: 'warm', label: zh ? '暖黄纸' : 'Warm paper' },
    { value: 'green', label: zh ? '浅绿纸' : 'Green paper' },
  ];
  return (
    <div className="ml-auto flex min-w-0 max-w-full flex-wrap items-center justify-end gap-x-2 gap-y-1" role="group" aria-label={zh ? 'PDF 阅读外观' : 'PDF reading appearance'}>
      <select aria-label={zh ? 'PDF 阅读主题' : 'PDF reading theme'} value={appearance.mode}
        className="h-7 min-w-20 max-w-32 rounded-md border bg-background px-2 text-xs text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onChange={event => {
          const mode = event.target.value as PdfReadingMode;
          onChange({ mode, brightness: mode === 'original' ? 100 : appearance.brightness });
        }}>
        {modes.map(mode => <option key={mode.value} value={mode.value}>{mode.label}</option>)}
      </select>
      <label htmlFor={id} className="inline-flex shrink-0 items-center gap-1.5 text-[10px] text-muted-foreground">
        <Sun className="h-3.5 w-3.5" aria-hidden="true" />
        <span className="sr-only">{zh ? '亮度' : 'Brightness'}</span>
        <input id={id} className="w-14 accent-primary" type="range" min={60} max={120} step={1}
          value={appearance.brightness} onChange={event => onChange({ ...appearance, brightness: Number(event.target.value) })} />
        <span className="w-7 text-right tabular-nums">{appearance.brightness}%</span>
      </label>
    </div>
  );
}
