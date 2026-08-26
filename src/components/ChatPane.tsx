import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  Check,
  CheckCheck,
  Clock4,
  Download,
  File as FileIcon,
  FolderOpen,
  ImagePlus,
  Lock,
  Mic,
  Paperclip,
  Phone,
  Play,
  Send,
  ShieldAlert,
  ShieldCheck,
  Smile,
  Square,
  Trash2,
  Video,
  XCircle,
} from "lucide-react";
import { api } from "../api";
import { fmtDay, fmtSize, shortFp, useApp } from "../store";
import type { Message } from "../types";
import { Avatar, StatusDot } from "./ui";

const EMOJIS = [
  "😀","😂","🤣","😊","😍","😘","😜","😎","🤔","😴",
  "😭","😡","👍","👎","👏","🙏","💪","🤝","👌","✌️",
  "❤️","💔","🎉","🔥","⭐","🌹","🍀","🌈","☀️","🌙",
  "🍉","🍎","🍺","☕","🍰","🎁","⚽","🚀","💡","📌",
];

export default function ChatPane() {
  const activeFp = useApp((s) => s.activeFp);
  if (!activeFp) return <EmptyChat />;
  return <Chat key={activeFp} fp={activeFp} />;
}

function EmptyChat() {
  return (
    <div className="flex-1 h-full flex flex-col items-center justify-center gap-3" style={{ color: "var(--sub)" }}>
      <div
        className="w-16 h-16 rounded-2xl flex items-center justify-center font-black text-white"
        style={{ background: "linear-gradient(135deg,#5f92ff,#3a58cd)", fontSize: 32 }}
      >
        S
      </div>
      <div className="text-sm">选择一个联系人开始加密聊天</div>
      <div className="text-xs opacity-60">所有消息仅在局域网内端到端传输</div>
    </div>
  );
}

function Chat({ fp }: { fp: string }) {
  const peer = useApp((s) => s.peers.find((p) => p.fp === fp));
  const msgs = useApp((s) => s.messages[fp]);
  const typingAt = useApp((s) => s.typingUntil[fp]);
  const sessionLive = useApp((s) => s.sessionOnline[fp]);
  const confirmPeer = useApp((s) => s.toast);
  const [, force] = useState(0);

  useEffect(() => {
    const t = setInterval(() => force((n) => n + 1), 1500);
    return () => clearInterval(t);
  }, []);

  if (!msgs) return <EmptyChat />;
  const nick = peer?.nick ?? shortFp(fp);
  const online = peer?.online ?? false;
  const isTyping = typingAt && Date.now() < typingAt;

  return (
    <div className="flex-1 h-full flex flex-col min-w-0" style={{ background: "var(--bg)" }}>
      {/* header */}
      <div
        className="h-[64px] shrink-0 flex items-center gap-3 px-5"
        style={{ borderBottom: "1px solid var(--line)", background: "var(--panel)" }}
      >
        <div className="relative">
          <Avatar fp={fp} nick={nick} ver={peer?.avaVer} size={40} />
        </div>
        <div className="min-w-0">
          <div className="font-semibold truncate flex items-center gap-1.5" style={{ color: "var(--txt)" }}>
            {nick}
            {peer?.confirmed && (
              <span title={`指纹已核对：${shortFp(fp)}`}>
                <ShieldCheck size={14} style={{ color: "var(--ok)" }} />
              </span>
            )}
          </div>
          <div className="text-xs flex items-center gap-1.5 mt-0.5" style={{ color: "var(--sub)" }}>
            <StatusDot on={online} />
            {online ? (sessionLive ? "在线 · 已连接" : "在线") : "离线"}
          </div>
        </div>
        <div className="flex-1" />
        <div
          className="hidden md:flex items-center gap-1.5 text-xs px-2.5 h-7 rounded-full cursor-default select-none"
          style={{ background: "var(--acc-weak)", color: "var(--acc)" }}
          title={`本机与对方指纹需一致（TOFU 校验）\n${shortFp(fp)}`}
        >
          <Lock size={12} />
          已加密 · {shortFp(fp).slice(-9)}
        </div>
        <IconBtn title="语音通话（即将上线）" disabled>
          <Phone size={18} />
        </IconBtn>
        <IconBtn title="视频通话（即将上线）" disabled>
          <Video size={18} />
        </IconBtn>
      </div>

      {/* unverified banner */}
      {peer && !peer.confirmed && (
        <div
          className="shrink-0 flex items-center gap-2 px-5 py-2 text-xs"
          style={{ background: "rgba(251,191,36,0.08)", color: "var(--warn)" }}
        >
          <ShieldAlert size={14} className="shrink-0" />
          <span className="truncate">
            首次聊天：请与对方当面核对其指纹为 {shortFp(fp)}，确认后消除提示
          </span>
          <button
            className="ml-auto shrink-0 px-2.5 py-1 rounded-lg font-medium hover:brightness-125"
            style={{ background: "var(--acc-weak)", color: "var(--acc)" }}
            onClick={async () => {
              await api.confirmPeer(fp);
              confirmPeer("已确认对方指纹", "ok");
            }}
          >
            我已核对，确认
          </button>
        </div>
      )}

      <MsgList msgs={msgs} fp={fp} nick={nick} />

      <InputBar fp={fp} online={online} nick={nick} isTyping={!!isTyping} confirmed={!!peer?.confirmed} />
    </div>
  );
}

