import { describe, expect, it } from "vitest";
import {
  registrationError,
  resendWait,
  signupSchema,
  verificationSchema,
} from "./registration";
const valid = {
  invitationCode: "VALID-INVITATION-CODE",
  email: "new@example.test",
  password: "long-password",
  confirmPassword: "long-password",
  displayName: "New user",
};
describe("email-confirmed invitation registration", () => {
  it("requires both matching passwords before any signup request", () => {
    expect(signupSchema.safeParse(valid).success).toBe(true);
    const mismatch = signupSchema.safeParse({
      ...valid,
      confirmPassword: "different",
    });
    expect(mismatch.success).toBe(false);
    if (!mismatch.success)
      expect(mismatch.error.issues[0]?.path).toEqual(["confirmPassword"]);
    expect(
      signupSchema.safeParse({ ...valid, invitationCode: "" }).success,
    ).toBe(false);
    expect(
      signupSchema.safeParse({ ...valid, confirmPassword: "" }).success,
    ).toBe(false);
  });
  it("normalizes email and accepts native GoTrue OTP lengths", () => {
    expect(
      signupSchema.parse({ ...valid, email: " new@example.test " }).email,
    ).toBe("new@example.test");
    for (const token of ["123456", "12345678", "1234567890", " 123456 "])
      expect(verificationSchema.safeParse({ token }).success).toBe(true);
    for (const token of ["", "1234", "12345678901", "abcdef", "<script>"])
      expect(verificationSchema.safeParse({ token }).success).toBe(false);
  });
  it("supports recoverable expiry, mail delivery and network errors without exposing backend details", () => {
    expect(registrationError({ code: "otp_expired" }, "verify")).toContain(
      "重新发送",
    );
    expect(
      registrationError({ message: "Database error updating user" }, "verify"),
    ).toContain("邀请码");
    expect(registrationError({ status: 429 }, "resend")).toContain("稍等");
    expect(
      registrationError(
        { message: "Database error creating audit log entry" },
        "resend",
      ),
    ).toContain("60 秒");
    expect(
      registrationError(new TypeError("Failed to fetch"), "signup"),
    ).toContain("会保留");
    expect(
      registrationError({ message: "smtp password PRIVATE_SECRET" }, "resend"),
    ).not.toContain("PRIVATE_SECRET");
  });
  it("calculates the resend cooldown by elapsed time, including reloaded pending state", () => {
    expect(resendWait(1000, 1000)).toBe(60);
    expect(resendWait(1000, 60501)).toBe(1);
    expect(resendWait(1000, 61000)).toBe(0);
    expect(resendWait(0, Date.now())).toBe(0);
  });
});
