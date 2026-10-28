import { describe, it, expect, vi } from 'vitest';
import { completeAuthCallback, callbackError } from './auth-callback';
const client = () => ({ verifyOtp: vi.fn().mockResolvedValue({ data: { session: { user: {} } }, error: null }), exchangeCodeForSession: vi.fn().mockResolvedValue({error:null}), getSession: vi.fn().mockResolvedValue({ data: { session: { user: {} } } }) });
describe('emailed password recovery', () => {
 it('works in a fresh browser without any PKCE verifier', async()=>{const c=client();expect(await completeAuthCallback(c,'https://example.com/auth/callback?token_hash=one-time-proof&type=recovery&next=https://evil.test')).toBe('/auth/reset-password');expect(c.verifyOtp).toHaveBeenCalledWith({token_hash:'one-time-proof',type:'recovery'});expect(c.exchangeCodeForSession).not.toHaveBeenCalled();});
 it('rejects unrelated token types', async()=>{const c=client();await expect(completeAuthCallback(c,'https://example.com/auth/callback?token_hash=x&type=signup')).rejects.toThrow();expect(c.verifyOtp).not.toHaveBeenCalled();});
 it('does not navigate without a recovery session', async()=>{const c=client();c.verifyOtp.mockResolvedValue({data:{session:null},error:null});await expect(completeAuthCallback(c,'https://example.com/auth/callback?token_hash=x&type=recovery')).rejects.toThrow('重置会话');});
 it('makes already used links recoverable in the UI', async()=>{const c=client();c.verifyOtp.mockResolvedValue({data:{session:null},error:{message:'Token has expired or is invalid'}});await expect(completeAuthCallback(c,'https://example.com/auth/callback?token_hash=x&type=recovery')).rejects.toThrow('重新申请');});
 it('preserves same-browser PKCE links',async()=>{const c=client();expect(await completeAuthCallback(c,'https://example.com/auth/callback?code=x&next=/auth/reset-password')).toBe('/auth/reset-password');expect(c.exchangeCodeForSession).toHaveBeenCalledWith('x');});
 it('explains missing verifier without exposing internals',async()=>{expect(callbackError('PKCE code verifier not found in storage')).toContain('旧邮件');});
 it('does not follow untrusted next destinations',async()=>{const c=client();expect(await completeAuthCallback(c,'https://example.com/auth/callback?next=https://evil.test')).toBe('/dashboard');});
});
