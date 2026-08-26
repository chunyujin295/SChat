export interface PeerView {
  fp: string;
  nick: string;
  online: boolean;
  ip: string;
  port: number;
  avaVer: number;
  confirmed: boolean;
  blocked: boolean;
  lastSeen: number;
}

export interface Conversation {
  fp: string;
  nick: string;
  online: boolean;
  confirmed: boolean;
  lastTs: number;
  preview: string;
  unread: number;
}

export type MsgKind = "text" | "image" | "file" | "audio" | "video";

export interface Message {
  mid: string;
  fp: string;
  dir: number;
  kind: MsgKind;
  body?: string | null;
  fid?: string | null;
  fname?: string | null;
  fsize?: number | null;
  mime?: string | null;
  fpath?: string | null;
  ts: number;
  state: string;
  progress?: number | null;
}

export interface Profile {
  nickname: string;
  fp: string;
  fpDisplay: string;
  avaVer: number;
}

export interface AppConfig {
  theme: string;
  hotkey: string;
  closeToTray: boolean;
  onboarded: boolean;
}
