export type ItemStatus = "pending" | "accepted" | "ignored" | "expired";

export interface HistoryItemView {
  id: string;
  sourceDeviceId: string;
  sourceDeviceName: string;
  contentType: "text" | "html" | "url" | string;
  preview: string;
  contentSize: number;
  status: ItemStatus;
  createdAt: number;
  expiresAt: number | null;
  persistent: boolean;
}

export interface DeviceView {
  deviceId: string;
  name: string;
  online: boolean;
  paired: boolean;
  sendEnabled: boolean;
  receiveEnabled: boolean;
  lastSeenAt: number | null;
  address: string | null;
}

export interface SettingsView {
  deviceName: string;
  watchEnabled: boolean;
  notifyPreview: boolean;
  systemNotification: boolean;
  saveHistory: boolean;
  retentionDays: number;
  maxItems: number;
  autostart: boolean;
}

export interface OwnInfo {
  deviceId: string;
  deviceName: string;
  version: string;
}

export interface StateView {
  own: OwnInfo;
  settings: SettingsView;
  devices: DeviceView[];
}

export interface PairingEvent {
  deviceId: string;
  deviceName: string;
  code: string;
}

export interface PairingResult {
  deviceId: string;
  deviceName: string;
  ok: boolean;
  message: string;
}

export interface ContentView {
  contentType: string;
  text: string;
  html: string | null;
}

export interface IncomingPayload {
  item: HistoryItemView;
  previewAllowed: boolean;
}

export function formatTime(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
