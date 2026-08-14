import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { api } from "../lib/ipc";
import { useApp } from "../stores/app";

export default function SettingsPage() {
  const settings = useApp((s) => s.settings);
  const refreshState = useApp((s) => s.refreshState);
  const toast = useApp((s) => s.toast);
  const [nameDraft, setNameDraft] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (settings) setNameDraft(settings.deviceName);
  }, [settings?.deviceName]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!settings) {
    return <div className="flex h-full items-center justify-center text-slate-400">加载中…</div>;
  }

  const patch = async (p: Partial<typeof settings>) => {
    setSaving(true);
    try {
      await api.updateSettings({ ...settings, ...p });
      await refreshState();
    } catch (e) {
      toast("err", `保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-slate-200 bg-white px-6 py-4">
        <h1 className="text-base font-semibold">设置</h1>
        {saving && (
          <span className="flex items-center gap-1 text-xs text-slate-400">
            <Loader2 size={12} className="animate-spin" /> 保存中…
          </span>
        )}
      </header>

      <div className="flex-1 overflow-y-auto px-6 py-5">
        <div className="mx-auto flex max-w-2xl flex-col gap-8">
          <Section title="通用">
            <Row label="设备名称" hint="局域网内其他设备看到的名字">
              <div className="flex gap-2">
                <input
                  value={nameDraft}
                  onChange={(e) => setNameDraft(e.target.value)}
                  className="w-56 rounded-lg border border-slate-200 px-3 py-1.5 text-sm outline-none focus:border-indigo-400 focus:ring-2 focus:ring-indigo-100"
                />
                <button
                  onClick={() => void patch({ deviceName: nameDraft.trim() || settings.deviceName })}
                  className="rounded-lg bg-indigo-600 px-3 py-1.5 text-sm text-white hover:bg-indigo-500"
                >
                  保存
                </button>
              </div>
            </Row>
            <Row label="开机自启动" hint="随系统启动并驻留托盘">
              <Switch checked={settings.autostart} onChange={(v) => void patch({ autostart: v })} />
            </Row>
          </Section>

          <Section title="同步">
            <Row label="监听剪切板" hint="关闭后本机复制不再发送，接收不受影响">
              <Switch checked={settings.watchEnabled} onChange={(v) => void patch({ watchEnabled: v })} />
            </Row>
          </Section>

          <Section title="通知">
            <Row label="通知内容预览" hint="关闭后通知只显示来源，不显示内容（敏感环境建议关闭）">
              <Switch checked={settings.notifyPreview} onChange={(v) => void patch({ notifyPreview: v })} />
            </Row>
            <Row label="系统通知" hint="主窗口隐藏时通过系统通知提醒">
              <Switch
                checked={settings.systemNotification}
                onChange={(v) => void patch({ systemNotification: v })}
              />
            </Row>
          </Section>

          <Section title="历史记录">
            <Row label="保存历史" hint="关闭后收到的内容只在内存中暂存，重启后消失">
              <Switch checked={settings.saveHistory} onChange={(v) => void patch({ saveHistory: v })} />
            </Row>
            <Row label="保留策略" hint="超期或超出条数后自动清理">
              <div className="flex items-center gap-2 text-sm text-slate-600">
                <input
                  type="number"
                  min={1}
                  max={90}
                  value={settings.retentionDays}
                  onChange={(e) => void patch({ retentionDays: Number(e.target.value) || 7 })}
                  className="w-20 rounded-lg border border-slate-200 px-2.5 py-1.5"
                />
                天 / 最近
                <input
                  type="number"
                  min={10}
                  max={10000}
                  step={100}
                  value={settings.maxItems}
                  onChange={(e) => void patch({ maxItems: Number(e.target.value) || 1000 })}
                  className="w-24 rounded-lg border border-slate-200 px-2.5 py-1.5"
                />
                条
              </div>
            </Row>
            <Row label="清空历史" hint="删除所有已保存的接收记录（含加密内容）">
              <button
                onClick={() => {
                  void api.clearHistory();
                  toast("ok", "历史已清空");
                }}
                className="rounded-lg border border-rose-200 px-3 py-1.5 text-sm text-rose-600 hover:bg-rose-50"
              >
                清空
              </button>
            </Row>
          </Section>

          <Section title="关于">
            <div className="rounded-xl bg-slate-50 p-4 text-xs leading-relaxed text-slate-500">
              LanClip v{useApp.getState().own?.version} — 无账号、无服务器、端到端加密的局域网剪切板收件箱。
              <br />
              技术栈：Tauri 2 · Rust · QUIC (quinn) · mDNS · SQLite · Ed25519 · BLAKE3
            </div>
          </Section>
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-slate-400">{title}</h2>
      <div className="flex flex-col gap-1 rounded-xl border border-slate-200 bg-white p-2">
        {children}
      </div>
    </section>
  );
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-4 px-3.5 py-3">
      <div className="flex-1">
        <div className="text-sm text-slate-700">{label}</div>
        {hint && <div className="mt-0.5 text-xs text-slate-400">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

function Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors ${
        checked ? "bg-indigo-600" : "bg-slate-300"
      }`}
    >
      <span
        className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all ${
          checked ? "left-[22px]" : "left-0.5"
        }`}
      />
    </button>
  );
}
