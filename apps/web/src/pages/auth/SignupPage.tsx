import { zodResolver } from "@hookform/resolvers/zod";
import { Button, Input, Label } from "@scribe/ui";
import { Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router-dom";

import { useDocumentTitle } from "../../hooks/useDocumentTitle";
import { supabase } from "../../lib/supabase";

import { AuthArtShell } from "../../components/auth/AuthArtShell";
import {
  registrationError,
  resendWait,
  signupSchema,
  verificationSchema,
} from "./registration";
import type { SignupValues, VerificationValues } from "./registration";

const PENDING_KEY = "prims.pending-registration";
type PendingRegistration = { email: string; sentAt: number };
function readPending(): PendingRegistration | null {
  try {
    const requestedEmail = new URLSearchParams(window.location.search).get(
      "verify",
    );
    if (requestedEmail && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(requestedEmail))
      return { email: requestedEmail, sentAt: 0 };
    const value = JSON.parse(
      sessionStorage.getItem(PENDING_KEY) ?? "null",
    ) as Partial<PendingRegistration> | null;
    return value &&
      typeof value.email === "string" &&
      /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.email) &&
      typeof value.sentAt === "number" &&
      Number.isFinite(value.sentAt)
      ? { email: value.email, sentAt: value.sentAt }
      : null;
  } catch {
    return null;
  }
}
function savePending(value: PendingRegistration | null) {
  try {
    if (value) sessionStorage.setItem(PENDING_KEY, JSON.stringify(value));
    else sessionStorage.removeItem(PENDING_KEY);
  } catch {
    /* Registration works with storage blocked. Never store passwords or invitation codes. */
  }
}

