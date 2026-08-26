import { MessageCircle, Settings, Users } from "lucide-react";
import { useApp } from "../store";

export default function NavRail() {
  const tab = useApp((s) => s.tab);
  const setTab = useApp((s) => s.setTab);
  const settingsOpen = useApp((s) => s.settingsOpen);
  const openSettings = useApp((s) => s.setSettingsOpen);
  const convs = useApp((s) => s.conversations);
  const unreadTotal = convs.reduce((a, c) => a + (c.unread > 0 ? 1 : 0), 0);

  const Item = ({
    active,
    badge,
    children,
    onClick,
  }: {
    active: boolean;
    badge?: number;
    children: React.ReactNode;
    onClick: () => void;
  }) => (
    <button
      onClick={onClick}
      className="relative w-11 h-11 rounded-xl flex items-center justify-center transition-colors"
      style={{
        background: active ? "var(--acc-weak)" : "transparent",
        color: active ? "var(--acc)" : "var(--sub)",
      }}
      onMouseEnter={(e) => {
        if (!active) e.currentTarget.style.background = "var(--panel2)";
      }}
      onMouseLeave={(e) => {
        if (!active) e.currentTarget.style.background = "transparent";
      }}
    >
      {children}
      {badge ? (
        <span
          className="absolute -top-1 -right-1 min-w-[18px] h-[18px] px-1 rounded-full text-[10px] leading-[18px] text-white font-semibold"
          style={{ background: "var(--danger)" }}
        >
          {badge > 99 ? "99+" : badge}
        </span>
      ) : null}
    </button>
  );

  return (
    <div
      className="w-16 h-full flex flex-col items-center py-4 gap-3 shrink-0"
      style={{ background: "var(--panel)", borderRight: "1px solid var(--line)" }}
    >
      <div
        className="w-9 h-9 rounded-xl flex items-center justify-center font-black text-white mb-2 select-none"
        style={{ background: "linear-gradient(135deg,#5f92ff,#3a58cd)", fontSize: 18 }}
      >
        S
      </div>
      <Item active={tab === "chats"} badge={unreadTotal} onClick={() => setTab("chats")}>
        <MessageCircle size={22} />
      </Item>
      <Item active={tab === "nearby"} onClick={() => setTab("nearby")}>
        <Users size={22} />
      </Item>
      <div className="flex-1" />
      <Item active={settingsOpen} onClick={() => openSettings(!settingsOpen)}>
        <Settings size={22} />
      </Item>
    </div>
  );
}
