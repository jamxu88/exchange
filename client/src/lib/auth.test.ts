import {
  createSessionForApiKey,
  decodeSessionCookie,
  defaultRouteForRole,
  encodeSessionCookie,
  maskApiKey,
  resolveRoleForApiKey,
} from "@/lib/auth";

describe("auth session helpers", () => {
  it("creates trader sessions by default", () => {
    const session = createSessionForApiKey("trader-secret-key");

    expect(session.role).toBe("trader");
    expect(session.apiKey).toBe("trader-secret-key");
    expect(session.apiKeyPreview).toBe(maskApiKey("trader-secret-key"));
  });

  it("detects admin keys from server env", () => {
    expect(
      resolveRoleForApiKey("admin-secret", {
        EXCHANGE_ADMIN_API_KEYS: "other-key,admin-secret",
      } as NodeJS.ProcessEnv),
    ).toBe("admin");
  });

  it("supports dev shortcut keys for testing", () => {
    expect(
      resolveRoleForApiKey("admin", {
        NODE_ENV: "development",
      } as NodeJS.ProcessEnv),
    ).toBe("admin");
    expect(
      resolveRoleForApiKey("trader", {
        NODE_ENV: "development",
      } as NodeJS.ProcessEnv),
    ).toBe("trader");
  });

  it("does not use dev shortcut keys in production", () => {
    expect(
      resolveRoleForApiKey("admin", {
        NODE_ENV: "production",
      } as NodeJS.ProcessEnv),
    ).toBe("trader");
  });

  it("round-trips session cookies", () => {
    const encoded = encodeSessionCookie(
      createSessionForApiKey("desk-user-key", {
        EXCHANGE_ADMIN_API_KEYS: "desk-user-key",
      } as NodeJS.ProcessEnv),
    );

    expect(decodeSessionCookie(encoded)).toEqual({
      id: `admin-${maskApiKey("desk-user-key")}`,
      role: "admin",
      apiKey: "desk-user-key",
      apiKeyPreview: maskApiKey("desk-user-key"),
    });
  });

  it("rejects malformed cookie values", () => {
    expect(decodeSessionCookie("not-a-valid-session")).toBeNull();
  });

  it("maps default routes by role", () => {
    expect(defaultRouteForRole("trader")).toBe("/trade");
    expect(defaultRouteForRole("admin")).toBe("/admin");
  });
});
