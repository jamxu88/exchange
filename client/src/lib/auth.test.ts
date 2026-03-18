import {
  createSessionForApiKey,
  decodeSessionCookie,
  defaultRouteForRole,
  encodeSessionCookie,
  maskApiKey,
} from "@/lib/auth";

describe("auth session helpers", () => {
  it("creates trader sessions", () => {
    const session = createSessionForApiKey("trader-secret-key", "trader");

    expect(session.role).toBe("trader");
    expect(session.apiKey).toBe("trader-secret-key");
    expect(session.apiKeyPreview).toBe(maskApiKey("trader-secret-key"));
  });

  it("creates admin sessions", () => {
    const session = createSessionForApiKey("admin-secret-key", "admin");

    expect(session.role).toBe("admin");
    expect(session.id).toBe(`admin-${maskApiKey("admin-secret-key")}`);
  });

  it("round-trips session cookies", () => {
    const encoded = encodeSessionCookie(
      createSessionForApiKey("desk-user-key", "admin"),
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
