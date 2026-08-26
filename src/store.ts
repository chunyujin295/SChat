import { create } from "zustand";
import { api, wireListeners } from "./api";
import { bustAvatarFp } from "./components/ui";
import type { AppConfig, Conversation, Message, PeerView, Profile } from "./types";

interface Toast {
  id: number;
  text: string;
  kind: "err" | "ok" | "info";
}

interface AppState {
  ready: boolean;
  profile: Profile | null;
  config: AppConfig | null;
  peers: PeerView[];
  conversations: Conversation[];
  messages: Record<string, Message[]>;
  loaded: Record<string, boolean>;
  activeFp: string | null;
  sessionOnline: Record<string, boolean>;
  typingUntil: Record<string, number>;
  tab: "chats" | "nearby";
  settingsOpen: boolean;
  lightbox: string | null;
  dragOver: boolean;
  toasts: Toast[];
  search: string;

  init: () => Promise<void>;
  wireEvents: () => () => void;
  setActive: (fp: string) => Promise<void>;
  setTab: (t: "chats" | "nearby") => void;
  setSearch: (s: string) => void;
  setSettingsOpen: (b: boolean) => void;
  setLightbox: (u: string | null) => void;
  setDragOver: (b: boolean) => void;
  toast: (text: string, kind?: Toast["kind"]) => void;
  dropToast: (id: number) => void;
  applyTheme: () => void;
  patchConfigLocal: (c: Partial<AppConfig>) => void;
  refreshPeers: () => void;
}

let midIndex: Record<string, string> = {};
let unreadBackup: Record<string, number> = {};

function convKey(list: Conversation[], fp: string) {
  return list.find((c) => c.fp === fp);
}

