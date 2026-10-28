import { zodResolver } from "@hookform/resolvers/zod";
import { Button, Input, Label } from "@scribe/ui";
import { Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { z } from "zod";

import { useDocumentTitle } from "../../hooks/useDocumentTitle";
import { supabase } from "../../lib/supabase";
import { useAuthStore } from "../../stores/auth";

import { AuthArtShell } from "../../components/auth/AuthArtShell";

const passwordSchema = z.object({
  email: z.string().trim().email("请输入有效的邮箱地址"),
  password: z.string().min(8, { message: "At least 8 characters" }),
});
type PasswordValues = z.infer<typeof passwordSchema>;

const magicLinkSchema = z.object({
  email: z.string().trim().email("请输入有效的邮箱地址"),
});
type MagicLinkValues = z.infer<typeof magicLinkSchema>;

export function LoginPage() {
  const { t } = useTranslation();
  useDocumentTitle(t("auth.signIn"));
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const from = params.get("from") ?? "/dashboard";
  const status = useAuthStore((state) => state.status);
  const [mode, setMode] = useState<"password" | "magic">("password");
  const [magicSent, setMagicSent] = useState(false);
  const [error, setError] = useState("");
  const [unconfirmedEmail, setUnconfirmedEmail] = useState("");

  useEffect(() => {
    if (status === "authenticated") {
      void navigate(from, { replace: true });
    }
  }, [status, navigate, from]);

  const passwordForm = useForm<PasswordValues>({
    resolver: zodResolver(passwordSchema),
    defaultValues: { email: "", password: "" },
  });

  const magicForm = useForm<MagicLinkValues>({
    resolver: zodResolver(magicLinkSchema),
    defaultValues: { email: "" },
  });

  const onPasswordSubmit = async (values: PasswordValues) => {
    setError("");
    setUnconfirmedEmail("");
    try {
      const { error: failure } = await supabase.auth.signInWithPassword(values);
      if (failure) {
        if (failure.code === "email_not_confirmed") {
          setUnconfirmedEmail(values.email);
          setError("请先完成邮箱验证，再登录。");
        } else if (failure.status === 429) setError("操作太频繁，请稍后重试。");
        else setError("登录失败，请检查邮箱与密码。忘记密码时可通过邮件重置。");
        return;
      }
      void navigate(from, { replace: true });
    } catch {
      setError("网络连接失败，请稍后重试。");
    }
  };

  const onMagicSubmit = async (values: MagicLinkValues) => {
    setError("");
    setUnconfirmedEmail("");
    try {
      const { error: failure } = await supabase.auth.signInWithOtp({
        email: values.email,
        options: {
          shouldCreateUser: false,
          emailRedirectTo: `${window.location.origin}/auth/callback`,
        },
      });
      if (failure) {
        setError(
          failure.status === 429
            ? "发送太频繁，请稍后重试。"
            : "登录邮件发送失败，请确认邮箱已注册后重试。",
        );
        return;
      }
      setMagicSent(true);
    } catch {
      setError("网络连接失败，请稍后重试。");
    }
  };

  return (
    <AuthArtShell
      mode="login"
      title={t("auth.signIn")}
      footer={
        <>
          {t("auth.newToScribe")}{" "}
          <Link
            to="/signup"
            className="font-medium text-primary hover:underline"
          >
            {t("auth.signUp")}
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
          {unconfirmedEmail && (
            <>
              {" "}
              <Link
                className="font-medium underline"
                to={`/signup?verify=${encodeURIComponent(unconfirmedEmail)}`}
              >
                继续邮箱验证
              </Link>
            </>
          )}
        </p>
      )}
      {mode === "password" ? (
        <form
          onSubmit={passwordForm.handleSubmit((values) => {
            void onPasswordSubmit(values);
          })}
          className="space-y-4"
          noValidate
        >
          <div className="space-y-2">
            <Label htmlFor="email">{t("auth.email")}</Label>
            <Input
              id="email"
              type="email"
              autoComplete="email"
              {...passwordForm.register("email")}
              aria-invalid={passwordForm.formState.errors.email !== undefined}
            />
            {passwordForm.formState.errors.email !== undefined && (
              <p className="text-xs text-destructive">
                {passwordForm.formState.errors.email.message}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="password">{t("auth.password")}</Label>
              <Link
                to="/forgot"
                className="text-xs text-muted-foreground hover:underline"
              >
                {t("auth.forgotPassword")}
              </Link>
            </div>
            <Input
              id="password"
              type="password"
              autoComplete="current-password"
              {...passwordForm.register("password")}
              aria-invalid={
                passwordForm.formState.errors.password !== undefined
              }
            />
            {passwordForm.formState.errors.password !== undefined && (
              <p className="text-xs text-destructive">
                {passwordForm.formState.errors.password.message}
              </p>
            )}
          </div>
          <Button
            type="submit"
            className="w-full"
            disabled={passwordForm.formState.isSubmitting}
          >
            {passwordForm.formState.isSubmitting && (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            )}
            {t("auth.signIn")}
          </Button>
        </form>
      ) : magicSent ? (
        <p className="text-center text-sm text-muted-foreground">
          {t("auth.magicLinkSent")}
        </p>
      ) : (
        <form
          onSubmit={magicForm.handleSubmit((values) => {
            void onMagicSubmit(values);
          })}
          className="space-y-4"
          noValidate
        >
          <div className="space-y-2">
            <Label htmlFor="magic-email">{t("auth.email")}</Label>
            <Input
              id="magic-email"
              type="email"
              autoComplete="email"
              {...magicForm.register("email")}
            />
            {magicForm.formState.errors.email !== undefined && (
              <p className="text-xs text-destructive">
                {magicForm.formState.errors.email.message}
              </p>
            )}
          </div>
          <Button
            type="submit"
            className="w-full"
            disabled={magicForm.formState.isSubmitting}
          >
            {magicForm.formState.isSubmitting && (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
            )}
            {t("auth.signIn")}
          </Button>
        </form>
      )}

      <button
        type="button"
        onClick={() => {
          setMode((m) => (m === "password" ? "magic" : "password"));
          setMagicSent(false);
          setError("");
          setUnconfirmedEmail("");
        }}
        className="w-full text-center text-xs text-muted-foreground underline-offset-4 hover:underline"
      >
        {mode === "password"
          ? t("auth.useMagicLink")
          : t("auth.useEmailPassword")}
      </button>
    </AuthArtShell>
  );
}
