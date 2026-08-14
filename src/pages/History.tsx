import { useEffect, useState } from "react";
import { Check, ClipboardCopy, Eye, Link2, Trash2, Type, X, Code2 } from "lucide-react";
import { api } from "../lib/ipc";
import { useApp } from "../stores/app";
import { formatSize, formatTime, type ContentView, type HistoryItemView } from "../lib/types";

const STATUS_META: Record<string, { label: string; cls: string }> = {
  pending: { label: "待处理", cls: "bg-amber-100 text-amber-700" },
  accepted: { label: "已接收", cls: "bg-emerald-100 text-emerald-700" },
  ignored: { label: "已忽略", cls: "bg-slate-200 text-slate-500" },
  expired: { label: "已过期", cls: "bg-slate-200 text-slate-400" },
};

function TypeIcon({ type }: { type: string }) {
  if (type === "url") return <Link2 size={16} className="text-sky-600" />;
  if (type === "html") return <Code2 size={16} className="text-violet-600" />;
  return <Type size={16} className="text-indigo-600" />;
}

const TYPE_LABEL: Record<string, string> = { text: "文本", html: "HTML", url: "链接" };

export default function HistoryPage() {
  const history = useApp((s) => s.history);
  const filter = useApp((s) => s.filter);
  const search = useApp((s) => s.search);
  const setFilter = useApp((s) => s.setFilter);
  const setSearch = useApp((s) => s.setSearch);
  const refreshHistory = useApp((s) => s.refreshHistory);
  const toast = useApp((s) => s.toast);
  const [preview, setPreview] = useState<{ id: string; content: ContentView } | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    void refreshHistory();
  }, [search, filter, refreshHistory]);

  const accept = async (item: HistoryItemView) => {
    try {
      await api.acceptItem(item.id);
      toast("ok", "已复制到本机剪切板");
      void refreshHistory();
    } catch (e) {
      toast("err", `接收失败: ${e}`);
    }
  };

  const filters: { key: typeof filter; label: string }[] = [
    { key: "all", label: "全部" },
    { key: "pending", label: "待处理" },
    { key: "accepted", label: "已接收" },
    { key: "ignored", label: "已忽略" },
  ];

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-slate-200 bg-white px-6 py-4">
        <h1 className="text-base font-semibold">历史记录</h1>
        <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs text-slate-500">
          {history.length}
        </span>
        <div className="ml-auto flex items-center gap-2">
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索内容 / 设备名…"
            className="w-56 rounded-lg border border-slate-200 px-3 py-1.5 text-sm outline-none focus:border-indigo-400 focus:ring-2 focus:ring-indigo-100"
          />
          <button
            onClick={() => {
              if (!confirmClear) {
                setConfirmClear(true);
                setTimeout(() => setConfirmClear(false), 3000);
                return;
              }
              setConfirmClear(false);
              void api.clearHistory();
              toast("ok", "历史已清空");
            }}
            className={`rounded-lg px-3 py-1.5 text-sm transition-colors ${
              confirmClear
                ? "bg-rose-600 text-white"
                : "border border-slate-200 text-slate-600 hover:bg-slate-100"
            }`}
          >
            {confirmClear ? "确认清空？" : "清空历史"}
          </button>
        </div>
      </header>

      <div className="flex gap-2 px-6 pt-4">
        {filters.map((f) => (
          <button
            key={f.key}
            onClick={() => setFilter(f.key)}
            className={`rounded-full px-3 py-1 text-xs transition-colors ${
              filter === f.key
                ? "bg-indigo-600 text-white"
                : "bg-white text-slate-600 ring-1 ring-slate-200 hover:bg-slate-100"
            }`}
          >
            {f.label}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        {history.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-slate-400">
            <ClipboardCopy size={36} strokeWidth={1.5} />
            <p className="text-sm">暂无记录：在局域网内其他已配对设备上复制内容即可收到</p>
          </div>
        )}
        <div className="flex flex-col gap-2.5">
          {history.map((item) => (
            <div
              key={item.id}
              className="group cursor-pointer rounded-xl border border-slate-200 bg-white p-4 transition-shadow hover:shadow-md"
              onClick={() => {
                api
                  .getItemContent(item.id)
                  .then((content) => setPreview({ id: item.id, content }))
                  .catch((e) => toast("err", `读取内容失败: ${e}`));
              }}
            >
              <div className="flex items-start gap-3">
                <div className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-slate-100">
                  <TypeIcon type={item.contentType} />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="line-clamp-2 break-all text-sm text-slate-700">{item.preview}</p>
                  <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-slate-400">
                    <span className="font-medium text-slate-500">{item.sourceDeviceName}</span>
                    <span>·</span>
                    <span>{formatTime(item.createdAt)}</span>
                    <span>·</span>
                    <span>{TYPE_LABEL[item.contentType] ?? item.contentType}</span>
                    <span>·</span>
                    <span>{formatSize(item.contentSize)}</span>
                    {!item.persistent && (
                      <span className="rounded bg-slate-100 px-1.5 py-0.5">不保存模式</span>
                    )}
                    <span
                      className={`rounded px-1.5 py-0.5 ${STATUS_META[item.status]?.cls ?? ""}`}
                    >
                      {STATUS_META[item.status]?.label ?? item.status}
                    </span>
                  </div>
                </div>
                <div
                  className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100"
                  onClick={(e) => e.stopPropagation()}
                >
                  {item.status !== "accepted" && (
                    <button
                      title="接收（复制到本机剪切板）"
                      onClick={() => void accept(item)}
                      className="flex items-center gap-1 rounded-lg bg-indigo-600 px-2.5 py-1.5 text-xs text-white hover:bg-indigo-500"
                    >
                      <Check size={13} /> 接收
                    </button>
                  )}
                  {item.status === "pending" && (
                    <button
                      title="忽略（保留在历史中）"
                      onClick={() => void api.ignoreItem(item.id)}
                      className="flex items-center gap-1 rounded-lg border border-slate-200 px-2.5 py-1.5 text-xs text-slate-600 hover:bg-slate-100"
                    >
                      <X size={13} /> 忽略
                    </button>
                  )}
                  <button
                    title="删除"
                    onClick={() => void api.deleteItem(item.id)}
                    className="rounded-lg border border-slate-200 p-1.5 text-slate-400 hover:bg-rose-50 hover:text-rose-600"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* 完整内容预览 */}
      {preview && (
        <div
          className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-8"
          onClick={() => setPreview(null)}
        >
          <div
            className="flex max-h-[75%] w-[640px] max-w-full flex-col rounded-2xl bg-white shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center gap-2 border-b border-slate-100 px-5 py-3.5">
              <Eye size={16} className="text-slate-400" />
              <span className="text-sm font-medium">完整内容</span>
              <button
                onClick={() => void api.acceptItem(preview.id).then(() => toast("ok", "已复制到本机剪切板"))}
                className="ml-auto flex items-center gap-1 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs text-white hover:bg-indigo-500"
              >
                <ClipboardCopy size={13} /> 复制
              </button>
              <button
                onClick={() => setPreview(null)}
                className="rounded-lg border border-slate-200 p-1.5 text-slate-400 hover:bg-slate-100"
              >
                <X size={14} />
              </button>
            </div>
            <pre className="flex-1 overflow-auto whitespace-pre-wrap break-all px-5 py-4 text-sm leading-relaxed text-slate-700">
              {preview.content.text}
            </pre>
          </div>
        </div>
      )}
    </div>
  );
}
