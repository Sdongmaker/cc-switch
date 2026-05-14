import { invoke } from "@tauri-apps/api/core";

export interface ProprietaryBootstrapState {
  enabled: boolean;
  status?: "pending" | "ready" | "error" | "blocked" | string;
  lastAction?: string | null;
  lastSuccessAt?: number | null;
  lastAttemptAt?: number | null;
  lastError?: string | null;
  providerBaseUrl?: string | null;
}

export interface ClaimLinkData {
  claimUrl: string;
  expiresAt: number;
}

export const proprietaryBootstrapApi = {
  async getState(): Promise<ProprietaryBootstrapState> {
    return await invoke("get_proprietary_bootstrap_state");
  },

  async retry(): Promise<ProprietaryBootstrapState> {
    return await invoke("retry_proprietary_bootstrap");
  },

  async claimAccountLink(): Promise<ClaimLinkData> {
    return await invoke("claim_account_link");
  },
};
