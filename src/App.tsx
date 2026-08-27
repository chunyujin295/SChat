import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api } from "./api";
import ChatPane from "./components/ChatPane";
import ListPane from "./components/ListPane";
import NavRail from "./components/NavRail";
import SettingsModal from "./components/SettingsModal";
import { Avatar, Lightbox, Toaster } from "./components/ui";
import { useApp } from "./store";

export default function App() {
  const ready = useApp((s) => s.ready);
  const init = useApp((s) => s.init);
  const wireEvents = useApp((s) => s.wireEvents);

  useEffect(() => {
    void init();
    return wireEvents();
  }, [init, wireEvents]);

  useEffect(() => {
    const un = getCurrentWebviewWindow().onDragDropEvent((e) => {
      const st = useApp.getState();
      if (e.payload.type === "enter" || e.payload.type === "over") {
        st.setDragOver(true);
      } else if (e.payload.type === "drop") {
        st.setDragOver(false);
        const fp = st.activeFp;
        if (!fp) {
          st.toast("请先在左侧选择一个联系人", "err");
          return;
        }
        void api.sendFiles(fp, e.payload.paths).then((results) => {
          for (const r of results) {
            if (r && typeof r.error === "string") st.toast(r.error, "err");
          }
        });
      } else {
        st.setDragOver(false);
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        const s = useApp.getState();
        if (s.lightbox) s.setLightbox(null);
        else if (s.settingsOpen) s.setSettingsOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    const markRead = () => useApp.getState().markActiveRead();
    const onVisibility = () => {
      if (document.visibilityState === "visible") markRead();
    };
    window.addEventListener("focus", markRead);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("focus", markRead);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  if (!ready) {
    return (
      <div className="h-full flex items-center justify-center" style={{ background: "var(--bg)" }}>
        <div className="flex flex-col items-center gap-3">
          <div
            className="w-14 h-14 rounded-2xl flex items-center justify-center font-black text-white"
            style={{ background: "linear-gradient(135deg,#5f92ff,#3a58cd)", fontSize: 26 }}
          >
            S
          </div>
          <span className="text-sm" style={{ color: "var(--sub)" }}>
            正在启动…
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex overflow-hidden">
      <NavRail />
      <ListPane />
      <ChatPane />
      <SettingsModal />
      <Lightbox />
      <Toaster />
      <Onboarding />
      <DragOverlay />
    </div>
  );
}

function Onboarding() {
  const config = useApp((s) => s.config);
  const profile = useApp((s) => s.profile);
  const toast = useApp((s) => s.toast);
  const patchLocal = useApp((s) => s.patchConfigLocal);
  const [nick, setNick] = useState(profile?.nickname ?? "");
  const [busy, setBusy] = useState(false);
  if (!config || config.onboarded) return null;

  const finish = async () => {
    setBusy(true);
    try {
      const p = await api.setProfile(nick.trim() || profile?.nickname || "匿名用户");
      useApp.setState({ profile: p });
      const cfg = await api.setSettings({});
      patchLocal(cfg as never);
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[70] flex items-center justify-center"
      style={{ background: "rgba(8,10,14,0.6)", backdropFilter: "blur(4px)" }}
    >
      <div
        className="w-[440px] rounded-2xl p-8 shadow-2xl text-center"
        style={{ background: "var(--panel)", border: "1px solid var(--line)" }}
      >
        <div
          className="w-16 h-16 mx-auto rounded-2xl flex items-center justify-center font-black text-white mb-4"
          style={{ background: "linear-gradient(135deg,#5f92ff,#3a58cd)", fontSize: 30 }}
        >
          S
        </div>
        <h1 className="text-lg font-bold mb-1" style={{ color: "var(--txt)" }}>
          欢迎使用 SChat
        </h1>
        <p className="text-xs leading-5 mb-6" style={{ color: "var(--sub)" }}>
          免登录 · 端到端加密 · 局域网内可发现
          <br />
          你的身份由本机密钥生成，无需注册任何账号
        </p>
        <input
          value={nick}
          maxLength={24}
          placeholder="给自己起个昵称"
          onChange={(e) => setNick(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void finish()}
          className="w-full h-11 px-4 rounded-xl text-sm text-center mb-3"
          style={{ background: "var(--panel2)", border: "1px solid var(--line)", color: "var(--txt)" }}
        />
        <button
          disabled={busy}
          onClick={() => void finish()}
          className="w-full h-11 rounded-xl text-white font-medium disabled:opacity-50"
          style={{ background: "var(--acc)" }}
        >
          开始使用
        </button>
        <p className="text-[11px] mt-4" style={{ color: "var(--sub)" }}>
          默认全局快捷键 Ctrl+Alt+S 可随时显示 / 隐藏窗口，稍后可在设置中修改
        </p>
        <div className="mt-4 flex justify-center">
          {profile && <Avatar fp="self" nick={nick || profile.nickname} ver={profile.avaVer} size={40} />}
        </div>
      </div>
    </div>
  );
}

function DragOverlay() {
  const dragOver = useApp((s) => s.dragOver);
  if (!dragOver) return null;
  return (
    <div
      className="fixed inset-0 z-[75] flex items-center justify-center pointer-events-none m-4 rounded-2xl border-2 border-dashed"
      style={{ borderColor: "var(--acc)", background: "rgba(79,140,255,0.08)" }}
    >
      <span className="text-base font-medium px-4 py-2 rounded-xl" style={{ background: "var(--panel)", color: "var(--txt)" }}>
        松开即发送文件到当前会话
      </span>
    </div>
  );
}
