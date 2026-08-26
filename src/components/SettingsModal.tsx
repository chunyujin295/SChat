import { useRef, useState } from "react";
import { Keyboard, LogOut, Palette, ShieldCheck, UserRound, X } from "lucide-react";
import { api } from "../api";
import { bustAvatarCache, Avatar } from "./ui";
import { useApp } from "../store";

export default function SettingsModal() {
  const open = useApp((s) => s.settingsOpen);
  const setOpen = useApp((s) => s.setSettingsOpen);
  const profile = useApp((s) => s.profile);
  const config = useApp((s) => s.config);
  if (!open || !profile || !config) return null;
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center"
      style={{ background: "rgba(8,10,14,0.55)", backdropFilter: "blur(3px)" }}
      onClick={() => setOpen(false)}
    >
      <div
        className="w-[600px] max-w-[92vw] max-h-[84vh] overflow-y-auto rounded-2xl shadow-2xl"
        style={{ background: "var(--panel)", border: "1px solid var(--line)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <Header onClose={() => setOpen(false)} />
        <ProfileSection />
        <AppearanceSection />
        <HotkeySection />
        <PrivacySection />
        <AboutSection />
      </div>
    </div>
  );
}

function Header({ onClose }: { onClose: () => void }) {
  return (
    <div
      className="sticky top-0 z-10 flex items-center justify-between px-6 h-14"
      style={{ background: "var(--panel)", borderBottom: "1px solid var(--line)" }}
    >
      <span className="font-semibold" style={{ color: "var(--txt)" }}>
        设置
      </span>
      <IconX onClick={onClose} />
    </div>
  );
}

function IconX({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="w-8 h-8 rounded-lg flex items-center justify-center"
      style={{ color: "var(--sub)" }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--panel2)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <X size={18} />
    </button>
  );
}

function Section({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="px-6 py-5" style={{ borderBottom: "1px solid var(--line)" }}>
      <div className="flex items-center gap-2 mb-4 text-sm font-semibold" style={{ color: "var(--txt)" }}>
        {icon}
        {title}
      </div>
      {children}
    </div>
  );
}

function ProfileSection() {
  const profile = useApp((s) => s.profile)!;
  const toast = useApp((s) => s.toast);
  const [nick, setNick] = useState(profile.nickname);
  const [busy, setBusy] = useState(false);
  const fileRef = useRef<HTMLInputElement | null>(null);

  const saveNick = async () => {
    setBusy(true);
    try {
      const p = await api.setProfile(nick);
      useApp.setState({ profile: p });
      toast("资料已更新", "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const pickAvatar = async (f: File) => {
    try {
      const dataUrl = await cropToSquare(f);
      const p = await api.setProfile(nick || profile.nickname, dataUrl);
      bustAvatarCache();
      useApp.setState({ profile: p });
      toast("头像已更新", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  };

  return (
    <Section icon={<UserRound size={16} />} title="我的资料">
      <div className="flex items-center gap-5">
        <button
          className="relative group"
          title="点击更换头像"
          onClick={() => fileRef.current?.click()}
        >
          <Avatar fp="self" nick={nick} ver={profile.avaVer} size={64} />
          <span
            className="absolute inset-0 rounded-full items-center justify-center text-[11px] text-white hidden group-hover:flex"
            style={{ background: "rgba(0,0,0,0.45)" }}
          >
            更换
          </span>
        </button>
        <input
          ref={fileRef}
          type="file"
          accept="image/png,image/jpeg"
          className="hidden"
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) void pickAvatar(f);
            e.target.value = "";
          }}
        />
        <div className="flex-1">
          <label className="text-xs block mb-1.5" style={{ color: "var(--sub)" }}>
            昵称（最多 24 字符）
          </label>
          <div className="flex gap-2">
            <input
              value={nick}
              maxLength={24}
              onChange={(e) => setNick(e.target.value)}
              className="flex-1 h-9 px-3 rounded-lg text-sm"
              style={{ background: "var(--panel2)", border: "1px solid var(--line)", color: "var(--txt)" }}
            />
            <button
              disabled={busy || nick.trim() === profile.nickname}
              onClick={() => void saveNick()}
              className="px-4 h-9 rounded-lg text-sm text-white disabled:opacity-40"
              style={{ background: "var(--acc)" }}
            >
              保存
            </button>
          </div>
        </div>
      </div>
    </Section>
  );
}

async function cropToSquare(file: File): Promise<string> {
  const url = URL.createObjectURL(file);
  try {
    const img = await new Promise<HTMLImageElement>((res, rej) => {
      const im = new Image();
      im.onload = () => res(im);
      im.onerror = () => rej(new Error("图片读取失败"));
      im.src = url;
    });
    const size = 256;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d")!;
    const side = Math.min(img.width, img.height);
    ctx.drawImage(
      img,
      (img.width - side) / 2,
      (img.height - side) / 2,
      side,
      side,
      0,
      0,
      size,
      size
    );
    let out = canvas.toDataURL("image/jpeg", 0.9);
    if (out.length > 512 * 1024 * 1.34) out = canvas.toDataURL("image/jpeg", 0.72);
    return out;
  } finally {
    URL.revokeObjectURL(url);
  }
}

function AppearanceSection() {
  const config = useApp((s) => s.config)!;
  const patchLocal = useApp((s) => s.patchConfigLocal);
  const toast = useApp((s) => s.toast);
  const setTheme = async (t: string) => {
    patchLocal({ theme: t });
    try {
      await api.setSettings({ theme: t });
    } catch (e) {
      toast(String(e), "err");
    }
  };
  return (
    <Section icon={<Palette size={16} />} title="外观">
      <div className="flex gap-2">
        {(
          [
            ["dark", "深色"],
            ["light", "浅色"],
          ] as const
        ).map(([k, label]) => (
          <button
            key={k}
            onClick={() => void setTheme(k)}
            className="px-4 h-9 rounded-lg text-sm font-medium"
            style={{
              background: config.theme === k ? "var(--acc-weak)" : "var(--panel2)",
              color: config.theme === k ? "var(--acc)" : "var(--sub)",
              border: `1px solid ${config.theme === k ? "var(--acc)" : "var(--line)"}`,
            }}
          >
            {label}
          </button>
        ))}
        <div className="flex-1" />
        <label className="flex items-center gap-2 text-sm cursor-pointer" style={{ color: "var(--sub)" }}>
          <input
            type="checkbox"
            checked={config.closeToTray}
            onChange={async (e) => {
              patchLocal({ closeToTray: e.target.checked });
              await api.setSettings({ closeToTray: e.target.checked });
            }}
          />
          关闭窗口时最小化到托盘
        </label>
      </div>
    </Section>
  );
}

const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta"]);

function HotkeySection() {
  const config = useApp((s) => s.config)!;
  const patchLocal = useApp((s) => s.patchConfigLocal);
  const toast = useApp((s) => s.toast);
  const [combo, setCombo] = useState(config.hotkey);
  const [recording, setRecording] = useState(false);

  const capture = (e: React.KeyboardEvent) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (MODIFIER_KEYS.has(e.key)) return;
    if (e.key === "Escape") {
      setCombo(config.hotkey);
      setRecording(false);
      return;
    }
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("ctrl");
    if (e.altKey) parts.push("alt");
    if (e.shiftKey) parts.push("shift");
    if (e.metaKey) parts.push("super");
    let key = e.key.length === 1 ? e.key.toLowerCase() : e.key.replace(/^Key|^Digit/, "").toLowerCase();
    if (!key) return;
    parts.push(key);
    setCombo(parts.join("+"));
    setRecording(false);
  };

  const save = async () => {
    try {
      await api.setSettings({ hotkey: combo });
      patchLocal({ hotkey: combo });
      toast("快捷键已生效", "ok");
    } catch (e) {
      setCombo(config.hotkey);
      toast(String(e), "err");
    }
  };

  return (
    <Section icon={<Keyboard size={16} />} title="全局快捷键">
      <div className="flex items-center gap-3">
        <div
          tabIndex={0}
          onKeyDown={capture}
          onClick={() => setRecording(true)}
          className="h-10 min-w-[160px] px-4 rounded-lg flex items-center justify-center cursor-pointer text-sm font-mono tracking-wide select-none"
          style={{
            background: "var(--panel2)",
            border: recording ? "1px solid var(--acc)" : "1px solid var(--line)",
            color: "var(--txt)",
          }}
        >
          {recording ? (
            <span style={{ color: "var(--acc)" }}>按下组合键…（Esc 取消）</span>
          ) : (
            combo.toUpperCase()
          )}
        </div>
        <button
          disabled={recording || combo === config.hotkey}
          onClick={() => void save()}
          className="px-4 h-10 rounded-lg text-sm text-white disabled:opacity-40"
          style={{ background: "var(--acc)" }}
        >
          应用
        </button>
        <span className="text-xs" style={{ color: "var(--sub)" }}>
          用于显示 / 隐藏主窗口
        </span>
      </div>
    </Section>
  );
}

function PrivacySection() {
  const profile = useApp((s) => s.profile)!;
  const activeFp = useApp((s) => s.activeFp);
  const setActive = useApp((s) => s.setActive);
  const toast = useApp((s) => s.toast);
  return (
    <Section icon={<ShieldCheck size={16} />} title="隐私与数据">
      <div className="text-sm space-y-3" style={{ color: "var(--sub)" }}>
        <div>
          我的设备指纹：
          <span className="font-mono ml-1" style={{ color: "var(--txt)" }}>
            {profile.fpDisplay}
          </span>
        </div>
        <p className="text-xs leading-5">
          SChat 无账号、无服务器、无遥测。消息经端到端加密仅在局域网内传输；
          聊天记录加密存储在本机，密钥由 Windows 账户保护。
          请与好友当面核对指纹以防止中间人冒充。
        </p>
        <div className="flex gap-2 pt-1">
          <button
            className="px-3.5 h-9 rounded-lg text-sm"
            style={{ background: "var(--panel2)", color: "var(--danger)", border: "1px solid var(--line)" }}
            onClick={async () => {
              if (!activeFp) return toast("先选择一个会话", "err");
              if (!window.confirm("清空当前会话的聊天记录？")) return;
              await api.clearHistory(activeFp);
              useApp.setState({
                messages: { ...useApp.getState().messages, [activeFp]: [] },
                conversations: useApp.getState().conversations.map((c) =>
                  c.fp === activeFp ? { ...c, preview: "", lastTs: 0 } : c
                ),
              });
              await setActive(activeFp);
              toast("已清空当前会话记录", "ok");
            }}
          >
            清空当前会话记录
          </button>
          <button
            className="px-3.5 h-9 rounded-lg text-sm"
            style={{ background: "var(--panel2)", color: "var(--danger)", border: "1px solid var(--line)" }}
            onClick={async () => {
              if (!window.confirm("清空全部聊天记录？此操作不可恢复。")) return;
              await api.clearHistory(null);
              useApp.setState({ messages: {}, loaded: {}, conversations: [] });
              toast("已清空全部记录", "ok");
            }}
          >
            清空全部记录
          </button>
        </div>
      </div>
    </Section>
  );
}

function AboutSection() {
  const toast = useApp((s) => s.toast);
  const profile = useApp((s) => s.profile);
  return (
    <Section icon={<LogOut size={16} />} title="关于">
      <div className="text-sm space-y-3" style={{ color: "var(--sub)" }}>
        <div>SChat v0.1.0 · 局域网端到端加密通讯</div>
        <div className="flex gap-2 pt-1">
          <button
            className="px-3.5 h-9 rounded-lg text-sm"
            style={{ background: "var(--panel2)", color: "var(--txt)", border: "1px solid var(--line)" }}
            onClick={() => {
              if (profile?.fpDisplay) {
                navigator.clipboard.writeText(profile.fp).then(() => toast("指纹已复制", "ok"));
              }
            }}
          >
            复制我的指纹
          </button>
          <button
            className="px-3.5 h-9 rounded-lg text-sm"
            style={{ background: "var(--panel2)", color: "var(--danger)", border: "1px solid var(--line)" }}
            onClick={() => void api.quitApp()}
          >
            退出 SChat
          </button>
        </div>
      </div>
    </Section>
  );
}
