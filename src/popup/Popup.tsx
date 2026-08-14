import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check, ClipboardList, Code2, Link2, Type, X } from "lucide-react";
import { api } from "../lib/ipc";
import type { HistoryItemView, IncomingPayload } from "../lib/types";

interface Card {
  item: HistoryItemView;
  previewAllowed: boolean;
}

const AUTO_HIDE_MS = 8000;

export default function Popup() {
  const [cards, setCards] = useState<Card[]>([]);

  useEffect(() => {
    const un = listen<IncomingPayload>("lanclip://incoming", (e) => {
      const { item, previewAllowed } = e.payload;
      setCards((prev) => {
        if (prev.some((c) => c.item.id === item.id)) return prev;
        const next = [...prev, { item, previewAllowed }];
        // 最多同时展示 3 张，其余进历史
        return next.slice(-3);
      });
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  // 全部处理完（接收/忽略/超时）后收起窗口
  useEffect(() => {
    if (cards.length === 0) {
      const t = setTimeout(() => void api.hidePopup(), 150);
      return () => clearTimeout(t);
    }
  }, [cards]);

  return (
    <div className="flex h-full flex-col items-end gap-2.5 p-3">
      {cards.map((c) => (
        <CardView
          key={c.item.id}
          card={c}
          onDone={() => setCards((prev) => prev.filter((x) => x.item.id !== c.item.id))}
        />
      ))}
    </div>
  );
}

function CardView({ card, onDone }: { card: Card; onDone: () => void }) {
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    const t = setTimeout(() => onDone(), AUTO_HIDE_MS);
    return () => clearTimeout(t);
  }, [onDone]);

  const finish = (action: "accept" | "ignore") => {
    if (closing) return;
    setClosing(true);
    const p =
      action === "accept" ? api.acceptItem(card.item.id) : api.ignoreItem(card.item.id);
    p.catch(() => {}).finally(() => onDone());
  };

  const { item, previewAllowed } = card;
  const preview = previewAllowed
    ? item.preview || "（无文本预览）"
    : `来自 ${item.sourceDeviceName} 的${typeLabel(item.contentType)}内容`;

  return (
    <div className="w-[376px] overflow-hidden rounded-2xl bg-white/95 shadow-2xl ring-1 ring-black/5 backdrop-blur">
      <div className="flex items-center gap-2 px-4 pt-3">
        {typeIcon(item.contentType)}
        <span className="text-xs font-medium text-slate-700">{item.sourceDeviceName}</span>
        <span className="text-xs text-slate-400">发来了一段{typeLabel(item.contentType)}</span>
        <button
          onClick={() => finish("ignore")}
          className="ml-auto rounded p-1 text-slate-300 hover:bg-slate-100 hover:text-slate-500"
        >
          <X size={13} />
        </button>
      </div>
      <p className="mt-1.5 line-clamp-2 px-4 text-sm text-slate-600">{preview}</p>
      <div className="mt-2 flex justify-end gap-2 px-4 pb-3">
        <button
          onClick={() => finish("ignore")}
          className="rounded-lg border border-slate-200 px-3 py-1.5 text-xs text-slate-500 hover:bg-slate-100"
        >
          忽略
        </button>
        <button
          onClick={() => finish("accept")}
          className="flex items-center gap-1 rounded-lg bg-indigo-600 px-3.5 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
        >
          <Check size={13} /> 接收到本机
        </button>
      </div>
      <div className="h-[3px] w-full bg-slate-100">
        <div className="popup-countdown h-full bg-indigo-500" />
      </div>
    </div>
  );
}

function typeLabel(t: string): string {
  if (t === "url") return "链接";
  if (t === "html") return "HTML";
  return "文本";
}

function typeIcon(t: string) {
  if (t === "url") return <Link2 size={15} className="text-sky-600" />;
  if (t === "html") return <Code2 size={15} className="text-violet-600" />;
  if (t === "text") return <Type size={15} className="text-indigo-600" />;
  return <ClipboardList size={15} className="text-slate-400" />;
}
