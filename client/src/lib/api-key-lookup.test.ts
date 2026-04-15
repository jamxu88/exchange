import { describe, expect, it } from "vitest";
import {
  normalizeLookupIdentifier,
  parseAllocatedApiKeysFile,
} from "@/lib/api-key-lookup";

describe("parseAllocatedApiKeysFile", () => {
  it("loads identifier to api key mappings from the TSV format", () => {
    const table = parseAllocatedApiKeysFile(
      [
        "identifier\tapi_key",
        "fl20k\texch_123",
        " QUNNE \texch_456 ",
        "",
      ].join("\n"),
    );

    expect(table.get("FL20K")).toBe("exch_123");
    expect(table.get("QUNNE")).toBe("exch_456");
  });
});

describe("normalizeLookupIdentifier", () => {
  it("trims whitespace and normalizes identifiers to uppercase", () => {
    expect(normalizeLookupIdentifier("  fl20k ")).toBe("FL20K");
  });
});
