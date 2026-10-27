import { describe, expect, it } from 'vitest';
import { avatarColor, avatarInitials } from './avatar';

describe('user avatar fallback', () => {
  it('uses one Chinese character and English initials', () => {
    expect(avatarInitials('  张三 ')).toBe('张');
    expect(avatarInitials('Ada Lovelace')).toBe('AL');
    expect(avatarInitials('ada')).toBe('A');
    expect(avatarInitials('')).toBe('?');
  });
  it('keeps color stable across views and name changes', () => {
    expect(avatarColor('user-a')).toBe(avatarColor('user-a'));
    expect(avatarColor('user-a')).toMatch(/^#[\da-f]{6}$/);
    expect(avatarColor('user-a')).not.toBe(avatarColor('user-b'));
  });
});