export const useApp = create<AppState>()((set, get) => ({
  ready: false,
  profile: null,
  config: null,
  peers: [],
  conversations: [],
  messages: {},
  loaded: {},
  activeFp: null,
  sessionOnline: {},
  typingUntil: {},
  tab: "chats",
  settingsOpen: false,
  lightbox: null,
  dragOver: false,
  toasts: [],
  search: "",

  async init() {
    const bs = await api.bootstrap();
    set({
      profile: bs.profile,
      config: bs.config,
      peers: bs.peers,
      conversations: bs.conversations,
      ready: true,
    });
    get().applyTheme();
    unreadBackup = {};
    for (const c of bs.conversations) unreadBackup[c.fp] = c.unread;
  },

  wireEvents() {
    return wireListeners({
      peers: (peers) => {
        set({ peers });
        const s = get();
        const map = new Map(peers.map((p) => [p.fp, p]));
        let convs = s.conversations.filter((c) => map.has(c.fp));
        for (const p of peers) {
          const c = convKey(convs, p.fp);
          if (!c) {
            if ((unreadBackup[p.fp] ?? 0) > 0 || p.online) {
              convs.push({
                fp: p.fp,
                nick: p.nick,
                online: p.online,
                confirmed: p.confirmed,
                lastTs: Date.now(),
                preview: "",
                unread: unreadBackup[p.fp] ?? 0,
              });
            }
          } else {
            c.nick = p.nick;
            c.online = p.online;
            c.confirmed = p.confirmed;
          }
        }
        convs = convs.map((c) => {
          const p = map.get(c.fp)!;
          return { ...c, nick: p.nick, online: p.online, confirmed: p.confirmed };
        });
        convs.sort((a, b) => b.lastTs - a.lastTs);
        set({ conversations: convs });
      },
      "message-new": (m) => {
        const s = get();
        midIndex[m.mid] = m.fp;
        const list = s.messages[m.fp];
        if (list && !list.some((x) => x.mid === m.mid)) {
          set({ messages: { ...s.messages, [m.fp]: [...list, m] } });
        }
        const convs = [...s.conversations];
        const c = convKey(convs, m.fp);
        const kindLabel: Record<string, string> = {
          image: "图片",
          file: "文件",
          audio: "语音",
          video: "视频",
        };
        const preview =
          m.kind === "text"
            ? m.body ?? ""
            : `[${kindLabel[m.kind] ?? "文件"}] ${m.fname ?? ""}`;
        if (c) {
          c.lastTs = m.ts;
          c.preview = preview.slice(0, 60);
          if (m.dir === 1) c.unread += 1;
          convs.sort((a, b) => b.lastTs - a.lastTs);
          set({ conversations: convs });
        }
        if (m.dir === 1 && s.activeFp === m.fp && document.hasFocus()) {
          api.markRead(m.fp);
          unreadBackup[m.fp] = 0;
          const cs = get().conversations;
          const cc = convKey(cs, m.fp);
          if (cc) cc.unread = 0;
          set({ conversations: cs });
        }
        if (m.dir === 1) {
          unreadBackup[m.fp] = (unreadBackup[m.fp] ?? 0) + 1;
        }
      },
      "message-state": ({ mid, state }) => {
        const fp = midIndex[mid];
        if (!fp) return;
        const s = get();
        const list = s.messages[fp];
        if (!list) return;
        set({
          messages: {
            ...s.messages,
            [fp]: list.map((x) =>
              x.mid === mid
                ? { ...x, state, progress: state === "failed" ? null : x.progress }
                : x
            ),
          },
        });
      },
      session: ({ fp, state }) => {
        const s = get();
        set({
          sessionOnline: { ...s.sessionOnline, [fp]: state === "open" },
        });
      },
      typing: ({ fp }) => {
        const s = get();
        set({ typingUntil: { ...s.typingUntil, [fp]: Date.now() + 2600 } });
      },
      transfer: ({ mid, sent, total }) => {
        const fp = midIndex[mid];
        if (!fp) return;
        const s = get();
        const list = s.messages[fp];
        if (!list) return;
        set({
          messages: {
            ...s.messages,
            [fp]: list.map((x) =>
              x.mid === mid ? { ...x, progress: total > 0 ? sent / total : 0 } : x
            ),
          },
        });
      },
      "transfer-done": ({ mid }) => {
        const fp = midIndex[mid];
        if (!fp) return;
        const s = get();
        const list = s.messages[fp];
        if (!list) return;
        set({
          messages: {
            ...s.messages,
            [fp]: list.map((x) => (x.mid === mid ? { ...x, progress: null } : x)),
          },
        });
      },
      alert: (a) => {
        get().toast(a.message ?? `安全提醒：${a.code}`, "err");
      },
      "call-signal": () => {},
      "avatar-changed": ({ fp }) => bustAvatarFp(fp),
    });
  },

  async setActive(fp) {
    const s = get();
    set({ activeFp: fp, settingsOpen: false });
    if (!s.loaded[fp]) {
      const msgs = await api.listMessages(fp);
      for (const m of msgs) midIndex[m.mid] = fp;
      set({ messages: { ...get().messages, [fp]: msgs }, loaded: { ...get().loaded, [fp]: true } });
    }
    api.openSession(fp).then((r) => {
      set({ sessionOnline: { ...get().sessionOnline, [fp]: true } });
      if (!r.verified) {
        /* banner shows from peer.confirmed */
      }
    });
    api.markRead(fp);
    unreadBackup[fp] = 0;
    const convs = get().conversations;
    const c = convKey(convs, fp);
    if (c) {
      c.unread = 0;
      set({ conversations: [...convs] });
    }
  },

  setTab(tab) {
    set({ tab });
  },
  setSearch(search) {
    set({ search });
  },
  setSettingsOpen(settingsOpen) {
    set({ settingsOpen });
  },
  setLightbox(lightbox) {
    set({ lightbox });
  },
  setDragOver(dragOver) {
    set({ dragOver });
  },

  toast(text, kind = "info") {
    const id = Date.now() + Math.random();
    set({ toasts: [...get().toasts, { id, text, kind }] });
    setTimeout(() => get().dropToast(id), 4000);
  },
  dropToast(id) {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },

  applyTheme() {
    const t = get().config?.theme ?? "dark";
    document.documentElement.dataset.theme = t === "light" ? "light" : "dark";
  },

  patchConfigLocal(pc) {
    const cfg = { ...(get().config ?? ({} as AppConfig)), ...pc };
    set({ config: cfg });
    get().applyTheme();
  },

  refreshPeers() {},
}));

export function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function fmtTime(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return hm;
  const yest = new Date(now.getTime() - 86400000);
  if (d.toDateString() === yest.toDateString()) return `昨天 ${hm}`;
  if (d.getFullYear() === now.getFullYear()) {
    return `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
  }
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`;
}

export function fmtDay(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return "今天";
  const yest = new Date(now.getTime() - 86400000);
  if (d.toDateString() === yest.toDateString()) return "昨天";
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
}

export function shortFp(fp: string): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let out = "";
  const bytes: number[] = [];
  for (let i = 0; i < 15 && i * 2 < fp.length; i++) {
    bytes.push(parseInt(fp.slice(i * 2, i * 2 + 2), 16));
  }
  let bits = 0;
  let acc = 0;
  for (const b of bytes) {
    acc = (acc << 8) | b;
    bits += 8;
    while (bits >= 5 && out.length < 24) {
      bits -= 5;
      out += chars[(acc >> bits) & 31];
    }
    if (out.length >= 24) break;
  }
  const groups = (out.padEnd(24, "A").match(/.{4}/g) ?? []).slice(0, 6);
  return `SCAT-${groups.join("-")}`;
}
