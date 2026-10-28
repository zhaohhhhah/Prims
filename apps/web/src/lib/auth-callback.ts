export const RESET_PASSWORD_PATH = '/auth/reset-password';
export interface CallbackAuth {
  verifyOtp(input: { token_hash: string; type: 'recovery' }): Promise<{ data: { session: unknown }; error: { message: string } | null }>;
  exchangeCodeForSession(code: string): Promise<{ error: { message: string } | null }>;
  getSession(): Promise<{ data: { session: unknown } }>;
}
export function callbackError(message: string): string {
  if (/code.verifier|pkce|flow.state|code.challenge/i.test(message)) return '这封旧邮件依赖原浏览器的验证状态。请重新申请重置邮件，再使用新邮件中的链接。';
  if (/expired|invalid.*token|otp_expired|token.*invalid/i.test(message)) return '重置链接已失效或已经使用，请重新申请。';
  return message;
}
/** Recovery links verify the emailed one-time token, without a browser-local PKCE verifier. */
export async function completeAuthCallback(auth: CallbackAuth, href: string): Promise<string> {
  const url = new URL(href);
  const error = url.searchParams.get('error_description') ?? new URLSearchParams(url.hash.slice(1)).get('error_description');
  if (error) throw new Error(callbackError(error));
  const tokenHash = url.searchParams.get('token_hash');
  if (tokenHash !== null) {
    if (!tokenHash || url.searchParams.get('type') !== 'recovery') throw new Error('无效的密码重置链接，请重新申请。');
    const result = await auth.verifyOtp({ token_hash: tokenHash, type: 'recovery' });
    if (result.error) throw new Error(callbackError(result.error.message));
    if (!result.data.session) throw new Error('未能建立重置会话，请重新申请。');
    return RESET_PASSWORD_PATH;
  }
  const code = url.searchParams.get('code');
  if (code !== null) {
    const result = await auth.exchangeCodeForSession(code);
    if (result.error) throw new Error(callbackError(result.error.message));
  }
  const { data } = await auth.getSession();
  if (!data.session) throw new Error('登录链接已失效，请重新申请。');
  const recovery = url.searchParams.get('next') === RESET_PASSWORD_PATH || new URLSearchParams(url.hash.slice(1)).get('type') === 'recovery';
  return recovery ? RESET_PASSWORD_PATH : '/dashboard';
}
