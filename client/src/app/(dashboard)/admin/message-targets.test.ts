import { describe, expect, it } from "vitest";
import {
  parseUsernameList,
  resolveAdminMessageTargets,
} from "@/app/(dashboard)/admin/message-targets";

describe("parseUsernameList", () => {
  it("splits comma, space, and newline separated usernames and deduplicates them", () => {
    expect(parseUsernameList("alice, bob\ncarol alice  BOB")).toEqual([
      "alice",
      "bob",
      "carol",
    ]);
  });
});

describe("resolveAdminMessageTargets", () => {
  it("returns no explicit targets for broadcasts", () => {
    expect(resolveAdminMessageTargets("all", null, "")).toEqual({
      audience: "all",
      targets: [],
    });
  });

  it("requires a username for single-user sends", () => {
    expect(() => resolveAdminMessageTargets("single", null, "")).toThrow(
      "Target username is required for a single-user message.",
    );
  });

  it("returns a parsed list of usernames for list sends", () => {
    expect(resolveAdminMessageTargets("list", null, "alice\nbob,carol")).toEqual({
      audience: "list",
      targets: ["alice", "bob", "carol"],
    });
  });
});
