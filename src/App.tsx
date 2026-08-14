import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { ClipboardList, History as HistoryIcon, Laptop, Settings as SettingsIcon } from "lucide-react";
import { useApp } from "./stores/app";
import HistoryPage from "./pages/History";
import DevicesPage from "./pages/Devices";
import SettingsPage from "./pages/Settings";
import type { PairingEvent, PairingResult } from "./lib/types";

export default function App() {
  const page = useApp((s) => s.page);
  const settings = useApp((s) => s.settings);
  const own = useApp((s) => s.own);
  const toasts = useApp((s) => s.toasts);
  const refreshState = useApp((s) => s.refreshState);
  const refreshHistory = useApp((s) => s.refreshHistory);
  const toast = useApp((s) => s.toast);
  const setIncomingPairing = useApp((s) => s.setIncomingPairing);
  const setWaitingPairing = useApp((s) => s.setWaitingPairing);
  const setPage = useApp((s) => s.setPage);

  useEffect(() => {
    void refreshState();
    void refreshHistory();
    // 后端事件
    const unlisteners: Promise<() => void>[] = [
      listen("lanclip://devices-changed", () => void refreshState()),
      listen("lanclip://history-changed", () => void refreshHistory()),
      listen("lanclip://settings-changed", () => void refreshState()),
      listen<PairingEvent>("lanclip://pairing-incoming", (e) => {
        setIncomingPairing(e.payload);
        toast("info", `收到来自 ${e.payload.deviceName} 的配对请求`);
        setPage("devices");
      }),
      listen<PairingEvent>("lanclip://pairing-waiting", (e) => {
        setWaitingPairing(e.payload);
      }),
      listen<PairingResult>("lanclip://pairing-result", (e) => {
        const { ok, message, deviceName } = e.payload;
        toast(ok ? "ok" : "err", ok ? `与 ${deviceName} 配对成功` : message);
        setWaitingPairing(null);
        setIncomingPairing(null);
        void refreshState();
      }),
    ];
    // 轮询设备在线状态
    const timer = setInterval(() => void refreshState(), 5000);
    return () => {
      clearInterval(timer);
      unlisteners.forEach((p) => p.then((f) => f()));
    };
  }, [refreshState, refreshHistory, toast, setIncomingPairing, setWaitingPairing, setPage]);

  const nav: { key: typeof page; label: string; icon: React.ReactNode }[] = [
    { key: "history", label: "历史记录", icon: <HistoryIcon size={18} /> },
    { key: "devices", label: "设备", icon: <Laptop size={18} /> },
    { key: "settings", label: "设置", icon: <SettingsIcon size={18} /> },
  ];

  return (
    <div className="flex h-full bg-slate-50 text-slate-800">
      {/* 侧边栏 */}
      <aside className="flex w-52 shrink-0 flex-col border-r border-slate-200 bg-white">
        <div className="flex items-center gap-2.5 px-5 py-5">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-indigo-600 text-white">
            <ClipboardList size={20} />
          </div>
          <div>
            <div className="text-[15px] font-semibold leading-tight">LanClip</div>
            <div className="text-[11px] text-slate-400">局域网剪切板同步</div>
          </div>
        </div>

        <nav className="mt-2 flex flex-col gap-1 px-3">
          {nav.map((n) => (
            <button
              key={n.key}
              onClick={() => setPage(n.key)}
              className={`flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors ${
                page === n.key
                  ? "bg-indigo-50 font-medium text-indigo-700"
                  : "text-slate-600 hover:bg-slate-100"
              }`}
            >
              {n.icon}
              {n.label}
            </button>
          ))}
        </nav>

        <div className="mt-auto px-5 py-4 text-[11px] leading-relaxed text-slate-400">
          <div className="flex items-center gap-1.5">
            <span
              className={`inline-block h-2 w-2 rounded-full ${
                settings?.watchEnabled ? "bg-emerald-500" : "bg-slate-300"
              }`}
            />
            {settings?.watchEnabled ? "剪切板监听中" : "监听已暂停"}
          </div>
          <div className="mt-1">v{own?.version ?? "-"} · 无服务器 · 端到端加密</div>
        </div>
      </aside>

      {/* 内容区 */}
      <main className="flex-1 overflow-hidden">
        {page === "history" && <HistoryPage />}
        {page === "devices" && <DevicesPage />}
        {page === "settings" && <SettingsPage />}
      </main>

      {/* Toast */}
      <div className="pointer-events-none fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`rounded-lg px-4 py-2 text-sm text-white shadow-lg ${
              t.kind === "ok" ? "bg-emerald-600" : t.kind === "err" ? "bg-rose-600" : "bg-slate-700"
            }`}
          >
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
