// Requests to the server. The web app signs in with an HttpOnly cookie, so no token is handled here.

import type { ErrorBody } from "./types/ErrorBody";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    /** The request field the error is about, when there is one. */
    readonly field?: string,
  ) {
    super(message);
  }
}

export const OFFLINE = "offline";

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`/api/v1${path}`, {
      method,
      headers: body === undefined ? {} : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, OFFLINE, "连不上服务器，请检查网络");
  }
  if (res.status === 204) return undefined as T;
  if (res.ok) return (await res.json()) as T;
  // A proxy in between may answer with a page of its own.
  const error: Partial<ErrorBody> = await res.json().catch(() => ({}));
  const field = (error.details as { field?: string } | undefined)?.field;
  throw new ApiError(res.status, error.code ?? "error", error.message ?? `服务器出错了（${res.status}）`, field);
}

/** How this browser shows up among the user's devices, such as `Chrome · macOS`. */
export function deviceName(): string {
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "浏览器";
  const os = /iPhone|iPad/.test(ua) ? "iOS" : /Android/.test(ua) ? "Android" : /Mac OS/.test(ua) ? "macOS" : /Windows/.test(ua) ? "Windows" : /Linux/.test(ua) ? "Linux" : "";
  return os ? `${browser} · ${os}` : browser;
}