export function SignupPage() {
  const { t } = useTranslation();
  useDocumentTitle(t("auth.signUp"));
  const [pending, setPending] = useState<PendingRegistration | null>(
    readPending,
  );
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [resending, setResending] = useState(false);
  const [now, setNow] = useState(Date.now());
  const navigate = useNavigate();
  const form = useForm<SignupValues>({
    resolver: zodResolver(signupSchema),
    defaultValues: {
      email: "",
      password: "",
      confirmPassword: "",
      displayName: "",
      invitationCode: "",
    },
  });
  const verification = useForm<VerificationValues>({
    resolver: zodResolver(verificationSchema),
    defaultValues: { token: "" },
  });
  useEffect(() => {
    if (!pending) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [pending]);
  const wait = pending ? resendWait(pending.sentAt, now) : 0;
  const remember = (value: PendingRegistration | null) => {
    setPending(value);
    savePending(value);
    setNow(Date.now());
  };

  const onSubmit = async (values: SignupValues) => {
    setError("");
    setNotice("");
    try {
      const { data, error: failure } = await supabase.auth.signUp({
        email: values.email,
        password: values.password,
        options: {
          emailRedirectTo: `${window.location.origin}/auth/callback`,
          data: {
            display_name: values.displayName ?? "",
            invitation_code: values.invitationCode,
          },
        },
      });
      if (failure) {
        setError(registrationError(failure, "signup"));
        return;
      }
      // The server must require confirmation. Never treat auto-confirm as successful registration.
      if (data.session) {
        await supabase.auth.signOut({ scope: "local" });
        setError("当前暂不能发送验证码，请稍后再试。");
        return;
      }
      remember({ email: values.email, sentAt: Date.now() });
      form.resetField("password");
      form.resetField("confirmPassword");
      setNotice("验证码已发送，请查看收件箱或垃圾邮件。");
    } catch (failure) {
      setError(registrationError(failure, "signup"));
    }
  };
  const onVerify = async (values: VerificationValues) => {
    if (!pending) return;
    setError("");
    setNotice("");
    try {
      const { data, error: failure } = await supabase.auth.verifyOtp({
        email: pending.email,
        token: values.token,
        type: "signup",
      });
      if (failure) {
        setError(registrationError(failure, "verify"));
        return;
      }
      if (!data.session || !data.user?.email_confirmed_at) {
        setError("邮箱尚未验证成功，请重新输入验证码。");
        return;
      }
      remember(null);
      void navigate("/dashboard", { replace: true });
    } catch (failure) {
      setError(registrationError(failure, "verify"));
    }
  };
  const resend = async () => {
    if (!pending || wait || resending || verification.formState.isSubmitting)
      return;
    setResending(true);
    setError("");
    setNotice("");
    try {
      const { error: failure } = await supabase.auth.resend({
        type: "signup",
        email: pending.email,
        options: { emailRedirectTo: `${window.location.origin}/auth/callback` },
      });
      if (failure) {
        setError(registrationError(failure, "resend"));
        return;
      }
      remember({ ...pending, sentAt: Date.now() });
      verification.reset();
      setNotice("已重新发送，请使用最新的验证码。");
    } catch (failure) {
      setError(registrationError(failure, "resend"));
    } finally {
      setResending(false);
    }
  };

  return (
    <AuthArtShell
      mode="signup"
      title={pending ? "验证邮箱" : t("auth.signUp")}
      description={
        pending ? `验证码已发送至 ${pending.email}` : "使用邀请码和邮箱注册"
      }
      footer={
        <>
          {t("auth.alreadyHaveAccount")}{" "}
          <Link
            to="/login"
            className="font-medium text-primary hover:underline"
          >
            {t("auth.signIn")}
          </Link>
        </>
      }
    >
      {error && (
        <p
          role="alert"
          className="rounded-md bg-destructive/10 p-3 text-sm text-destructive"
        >
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="text-sm text-muted-foreground">
          {notice}
        </p>
      )}
      {pending ? (
        <>
          <form
            onSubmit={verification.handleSubmit((values) => {
              void onVerify(values);
            })}
            className="space-y-4"
            noValidate
          >
            <div className="space-y-2">
              <Label htmlFor="verificationCode">邮箱验证码</Label>
              <Input
                id="verificationCode"
                autoFocus
                autoComplete="one-time-code"
                inputMode="numeric"
                maxLength={10}
                {...verification.register("token")}
                aria-invalid={!!verification.formState.errors.token}
              />
              {verification.formState.errors.token && (
                <p className="text-xs text-destructive">
                  {verification.formState.errors.token.message}
                </p>
              )}
            </div>
            <Button
              type="submit"
              className="w-full"
              disabled={verification.formState.isSubmitting || resending}
            >
              {verification.formState.isSubmitting && (
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
              )}
              验证并注册
            </Button>
          </form>
          <p className="text-xs text-muted-foreground">
            验证将完成此前的注册。重复发送不会修改密码；需要更换密码时，请在登录页选择“忘记密码”。
          </p>
          <div className="flex items-center justify-between gap-3 text-sm">
            <button
              type="button"
              className="text-muted-foreground hover:underline disabled:opacity-50"
              disabled={
                wait > 0 || resending || verification.formState.isSubmitting
              }
              onClick={() => {
                void resend();
              }}
            >
              {resending
                ? "正在发送…"
                : wait
                  ? `${wait} 秒后重新发送`
                  : "重新发送验证码"}
            </button>
            <button
              type="button"
              className="text-muted-foreground hover:underline disabled:opacity-50"
              disabled={resending || verification.formState.isSubmitting}
              onClick={() => {
                form.setValue("email", "");
                form.resetField("password");
                form.resetField("confirmPassword");
                remember(null);
                verification.reset();
                setError("");
                setNotice("");
              }}
            >
              更换邮箱注册
            </button>
          </div>
        </>
      ) : (
        <form
          onSubmit={form.handleSubmit((values) => {
            void onSubmit(values);
          })}
          className="space-y-4"
          noValidate
        >
          <div className="space-y-2">
            <Label htmlFor="invitationCode">邀请码</Label>
            <Input
              id="invitationCode"
              autoComplete="off"
              {...form.register("invitationCode")}
            />
            {form.formState.errors.invitationCode && (
              <p className="text-xs text-destructive">
                {form.formState.errors.invitationCode.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="displayName">{t("auth.displayName")}</Label>
            <Input
              id="displayName"
              autoComplete="name"
              {...form.register("displayName")}
            />
            {form.formState.errors.displayName && (
              <p className="text-xs text-destructive">
                {form.formState.errors.displayName.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="email">{t("auth.email")}</Label>
            <Input
              id="email"
              type="email"
              autoComplete="email"
              {...form.register("email")}
              aria-invalid={!!form.formState.errors.email}
            />
            {form.formState.errors.email && (
              <p className="text-xs text-destructive">
                {form.formState.errors.email.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="password">{t("auth.password")}</Label>
            <Input
              id="password"
              type="password"
              autoComplete="new-password"
              {...form.register("password")}
              aria-invalid={!!form.formState.errors.password}
            />
            {form.formState.errors.password && (
              <p className="text-xs text-destructive">
                {form.formState.errors.password.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="confirmPassword">再次输入密码</Label>
            <Input
              id="confirmPassword"
              type="password"
              autoComplete="new-password"
              {...form.register("confirmPassword")}
              aria-invalid={!!form.formState.errors.confirmPassword}
            />
            {form.formState.errors.confirmPassword && (
              <p className="text-xs text-destructive">
                {form.formState.errors.confirmPassword.message}
              </p>
            )}
          </div>
          <Button
            type="submit"
            className="w-full"
            disabled={form.formState.isSubmitting}
          >
            {form.formState.isSubmitting && (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            )}
            发送验证码
          </Button>
          <button
            type="button"
            className="w-full text-center text-xs text-muted-foreground hover:underline disabled:opacity-50"
            disabled={form.formState.isSubmitting}
            onClick={() => {
              const email = form.getValues("email").trim();
              if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
                form.setError("email", { message: "请先填写注册邮箱" });
                return;
              }
              remember({ email, sentAt: 0 });
              setError("");
              setNotice("输入之前收到的验证码，或重新发送。");
            }}
          >
            已收到验证码？继续验证
          </button>
        </form>
      )}
    </AuthArtShell>
  );
}
