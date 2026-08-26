import { Search, ShieldAlert } from "lucide-react";
import { fmtTime, useApp } from "../store";
import { Avatar, StatusDot } from "./ui";

export default function ListPane() {
  const tab = useApp((s) => s.tab);
  const setTab = useApp((s) => s.setTab);
  const search = useApp((s) => s.search);
  const setSearch = useApp((s) => s.setSearch);
  const conversations = useApp((s) => s.conversations);
  const peers = useApp((s) => s.peers);
  const activeFp = useApp((s) => s.activeFp);
  const setActive = useApp((s) => s.setActive);
  const sessionOnline = useApp((s) => s.sessionOnline);

  const q = search.trim().toLowerCase();

  return (
    <div
      className="w-[300px] h-full flex flex-col shrink-0"
      style={{ background: "var(--panel)", borderRight: "1px solid var(--line)" }}
    >
      <div className="px-4 pt-4 pb-2">
        <div
          className="flex items-center gap-2 px-3 h-9 rounded-lg"
          style={{ background: "var(--panel2)", border: "1px solid var(--line)" }}
        >
          <Search size={15} style={{ color: "var(--sub)" }} />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索昵称或消息"
            className="bg-transparent flex-1 text-sm"
            style={{ color: "var(--txt)" }}
          />
        </div>
        <div className="flex gap-1 mt-3">
          {(
            [
              ["chats", "聊天"],
              ["nearby", `附近 (${peers.filter((p) => p.online).length})`],
            ] as const
          ).map(([k, label]) => (
            <button
              key={k}
              onClick={() => setTab(k)}
              className="px-3 h-8 rounded-lg text-sm font-medium transition-colors"
              style={{
                background: tab === k ? "var(--acc-weak)" : "transparent",
                color: tab === k ? "var(--acc)" : "var(--sub)",
              }}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-2 pb-3">
        {tab === "chats" ? (
          <>
            {conversations.length === 0 && (
              <Empty text={"还没有会话\n到「附近」发现局域网内的小伙伴吧"} />
            )}
            {conversations
              .filter(
                (c) =>
                  !q ||
                  c.nick.toLowerCase().includes(q) ||
                  c.preview.toLowerCase().includes(q)
              )
              .map((c) => {
                const online =
                  peers.find((p) => p.fp === c.fp)?.online ?? false;
                return (
                  <Row
                    key={c.fp}
                    fp={c.fp}
                    nick={c.nick}
                    sub={c.preview || "…"}
                    time={c.lastTs ? fmtTime(c.lastTs) : ""}
                    online={online}
                    unread={c.unread}
                    confirmed={c.confirmed}
                    active={activeFp === c.fp}
                    live={sessionOnline[c.fp]}
                    onClick={() => setActive(c.fp)}
                  />
                );
              })}
          </>
        ) : (
          <>
            {peers.length === 0 && <Empty text="正在搜索局域网内的好友…" />}
            {peers
              .filter((p) => !q || p.nick.toLowerCase().includes(q))
              .map((p) => {
                const conv = conversations.find((c) => c.fp === p.fp);
                return (
                  <Row
                    key={p.fp}
                    fp={p.fp}
                    nick={p.nick}
                    sub={p.online ? `在线 · ${p.ip}` : `离线 · ${p.ip}`}
                    time=""
                    online={p.online}
                    unread={conv?.unread ?? 0}
                    confirmed={p.confirmed}
                    active={activeFp === p.fp}
                    live={sessionOnline[p.fp]}
                    onClick={() => setActive(p.fp)}
                  />
                );
              })}
          </>
        )}
      </div>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div
      className="text-center text-sm mt-16 px-6 whitespace-pre-line leading-6"
      style={{ color: "var(--sub)" }}
    >
      {text}
    </div>
  );
}

function Row(props: {
  fp: string;
  nick: string;
  sub: string;
  time: string;
  online: boolean;
  unread: number;
  confirmed: boolean;
  active: boolean;
  live?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={props.onClick}
      className="w-full flex items-center gap-3 px-3 py-2.5 rounded-xl transition-colors text-left"
      style={{
        background: props.active ? "var(--acc-weak)" : "transparent",
      }}
      onMouseEnter={(e) => {
        if (!props.active) e.currentTarget.style.background = "var(--panel2)";
      }}
      onMouseLeave={(e) => {
        if (!props.active) e.currentTarget.style.background = props.active ? "var(--acc-weak)" : "transparent";
      }}
    >
      <div className="relative">
        <Avatar fp={props.fp} nick={props.nick} size={42} />
        <span className="absolute bottom-0 right-0">
          <StatusDot on={props.online} />
        </span>
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="font-medium truncate" style={{ color: "var(--txt)" }}>
            {props.nick}
          </span>
          {!props.confirmed && (
            <span title="指纹未确认">
              <ShieldAlert size={13} style={{ color: "var(--warn)" }} />
            </span>
          )}
          {props.live && (
            <span
              className="text-[10px] px-1 rounded"
              style={{ background: "var(--acc-weak)", color: "var(--acc)" }}
            >
              已连接
            </span>
          )}
        </div>
        <div className="text-xs truncate mt-0.5" style={{ color: "var(--sub)" }}>
          {props.sub}
        </div>
      </div>
      <div className="flex flex-col items-end gap-1 shrink-0">
        <span className="text-[11px]" style={{ color: "var(--sub)" }}>
          {props.time}
        </span>
        {props.unread > 0 && (
          <span
            className="min-w-[18px] h-[18px] px-1 rounded-full text-[10px] leading-[18px] text-white font-semibold text-center"
            style={{ background: "var(--danger)" }}
          >
            {props.unread > 99 ? "99+" : props.unread}
          </span>
        )}
      </div>
    </button>
  );
}
