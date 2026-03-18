export type UserRole = "trader" | "admin";

export type SessionUser = {
  id: string;
  role: UserRole;
  apiKey: string;
  apiKeyPreview: string;
};

export const SESSION_COOKIE = "exchange_session";

function normalizeApiKey(apiKey: string) {
  return apiKey.trim();
}

export function maskApiKey(apiKey: string) {
  const normalized = normalizeApiKey(apiKey);
  if (normalized.length <= 8) {
    return normalized;
  }

  return `${normalized.slice(0, 4)}...${normalized.slice(-4)}`;
}

export function createSessionForApiKey(apiKey: string, role: UserRole): SessionUser {
  const normalized = normalizeApiKey(apiKey);

  return {
    id: `${role}-${maskApiKey(normalized)}`,
    role,
    apiKey: normalized,
    apiKeyPreview: maskApiKey(normalized),
  };
}

export function encodeSessionCookie(session: SessionUser) {
  return Buffer.from(JSON.stringify(session), "utf8").toString("base64url");
}

export function decodeSessionCookie(value?: string | null): SessionUser | null {
  if (!value) {
    return null;
  }

  try {
    const parsed = JSON.parse(Buffer.from(value, "base64url").toString("utf8")) as Partial<SessionUser>;
    if (
      !parsed ||
      typeof parsed.id !== "string" ||
      (parsed.role !== "trader" && parsed.role !== "admin") ||
      typeof parsed.apiKey !== "string" ||
      typeof parsed.apiKeyPreview !== "string"
    ) {
      return null;
    }

    return {
      id: parsed.id,
      role: parsed.role,
      apiKey: parsed.apiKey,
      apiKeyPreview: parsed.apiKeyPreview,
    };
  } catch {
    return null;
  }
}

export function defaultRouteForRole(role: UserRole) {
  return role === "admin" ? "/admin" : "/trade";
}

export function readSessionFromCookieValue(value?: string | null) {
  return decodeSessionCookie(value);
}
