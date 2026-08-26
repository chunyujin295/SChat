import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { api } from "../api";
import { useApp } from "../store";

const avatarCache = new Map<string, string>();

export function bustAvatarCache() {
  avatarCache.clear();
}

export function Avatar({
  fp,
  nick,
  ver,
  size = 40,
}: {
  fp: string;
  nick: string;
  ver?: number;
  size?: number;
}) {
  const key = `${fp}:${ver ?? 0}`;
  const [url, setUrl] = useState<string | null>(avatarCache.get(key) ?? null);
  useEffect(() => {
    let alive = true;
    if (!avatarCache.has(key)) {
      api.getAvatar(fp).then((d) => {
        if (d && alive) {
          avatarCache.set(key, d);
          setUrl(d);
        }
      });
    }
    return () => {
      alive = false;
    };
  }, [key, fp]);
  const hue =
    [...fp].reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
  return (
    <div
      className="rounded-full overflow-hidden shrink-0 flex items-center justify-center font-semibold select-none"
      style={{
        width: size,
        height: size,
        background: url ? undefined : `hsl(${hue} 42% 40%)`,
        fontSize: size * 0.4,
      }}
    >
      {url ? (
        <img src={url} className="w-full h-full object-cover" draggable={false} alt="" />
      ) : (
        <span className="text-white">{(nick || "?").slice(0, 1)}</span>
      )}
    </div>
  );
}

export function StatusDot({ on }: { on: boolean }) {
  return (
    <span
      className="inline-block w-2 h-2 rounded-full shrink-0"
      style={{ background: on ? "var(--ok)" : "var(--sub)" }}
    />
  );
}

export function Toaster() {
  const toasts = useApp((s) => s.toasts);
  const drop = useApp((s) => s.dropToast);
  return (
    <div className="fixed bottom-5 right-5 z-[90] flex flex-col gap-2 items-end">
      {toasts.map((t) => (
        <div
          key={t.id}
          onClick={() => drop(t.id)}
          className="cursor-pointer max-w-sm px-4 py-2.5 rounded-xl shadow-lg text-sm border"
          style={{
            background: "var(--panel2)",
            borderColor:
              t.kind === "err"
                ? "var(--danger)"
                : t.kind === "ok"
                  ? "var(--ok)"
                  : "var(--line)",
            color: "var(--txt)",
          }}
        >
          {t.text}
        </div>
      ))}
    </div>
  );
}

export function Lightbox() {
  const url = useApp((s) => s.lightbox);
  const close = useApp((s) => s.setLightbox);
  if (!url) return null;
  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={() => close(null)}
    >
      <img src={url} className="max-w-[90vw] max-h-[88vh] rounded-lg shadow-2xl" alt="" />
      <button
        className="absolute top-5 right-5 p-2 rounded-full bg-white/10 hover:bg-white/20 text-white"
        onClick={() => close(null)}
      >
        <X size={22} />
      </button>
    </div>
  );
}