function IconBtn({
  children,
  title,
  disabled,
  onClick,
}: {
  children: React.ReactNode;
  title?: string;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      title={title}
      disabled={disabled}
      onClick={onClick}
      className="w-9 h-9 rounded-lg flex items-center justify-center transition-colors disabled:opacity-35"
      style={{ color: "var(--sub)" }}
      onMouseEnter={(e) => !disabled && (e.currentTarget.style.background = "var(--panel2)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {children}
    </button>
  );
}

function MsgList({ msgs, fp, nick }: { msgs: Message[]; fp: string; nick: string }) {
  const boxRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = boxRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [msgs.length, fp]);

  let lastDay = "";
  let prevDir = -1;
  let prevTs = 0;

  return (
    <div ref={boxRef} className="flex-1 overflow-y-auto px-6 py-4">
      <div className="max-w-[820px] mx-auto flex flex-col gap-1.5">
        {msgs.map((m) => {
          const day = fmtDay(m.ts);
          const showDay = day !== lastDay;
          lastDay = day;
          const grouped =
            !showDay && m.dir === prevDir && m.ts - prevTs < 5 * 60_000;
          prevDir = m.dir;
          prevTs = m.ts;
          return (
            <div key={m.mid}>
              {showDay && (
                <div className="text-center my-3">
                  <span
                    className="text-xs px-3 py-1 rounded-full"
                    style={{ background: "var(--panel2)", color: "var(--sub)" }}
                  >
                    {day}
                  </span>
                </div>
              )}
              <div className={`flex ${m.dir === 0 ? "justify-end" : "justify-start"} ${grouped ? "mt-0.5" : "mt-2.5"}`}>
                {m.dir === 1 && (
                  <div className={`mr-2.5 ${grouped ? "invisible" : ""}`}>
                    <Avatar fp={fp} nick={nick} size={34} />
                  </div>
                )}
                <Bubble m={m} grouped={grouped} />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Bubble({ m, grouped }: { m: Message; grouped: boolean }) {
  const mine = m.dir === 0;
  return (
    <div className={`flex flex-col ${mine ? "items-end" : "items-start"} max-w-[68%] min-w-0`}>
      <div
        className="msg-select rounded-2xl px-3.5 py-2.5 text-sm leading-relaxed overflow-hidden"
        style={{
          background: mine ? "var(--mine)" : "var(--bubble)",
          border: mine ? "none" : "1px solid var(--line)",
          borderTopRightRadius: mine && grouped ? 6 : undefined,
          borderTopLeftRadius: !mine && grouped ? 6 : undefined,
          color: "var(--txt)",
        }}
      >
        {m.kind === "text" ? (
          <div className="whitespace-pre-wrap break-words msg-select">{m.body}</div>
        ) : m.kind === "image" ? (
          <ImageMsg m={m} />
        ) : m.kind === "audio" ? (
          <AudioBubble m={m} />
        ) : m.kind === "video" ? (
          <VideoMsg m={m} />
        ) : (
          <FileCard m={m} />
        )}
      </div>
      <div className="flex items-center gap-1 mt-1 px-1">
        <span className="text-[11px]" style={{ color: "var(--sub)" }}>
          {fmtClock(m.ts)}
        </span>
        <StateIcon m={m} />
      </div>
    </div>
  );
}

function fmtClock(ts: number): string {
  const d = new Date(ts);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function StateIcon({ m }: { m: Message }) {
  if (m.dir !== 0) return null;
  if (m.progress != null) {
    return (
      <span className="text-[11px]" style={{ color: "var(--sub)" }}>
        {Math.round(m.progress * 100)}%
      </span>
    );
  }
  switch (m.state) {
    case "pending":
      return <Clock4 size={13} style={{ color: "var(--sub)" }} aria-label="等待发送" />;
    case "sent":
      return <Check size={14} style={{ color: "var(--sub)" }} aria-label="已发送" />;
    case "delivered":
      return <CheckCheck size={14} style={{ color: "var(--sub)" }} aria-label="已送达" />;
    case "read":
      return <CheckCheck size={14} style={{ color: "var(--acc)" }} aria-label="已读" />;
    case "failed":
      return <XCircle size={14} style={{ color: "var(--danger)" }} aria-label="失败" />;
    default:
      return null;
  }
}

function transferOverlay(m: Message) {
  if (m.progress == null) return null;
  return (
    <div
      className="absolute inset-0 flex flex-col items-center justify-center gap-1.5"
      style={{ background: "rgba(10,12,18,0.55)", backdropFilter: "blur(2px)" }}
    >
      <Download size={18} className="text-white animate-pulse" />
      <div className="w-2/3 h-1 rounded-full bg-white/25 overflow-hidden">
        <div
          className="h-full rounded-full transition-all"
          style={{ width: `${Math.round(m.progress * 100)}%`, background: "#fff" }}
        />
      </div>
      <span className="text-[11px] text-white">{Math.round(m.progress * 100)}%</span>
    </div>
  );
}

function ImageMsg({ m }: { m: Message }) {
  const setLightbox = useApp((s) => s.setLightbox);
  if (!m.fpath || m.progress != null) {
    return (
      <div className="relative w-56 h-36 rounded-xl overflow-hidden" style={{ background: "var(--panel2)" }}>
        {transferOverlay(m)}
      </div>
    );
  }
  const url = convertFileSrc(m.fpath);
  return (
    <img
      src={url}
      className="rounded-xl max-w-64 max-h-64 object-cover cursor-zoom-in"
      draggable={false}
      onClick={() => setLightbox(url)}
      alt=""
    />
  );
}

function VideoMsg({ m }: { m: Message }) {
  if (!m.fpath || m.progress != null) {
    return (
      <div className="relative w-56 h-36 rounded-xl overflow-hidden" style={{ background: "var(--panel2)" }}>
        {transferOverlay(m)}
      </div>
    );
  }
  return <video src={convertFileSrc(m.fpath)} controls className="rounded-xl max-h-72 max-w-80" />;
}

function FileCard({ m }: { m: Message }) {
  const done = !!m.fpath && m.progress == null;
  return (
    <button
      className="relative flex items-center gap-3 min-w-[240px] max-w-[320px]"
      onClick={() => {
        if (done && m.fpath) api.openPath(m.fpath);
      }}
    >
      <div
        className="relative w-11 h-11 rounded-xl flex items-center justify-center shrink-0 overflow-hidden"
        style={{ background: "var(--acc-weak)", color: "var(--acc)" }}
      >
        <FileIcon size={22} />
        {!done && <div className="absolute inset-0">{transferOverlay(m)}</div>}
      </div>
      <div className="min-w-0 text-left">
        <div className="truncate font-medium">{m.fname ?? "文件"}</div>
        <div className="text-xs mt-0.5" style={{ color: "var(--sub)" }}>
          {m.fsize ? fmtSize(m.fsize) : ""}
          {m.progress != null ? ` · 传输中 ${Math.round((m.progress ?? 0) * 100)}%` : ""}
        </div>
      </div>
      {done && m.fpath && (
        <FolderOpen
          size={16}
          className="ml-2 shrink-0"
          style={{ color: "var(--sub)" }}
          onClick={(e) => {
            e.stopPropagation();
            if (m.fpath) api.revealPath(m.fpath);
          }}
        />
      )}
    </button>
  );
}

function AudioBubble({ m }: { m: Message }) {
  const ref = useRef<HTMLAudioElement | null>(null);
  const [playing, setPlaying] = useState(false);
  const [pos, setPos] = useState(0);
  const [dur, setDur] = useState(0);

  if (!m.fpath || m.progress != null) {
    return (
      <div className="relative flex items-center gap-2 min-w-[220px] h-10">
        <div className="w-8 h-8 rounded-full flex items-center justify-center" style={{ background: "var(--panel2)" }}>
          <Mic size={15} style={{ color: "var(--sub)" }} />
        </div>
        <div className="text-xs" style={{ color: "var(--sub)" }}>
          语音消息 · 传输中
        </div>
      </div>
    );
  }
  const url = convertFileSrc(m.fpath);
  const fmt = (t: number) =>
    `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;
  return (
    <div className="flex items-center gap-2.5 min-w-[240px]" data-url={url}>
      <button
        className="w-8 h-8 rounded-full flex items-center justify-center shrink-0"
        style={{ background: "var(--acc)", color: "#fff" }}
        onClick={() => {
          const a = ref.current;
          if (!a) return;
          if (a.paused) a.play();
          else a.pause();
        }}
      >
        {playing ? <Square size={13} /> : <Play size={14} />}
      </button>
      <input
        type="range"
        min={0}
        max={dur || 0.1}
        step={0.05}
        value={pos}
        onChange={(e) => {
          const v = Number(e.target.value);
          if (ref.current) ref.current.currentTime = v;
          setPos(v);
        }}
        className="flex-1 accent-blue-500"
      />
      <span className="text-xs tabular-nums" style={{ color: "var(--sub)" }}>
        {fmt(pos)}/{fmt(dur)}
      </span>
      <audio
        ref={ref}
        src={url}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => {
          setPlaying(false);
          setPos(0);
        }}
        onTimeUpdate={(e) => setPos(e.currentTarget.currentTime)}
        onLoadedMetadata={(e) => setDur(e.currentTarget.duration || 0)}
      />
    </div>
  );
}

function InputBar({ fp, online, nick, isTyping, confirmed }: { fp: string; online: boolean; nick: string; isTyping: boolean; confirmed: boolean }) {
  const toast = useApp((s) => s.toast);
  const [text, setText] = useState("");
  const [emojiOpen, setEmojiOpen] = useState(false);
  const [recording, setRecording] = useState(false);
  const [recSecs, setRecSecs] = useState(0);
  const taRef = useRef<HTMLTextAreaElement | null>(null);
  const recRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const timerRef = useRef<number | undefined>(undefined);
  const lastTypingRef = useRef(0);

  const doSendText = async () => {
    const body = text.trim();
    if (!body) return;
    if (!confirmed) {
      if (!window.confirm(`对方指纹尚未核对，确定要发送消息给 ${nick} 吗？`)) return;
    }
    setText("");
    try {
      const msg = await api.sendText(fp, body);
      const s = useApp.getState();
      const list = s.messages[fp] ?? [];
      if (!list.some((x) => x.mid === msg.mid)) {
        useApp.setState({ messages: { ...s.messages, [fp]: [...list, msg] } });
      }
    } catch (e) {
      toast(String(e), "err");
      setText(body);
    }
  };

  const notifyTyping = () => {
    const now = Date.now();
    if (now - lastTypingRef.current > 2200) {
      lastTypingRef.current = now;
      api.typing(fp);
    }
  };

  const pickFiles = async (imagesOnly: boolean) => {
    const res = await openFileDialog({
      multiple: true,
      ...(imagesOnly
        ? {
            filters: [
              { name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] },
            ],
          }
        : {}),
    });
    if (!res) return;
    const paths = Array.isArray(res) ? res : [res];
    const results = await api.sendFiles(fp, paths);
    for (const r of results) {
      if (r && typeof r.error === "string") toast(r.error, "err");
    }
  };

  const sendBlob = async (blob: Blob, name: string, kind: string) => {
    try {
      const buf = await blob.arrayBuffer();
      await api.sendMedia(fp, new Uint8Array(buf), name, kind);
    } catch (e) {
      toast(String(e), "err");
    }
  };

  const startRec = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mime = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : "audio/webm";
      const mr = new MediaRecorder(stream, { mimeType: mime });
      chunksRef.current = [];
      mr.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };
      mr.start(250);
      recRef.current = mr;
      setRecording(true);
      setRecSecs(0);
      timerRef.current = window.setInterval(() => {
        setRecSecs((s) => {
          if (s >= 599) {
            void finishRec(true);
            return s;
          }
          return s + 1;
        });
      }, 1000);
    } catch {
      toast("无法访问麦克风", "err");
    }
  };

  const finishRec = async (send: boolean): Promise<void> => {
    const mr = recRef.current;
    window.clearInterval(timerRef.current);
    if (!mr || mr.state === "inactive") {
      setRecording(false);
      return;
    }
    const blob: Blob | null = await new Promise((resolve) => {
      mr.onstop = () => {
        mr.stream.getTracks().forEach((t) => t.stop());
        resolve(new Blob(chunksRef.current, { type: "audio/webm" }));
      };
      mr.stop();
    });
    recRef.current = null;
    setRecording(false);
    if (send && blob && blob.size > 800) {
      const d = new Date();
      await sendBlob(
        blob,
        `语音_${String(d.getHours()).padStart(2, "0")}${String(d.getMinutes()).padStart(2, "0")}.webm`,
        "audio"
      );
    }
  };

  return (
    <div
      className="shrink-0 relative"
      style={{ borderTop: "1px solid var(--line)", background: "var(--panel)" }}
    >
      {isTyping && (
        <div className="absolute -top-7 left-6 text-xs px-2 py-0.5 rounded-md" style={{ background: "var(--panel2)", color: "var(--sub)" }}>
          {nick} 正在输入…
        </div>
      )}
      {emojiOpen && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setEmojiOpen(false)} />
          <div
            className="absolute bottom-full mb-2 left-14 z-20 p-3 rounded-xl grid grid-cols-10 gap-1 shadow-xl"
            style={{ background: "var(--panel2)", border: "1px solid var(--line)" }}
          >
            {EMOJIS.map((em) => (
              <button
                key={em}
                className="w-8 h-8 rounded-lg text-lg hover:bg-white/10"
                onClick={() => {
                  setText((t) => t + em);
                  setEmojiOpen(false);
                  taRef.current?.focus();
                }}
              >
                {em}
              </button>
            ))}
          </div>
        </>
      )}

      {recording ? (
        <div className="h-[92px] flex items-center gap-4 px-5">
          <span className="flex items-center gap-2 text-sm" style={{ color: "var(--danger)" }}>
            <span className="w-2.5 h-2.5 rounded-full animate-pulse" style={{ background: "var(--danger)" }} />
            录制中 {Math.floor(recSecs / 60)}:{String(recSecs % 60).padStart(2, "0")}
          </span>
          <div className="flex-1" />
          <button
            className="px-4 h-9 rounded-xl text-sm flex items-center gap-1.5"
            style={{ background: "var(--panel2)", color: "var(--sub)" }}
            onClick={() => void finishRec(false)}
          >
            <Trash2 size={15} /> 取消
          </button>
          <button
            className="px-4 h-9 rounded-xl text-sm text-white flex items-center gap-1.5"
            style={{ background: "var(--acc)" }}
            onClick={() => void finishRec(true)}
          >
            <Send size={15} /> 发送
          </button>
        </div>
      ) : (
        <div className="px-4 py-3">
          <div className="flex items-center gap-1 mb-2">
            <IconBtn title="发送图片" onClick={() => void pickFiles(true)}>
              <ImagePlus size={17} />
            </IconBtn>
            <IconBtn title="发送文件" onClick={() => void pickFiles(false)}>
              <Paperclip size={17} />
            </IconBtn>
            <div className="relative">
              <IconBtn title="表情" onClick={() => setEmojiOpen(!emojiOpen)}>
                <Smile size={17} />
              </IconBtn>
            </div>
            <div className="flex-1" />
            <span className="text-[11px] mr-1" style={{ color: "var(--sub)" }}>
              Enter 发送 · Shift+Enter 换行
            </span>
            <IconBtn
              title="按住说话（点击开始录制语音）"
              onClick={() => void startRec()}
            >
              <Mic size={17} />
            </IconBtn>
          </div>
          <div className="flex items-end gap-2">
            <textarea
              ref={taRef}
              value={text}
              rows={2}
              placeholder={online ? `发消息给 ${nick}` : `${nick} 离线中，消息将在其上线后送达`}
              onChange={(e) => {
                setText(e.target.value);
                notifyTyping();
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  void doSendText();
                }
              }}
              onPaste={async (e) => {
                const file = Array.from(e.clipboardData.files)[0];
                if (file && file.type.startsWith("image/")) {
                  e.preventDefault();
                  await sendBlob(file, `粘贴_${Date.now()}.${file.type.split("/")[1] ?? "png"}`, "image");
                }
              }}
              className="flex-1 bg-transparent text-sm leading-relaxed max-h-32 msg-select px-1"
              style={{ color: "var(--txt)" }}
            />
            <button
              onClick={() => void doSendText()}
              disabled={!text.trim()}
              className="w-10 h-10 rounded-xl flex items-center justify-center text-white shrink-0 transition-opacity disabled:opacity-35"
              style={{ background: "var(--acc)" }}
            >
              <Send size={17} />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
