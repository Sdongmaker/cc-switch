import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { ExternalLink, Loader2, ShieldCheck } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { proprietaryBootstrapApi } from "@/lib/api";

export function ManagedAccountSection() {
  const { t } = useTranslation();
  const [isClaiming, setIsClaiming] = useState(false);

  const { data: bootstrapState } = useQuery({
    queryKey: ["proprietaryBootstrap"],
    queryFn: () => proprietaryBootstrapApi.getState(),
  });

  if (!bootstrapState?.enabled || bootstrapState?.status !== "ready") {
    return null;
  }

  const handleClaimLink = async () => {
    setIsClaiming(true);
    try {
      const data = await proprietaryBootstrapApi.claimAccountLink();
      await invoke("open_external", { url: data.claimUrl });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      toast.error(
        t("settings.managedAccount.claimError", {
          defaultValue: "获取认领链接失败，请稍后重试",
        }),
        { description: message },
      );
    } finally {
      setIsClaiming(false);
    }
  };

  return (
    <section className="rounded-xl border border-border/60 bg-card/60 p-6">
      <div className="mb-4 flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-muted">
          <ShieldCheck className="h-5 w-5 text-primary" />
        </div>
        <div>
          <h4 className="font-medium">
            {t("settings.managedAccount.title", { defaultValue: "账号管理" })}
          </h4>
          <p className="text-sm text-muted-foreground">
            {t("settings.managedAccount.description", {
              defaultValue: "管理 NewAPI 账号、充值或登录",
            })}
          </p>
        </div>
      </div>

      <Button
        variant="outline"
        className="w-full"
        onClick={handleClaimLink}
        disabled={isClaiming}
      >
        {isClaiming ? (
          <>
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            {t("settings.managedAccount.loading", {
              defaultValue: "正在生成认领链接...",
            })}
          </>
        ) : (
          <>
            <ExternalLink className="mr-2 h-4 w-4" />
            {t("settings.managedAccount.button", {
              defaultValue: "管理账号 / 充值",
            })}
          </>
        )}
      </Button>
    </section>
  );
}
