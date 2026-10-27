const COLORS = ['#765678', '#526d66', '#786048', '#446b82', '#796632', '#79535c', '#536580', '#5c6b40'];

export function avatarColor(id: string): string {
  let hash = 2166136261;
  for (const char of id) hash = Math.imul(hash ^ char.codePointAt(0)!, 16777619);
  return COLORS[(hash >>> 0) % COLORS.length]!;
}

export function avatarInitials(name: string): string {
  const text = name.trim();
  if (!text) return '?';
  if (/\p{Script=Han}/u.test(text[0]!)) return Array.from(text)[0]!;
  return text.split(/\s+/).slice(0, 2).map(part => Array.from(part)[0]).join('').toLocaleUpperCase();
}

/** Re-encode locally: strip metadata and keep the stored photo small. */
export async function prepareAvatar(file: File): Promise<Blob> {
  if (!['image/jpeg', 'image/png', 'image/webp'].includes(file.type)) throw new Error('type');
  if (file.size > 8 * 1024 * 1024) throw new Error('size');
  const bitmap = await createImageBitmap(file);
  try {
    if (bitmap.width < 1 || bitmap.height < 1 || bitmap.width * bitmap.height > 40_000_000) throw new Error('image');
    const size = Math.min(512, bitmap.width, bitmap.height);
    const canvas = document.createElement('canvas');
    canvas.width = size; canvas.height = size;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('image');
    const edge = Math.min(bitmap.width, bitmap.height);
    context.drawImage(bitmap, (bitmap.width - edge) / 2, (bitmap.height - edge) / 2, edge, edge, 0, 0, size, size);
    const png = await new Promise<Blob>((resolve, reject) => canvas.toBlob(value => value ? resolve(value) : reject(new Error('image')), 'image/png'));
    if (png.size > 2 * 1024 * 1024) throw new Error('size');
    return png;
  } finally { bitmap.close(); }
}
