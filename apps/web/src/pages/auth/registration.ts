import { z } from "zod";

export const ADMIN_EMAIL = "970130431@qq.com";
export const RESEND_SECONDS = 60;
export const signupSchema = z
  .object({
    invitationCode: z
      .string()
      .trim()
      .min(16, "请输入完整的邀请码")
      .max(128, "邀请码格式不正确"),
    email: z.string().trim().email("请输入有效的邮箱地址"),
    password: z.string().min(8, "密码至少需要 8 个字符"),
    confirmPassword: z.string().min(1, "请再次输入密码"),
    displayName: z.string().trim().max(80).optional(),
  })
  .refine((values) => values.password === values.confirmPassword, {
    message: "两次输入的密码不一致",
    path: ["confirmPassword"],
  });
export type SignupValues = z.infer<typeof signupSchema>;
export const verificationSchema = z.object({
  token: z
    .string()
    .trim()
    .regex(/^\d{6,10}$/, "请输入邮件中的验证码"),
});
export type VerificationValues = z.infer<typeof verificationSchema>;

export function registrationError(
  error: unknown,
  stage: "signup" | "verify" | "resend",
): string {
  const value = error as {
    code?: string;
    message?: string;
    status?: number;
  } | null;
  const message = value?.message ?? "";
  const code = value?.code ?? "";
  if (
    code === "over_email_send_rate_limit" ||
    code === "over_request_rate_limit" ||
    value?.status === 429 ||
    /rate.limit|security purposes|too many/i.test(message)
  )
    return "操作太频繁，请稍等片刻后重新发送验证码。";
  if (
    code === "email_exists" ||
    code === "user_already_exists" ||
    /already (registered|exists)/i.test(message)
  )
    return "此邮箱已注册，请登录；尚未验证的邮箱可在验证码页面重新发送。";
  if (
    code === "otp_expired" ||
    /expired|invalid.*(token|otp)|(token|otp).*invalid/i.test(message)
  )
    return "验证码不正确或已过期，请检查后重试，或重新发送验证码。";
  if (/invitation|邀请码/i.test(message))
    return "邀请码无效、已过期或已用完，请更换有效的邀请码。";
  if (/audit log entry/i.test(message))
    return "暂时无法发送验证码。请等待 60 秒后重试，并确认邀请码仍有效。";
  if (/database error/i.test(message))
    return stage === "verify"
      ? "暂时无法完成邮箱验证；邀请码可能已过期或用完，请检查后重试。"
      : "暂时无法注册；请确认邀请码有效，然后重试。";
  if (/smtp|sending.*(email|confirmation)|email.*send/i.test(message))
    return "验证码邮件发送失败，请稍后重试。";
  if (error instanceof TypeError || /fetch|network|timeout/i.test(message))
    return "网络连接失败，请重试；已填写的内容会保留。";
  return stage === "verify"
    ? "验证未完成，请检查验证码后重试。"
    : stage === "resend"
      ? "验证码未能重新发送，请稍后重试。"
      : "注册未完成，请检查填写内容后重试。";
}

export function resendWait(sentAt: number, now = Date.now()): number {
  return Math.max(0, Math.ceil((sentAt + RESEND_SECONDS * 1000 - now) / 1000));
}
