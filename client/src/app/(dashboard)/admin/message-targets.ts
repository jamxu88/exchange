export type AdminMessageAudience = "all" | "single" | "list";

export function parseUsernameList(raw: string) {
  const seen = new Set<string>();

  return raw
    .split(/[\s,]+/)
    .map((username) => username.trim())
    .filter((username) => {
      if (username.length === 0) {
        return false;
      }

      const normalized = username.toLowerCase();
      if (seen.has(normalized)) {
        return false;
      }

      seen.add(normalized);
      return true;
    });
}

export function resolveAdminMessageTargets(
  audience: string,
  targetUsername: string | null,
  targetUsernames: string,
): { audience: AdminMessageAudience; targets: string[] } {
  if (audience === "all") {
    return { audience: "all", targets: [] };
  }

  if (audience === "single") {
    if (!targetUsername) {
      throw new Error("Target username is required for a single-user message.");
    }

    return { audience: "single", targets: [targetUsername] };
  }

  if (audience === "list") {
    const targets = parseUsernameList(targetUsernames);
    if (targets.length === 0) {
      throw new Error("Add at least one username for a list send.");
    }

    return { audience: "list", targets };
  }

  throw new Error("Unknown admin message audience.");
}
