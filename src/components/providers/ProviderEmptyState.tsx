import { Download, Loader2, RefreshCw, Users } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import type { AppId } from "@/lib/api/types";
import type { ProprietaryBootstrapState } from "@/lib/api";

interface ProviderEmptyStateProps {
  appId: AppId;
  onCreate?: () => void;
  onImport?: () => void;
  bootstrapState?: ProprietaryBootstrapState;
  onRetryBootstrap?: () => void;
  isRetryingBootstrap?: boolean;
}

export function ProviderEmptyState({
  appId,
  onCreate,
  onImport,
  bootstrapState,
  onRetryBootstrap,
  isRetryingBootstrap = false,
}: ProviderEmptyStateProps) {
  const { t } = useTranslation();
  const isProprietary = bootstrapState?.enabled === true;
  const showSnippetHint =
    !isProprietary &&
    (appId === "claude" || appId === "codex" || appId === "gemini");

  if (isProprietary) {
    const status = bootstrapState?.status ?? "pending";
    const isPending = status === "pending";
    const isReadyWithoutProvider = status === "ready";
    const isBlocked = status === "blocked";
    const title =
      status === "error"
        ? t("proprietaryBootstrap.errorTitle", {
            defaultValue: "开户注册失败",
          })
        : isReadyWithoutProvider
          ? t("proprietaryBootstrap.readyTitle", {
              defaultValue: "正在同步供应商",
            })
          : isBlocked
            ? t("proprietaryBootstrap.blockedTitle", {
                defaultValue: "设备暂不可用",
              })
            : t("proprietaryBootstrap.pendingTitle", {
                defaultValue: "正在开户注册",
              });
    const description =
      status === "error"
        ? bootstrapState?.lastError ||
          t("proprietaryBootstrap.errorDescription", {
            defaultValue: "请检查网络后重试。",
          })
        : isReadyWithoutProvider
          ? t("proprietaryBootstrap.readyDescription", {
              defaultValue: "托管供应商状态已就绪，但本地列表尚未刷新。",
            })
          : isBlocked
            ? t("proprietaryBootstrap.blockedDescription", {
                defaultValue: "请联系支持处理当前设备状态。",
              })
            : t("proprietaryBootstrap.pendingDescription", {
                defaultValue: "NewAPI 托管供应商正在同步，请稍候。",
              });

    return (
      <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border p-10 text-center">
        <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted">
          {isPending || isRetryingBootstrap ? (
            <Loader2 className="h-7 w-7 animate-spin text-muted-foreground" />
          ) : (
            <Users className="h-7 w-7 text-muted-foreground" />
          )}
        </div>
        <h3 className="text-lg font-semibold">{title}</h3>
        <p className="mt-2 max-w-lg text-sm text-muted-foreground">
          {description}
        </p>
        {!isBlocked && onRetryBootstrap && (
          <Button
            className="mt-6"
            onClick={onRetryBootstrap}
            disabled={isPending || isRetryingBootstrap}
          >
            {isRetryingBootstrap ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            {t("proprietaryBootstrap.retry", { defaultValue: "重试" })}
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border p-10 text-center">
      <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted">
        <Users className="h-7 w-7 text-muted-foreground" />
      </div>
      <h3 className="text-lg font-semibold">{t("provider.noProviders")}</h3>
      <p className="mt-2 max-w-lg text-sm text-muted-foreground">
        {t("provider.noProvidersDescription")}
      </p>
      {showSnippetHint && (
        <p className="mt-1 max-w-lg text-sm text-muted-foreground">
          {t("provider.noProvidersDescriptionSnippet")}
        </p>
      )}
      <div className="mt-6 flex flex-col gap-2">
        {onImport && (
          <Button onClick={onImport}>
            <Download className="mr-2 h-4 w-4" />
            {appId === "claude-desktop"
              ? t("provider.importFromClaude", {
                  defaultValue: "从 Claude 导入兼容供应商",
                })
              : t("provider.importCurrent")}
          </Button>
        )}
        {onCreate && (
          <Button variant={onImport ? "outline" : "default"} onClick={onCreate}>
            {t("provider.addProvider")}
          </Button>
        )}
      </div>
    </div>
  );
}
