// providers.json 的前端类型，镜像 Rust store.rs（camelCase）。
export type AppType = "claude" | "codex" | "grok";

export interface Provider {
  id: string;
  name: string;
  category?: string | null;
  /** 写进 live 配置的内容：Claude={env}，Codex={auth,config}，Grok={config} */
  settingsConfig: Record<string, unknown>;
  meta?: Record<string, unknown>;
  failover?: Record<string, unknown>;
}

export interface AppData {
  current?: string | null;
  order: string[];
  providers: Record<string, Provider>;
}

export interface Root {
  version: number;
  apps: Record<string, AppData>;
  settings?: Record<string, unknown>;
}
