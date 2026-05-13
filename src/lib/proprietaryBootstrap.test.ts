import { describe, expect, it } from "vitest";
import {
  firstProprietarySupportedApp,
  isProprietaryMode,
  isProprietarySupportedApp,
  PROPRIETARY_SUPPORTED_APPS,
} from "./proprietaryBootstrap";

describe("proprietary bootstrap helpers", () => {
  it("reports proprietary mode only when backend state is enabled", () => {
    expect(isProprietaryMode()).toBe(false);
    expect(isProprietaryMode(null)).toBe(false);
    expect(isProprietaryMode({ enabled: false })).toBe(false);
    expect(isProprietaryMode({ enabled: true, status: "pending" })).toBe(true);
  });

  it("limits proprietary provider management to core apps", () => {
    expect(PROPRIETARY_SUPPORTED_APPS).toEqual(["claude", "codex", "gemini"]);
    expect(isProprietarySupportedApp("claude")).toBe(true);
    expect(isProprietarySupportedApp("codex")).toBe(true);
    expect(isProprietarySupportedApp("gemini")).toBe(true);
    expect(isProprietarySupportedApp("claude-desktop")).toBe(false);
    expect(isProprietarySupportedApp("opencode")).toBe(false);
    expect(isProprietarySupportedApp("openclaw")).toBe(false);
    expect(isProprietarySupportedApp("hermes")).toBe(false);
  });

  it("uses Claude as the fallback proprietary app", () => {
    expect(firstProprietarySupportedApp()).toBe("claude");
  });
});
