import { useState } from "react";
import { Check, Link, Loader2, ShieldCheck, Trash2, Unlink, X } from "lucide-react";
import { api } from "../lib/ipc";
import { useApp } from "../stores/app";
import { formatTime } from "../lib/types";

export default function DevicesPage() {
  const own = useApp((s) => s.own);
  const devices = useApp((s) => s.devices);
  const incomingPairing = useApp((s) => s.incomingPairing);
  const waitingPairing = useApp((s) => s.waitingPairing);
  const refreshState = useApp((s) => s.refreshState);
  const toast = useApp((s) => s.toast);
  const setIncomingPairing = useApp((s) => s.setIncomingPairing);
  const setWaitingPairing = useApp((s) => s.setWaitingPairing);

  const [pairing, setPairing] = useState<string | null>(null);
  const paired = devices.filter((d) => d.paired);
  const discovered = devices.filter((d) => !d.paired);

  const requestPair = async (deviceId: string) => {
    setPairing(deviceId);
    try {
      await api.pairRequest(deviceId);
    } catch (e) {
      toast("err", `${e}`);
      setWaitingPairing(null);
    } finally {
      setPairing(null);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-slate-200 bg-white px-6 py-4">
        <h1 className="text-base font-semibold">设备</h1>
        <span className="text-xs text-slate-400">
          局域网内自动发现，配对后才能互相同步（mDNS 发现 + 加密传输）
        </span>
      </header>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        {/* 本机 */}
        <section className="mb-6">
          <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-slate-400">本机</h2>
          <div className="flex items-center gap-3 rounded-xl border border-slate-200 bg-white p-4">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-emerald-100 text-emerald-600">
              <ShieldCheck size={20} />
            </div>
            <div>
              <div className="text-sm font-medium">{own?.deviceName ?? "…"}</div>
              <div className="mt-0.5 font-mono text-xs text-slate-400">
                ID {own?.deviceId ?? "…"}
              </div>
            </div>
          </div>
        </section>

        {/* 收到的配对请求（本机是被请求方） */}
        {incomingPairing && (
          <section className="mb-6">
            <div className="rounded-xl border-2 border-dashed border-indigo-300 bg-indigo-50/60 p-4">
              <div className="flex items-center gap-3">
                <div className="flex-1">
                  <div className="text-sm font-medium text-indigo-900">
                    「{incomingPairing.deviceName}」请求配对
                  </div>
                  <div className="mt-0.5 text-xs text-indigo-500">
                    请核对两台设备上显示的验证码一致后再确认
                  </div>
                </div>
                <div className="rounded-xl bg-white px-5 py-2.5 text-2xl font-bold tracking-[0.35em] text-indigo-700 shadow-sm">
                  {incomingPairing.code}
                </div>
              </div>
              <div className="mt-3 flex justify-end gap-2">
                <button
                  onClick={() => {
                    void api
                      .respondPairing(incomingPairing.deviceId, false)
                      .catch((e) => toast("err", `${e}`))
                      .finally(() => setIncomingPairing(null));
                  }}
                  className="flex items-center gap-1 rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
                >
                  <X size={14} /> 拒绝
                </button>
                <button
                  onClick={() => {
                    void api
                      .respondPairing(incomingPairing.deviceId, true)
                      .then(() => toast("ok", "配对成功"))
                      .catch((e) => toast("err", `${e}`))
                      .finally(() => setIncomingPairing(null));
                  }}
                  className="flex items-center gap-1 rounded-lg bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-500"
                >
                  <Check size={14} /> 验证码一致，确认配对
                </button>
              </div>
            </div>
          </section>
        )}

        {/* 等待对方确认（本机是请求方） */}
        {waitingPairing && (
          <section className="mb-6">
            <div className="flex items-center gap-3 rounded-xl border-2 border-dashed border-amber-300 bg-amber-50/60 p-4">
              <Loader2 size={18} className="animate-spin text-amber-600" />
              <div className="flex-1">
                <div className="text-sm font-medium text-amber-900">
                  等待「{waitingPairing.deviceName}」确认配对…
                </div>
                <div className="mt-0.5 text-xs text-amber-600">
                  请核对两台设备上的验证码一致
                </div>
              </div>
              <div className="rounded-xl bg-white px-5 py-2.5 text-2xl font-bold tracking-[0.35em] text-amber-700 shadow-sm">
                {waitingPairing.code}
              </div>
              <button
                onClick={() => {
                  void api.cancelPairWait(waitingPairing.deviceId);
                  setWaitingPairing(null);
                }}
                className="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
              >
                取消
              </button>
            </div>
          </section>
        )}

        {/* 已配对设备 */}
        <section className="mb-6">
          <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-slate-400">
            已配对设备（{paired.length}）
          </h2>
          {paired.length === 0 && (
            <div className="rounded-xl border border-dashed border-slate-200 bg-white p-4 text-sm text-slate-400">
              还没有配对设备。在下方「发现的设备」中发起配对。
            </div>
          )}
          <div className="flex flex-col gap-2.5">
            {paired.map((d) => (
              <div
                key={d.deviceId}
                className="flex items-center gap-4 rounded-xl border border-slate-200 bg-white p-4"
              >
                <div className="flex items-center gap-2">
                  <span
                    className={`inline-block h-2.5 w-2.5 rounded-full ${
                      d.online ? "bg-emerald-500" : "bg-slate-300"
                    }`}
                  />
                  <div>
                    <div className="text-sm font-medium">{d.name}</div>
                    <div className="mt-0.5 font-mono text-xs text-slate-400">
                      {d.deviceId}
                      {d.address ? ` · ${d.address}` : ""}
                      {d.lastSeenAt ? ` · 上次在线 ${formatTime(d.lastSeenAt)}` : ""}
                    </div>
                  </div>
                </div>
                <div className="ml-auto flex items-center gap-5">
                  <Toggle
                    label="发送"
                    checked={d.sendEnabled}
                    disabled={!d.online}
                    onChange={(v) => void api.setDeviceFlags(d.deviceId, v, undefined)}
                  />
                  <Toggle
                    label="接收"
                    checked={d.receiveEnabled}
                    disabled={!d.online}
                    onChange={(v) => void api.setDeviceFlags(d.deviceId, undefined, v)}
                  />
                  <button
                    title="删除配对"
                    onClick={() => void api.removeDevice(d.deviceId)}
                    className="rounded-lg border border-slate-200 p-2 text-slate-400 hover:bg-rose-50 hover:text-rose-600"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>

        {/* 发现的未配对设备 */}
        <section>
          <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-slate-400">
            发现的设备（{discovered.length}）
          </h2>
          {discovered.length === 0 && (
            <div className="rounded-xl border border-dashed border-slate-200 bg-white p-4 text-sm text-slate-400">
              暂未发现局域网内其他 LanClip 设备。请确认对方已启动 LanClip 且在同一网络。
            </div>
          )}
          <div className="flex flex-col gap-2.5">
            {discovered.map((d) => (
              <div
                key={d.deviceId}
                className="flex items-center gap-4 rounded-xl border border-slate-200 bg-white p-4"
              >
                <Unlink size={18} className="text-slate-300" />
                <div>
                  <div className="text-sm font-medium">{d.name}</div>
                  <div className="mt-0.5 font-mono text-xs text-slate-400">
                    {d.deviceId} · {d.address}
                  </div>
                </div>
                <button
                  onClick={() => void requestPair(d.deviceId)}
                  disabled={pairing === d.deviceId}
                  className="ml-auto flex items-center gap-1.5 rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500 disabled:opacity-50"
                >
                  {pairing === d.deviceId ? (
                    <Loader2 size={14} className="animate-spin" />
                  ) : (
                    <Link size={14} />
                  )}
                  请求配对
                </button>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function Toggle({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className={`flex items-center gap-2 text-xs text-slate-500 ${disabled ? "opacity-50" : ""}`}>
      {label}
      <button
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative h-5 w-9 rounded-full transition-colors ${
          checked ? "bg-indigo-600" : "bg-slate-300"
        }`}
      >
        <span
          className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${
            checked ? "left-[18px]" : "left-0.5"
          }`}
        />
      </button>
    </label>
  );
}
