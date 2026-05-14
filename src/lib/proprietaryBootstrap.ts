import type { AppId } from "@/lib/api";
import type { ProprietaryBootstrapState } from "@/lib/api/proprietaryBootstrap";

export const PROPRIETARY_SUPPORTED_APPS = [
  "claude",
  "codex",
  "gemini",
] as const satisfies readonly AppId[];

export function isProprietaryMode(
  state?: ProprietaryBootstrapState | null,
): boolean {
  return state?.enabled === true;
}

export function isProprietarySupportedApp(appId: AppId): boolean {
  return (PROPRIETARY_SUPPORTED_APPS as readonly string[]).includes(appId);
}

export function firstProprietarySupportedApp(): AppId {
  return PROPRIETARY_SUPPORTED_APPS[0];
}

export function isManagedProviderId(id: string): boolean {
  return id === "managed-newapi";
}

export function isManagedUniversalProvider(provider: {
  providerType?: string;
  id?: string;
}): boolean {
  return (
    provider.providerType === "managed_newapi" ||
    isManagedProviderId(provider.id || "")
  );
}
