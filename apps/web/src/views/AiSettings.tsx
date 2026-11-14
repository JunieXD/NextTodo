// The settings of AI organizing: which model, how much it thinks, and whether they stay on this device
// or go to the server for every device. See docs/ai.md#模型配置.

import { Popover } from "radix-ui";
import { useEffect, useId, useRef, useState } from "react";
import { ApiError, api, app } from "../api/http";
import type { AiConfig } from "../api/types/AiConfig";
import type { AiModels } from "../api/types/AiModels";
import type { AiModelsInput } from "../api/types/AiModelsInput";
import type { AiProviders } from "../api/types/AiProviders";
import type { AiSettings } from "../api/types/AiSettings";
import type { AiSettingsInput } from "../api/types/AiSettingsInput";
import type { AiTestResult } from "../api/types/AiTestResult";
import type { AiThinking } from "../api/types/AiThinking";
import { keepAiConfig, localAiConfig } from "../ai";
import { setAiReady } from "../android";
import { inLocalMode } from "../local";
import { useSession } from "../session";
import { useBack } from "../ui/back";
import { Seg, Select } from "../ui/controls";
import { useFeedback } from "../ui/feedback";
import { Help } from "../ui/help";

type Place = "local" | "server";

/** What the page is doing, the buttons waiting meanwhile. */
type Busy = "models" | "test" | "save" | "clear";

/** What the settings row shows: the model and where its settings are, or that there are none. */
export function useAiSummary(): string {
  const { user } = useSession();
  const [server, setServer] = useState<AiSettings | null | undefined>(undefined);
  const local = localAiConfig(user.id);
  const here = local !== null;
  useEffect(() => {
    if (!here) api<AiSettings | null>("GET", "/ai/settings").then(setServer, () => setServer(undefined));
  }, [here]);
  if (local) return `${local.model} · 仅本机`;
  if (server) return `${server.model} · 已同步`;
  return server === null ? "未配置" : "";
}

/** The model AI organizing uses; saved or cleared, back to where it was opened from. */
export function AiPage() {
  const { user } = useSession();
  const { snack, confirm } = useFeedback();
  const [providers, setProviders] = useState<AiProviders | null>(null);
  const [kept, setKept] = useState<AiSettings | null>(null);
  const [form, setForm] = useState<AiSettingsInput | null>(null);
  const [place, setPlace] = useState<Place>("local");
  const [models, setModels] = useState<string[]>([]);
  const [result, setResult] = useState<AiTestResult | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<Busy | null>(null);

  const failed = (e: unknown) => {
    if (!(e instanceof ApiError)) throw e;
    setError(e.message);
  };
  useEffect(() => {
    void (async () => {
      try {
        const [p, s] = await Promise.all([api<AiProviders>("GET", "/ai/providers"), api<AiSettings | null>("GET", "/ai/settings")]);
        setProviders(p);
        setKept(s);
        const local = localAiConfig(user.id);
        if (local) setForm(local);
        else if (s) {
          setForm({ provider: s.provider, base_url: s.base_url, model: s.model, thinking: s.thinking });
          setPlace("server");
        } else {
          const first = p.providers[0];
          setForm({ provider: first.id, base_url: first.base_url, model: first.model, thinking: "off", api_key: "" });
        }
      } catch (e) {
        failed(e);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- loads once when opened
  }, []);

  if (!providers || !form) return error ? <p className="form-error">{error}</p> : <p className="empty">正在读取…</p>;
  const provider = providers.providers.find((p) => p.id === form.provider) ?? providers.providers[0];
  const set = (change: Partial<AiSettingsInput>) => {
    setForm({ ...form, ...change });
    setResult(null);
    setError("");
  };
  const pick = (id: string) => {
    const p = providers.providers.find((p) => p.id === id)!;
    set({ provider: id, base_url: p.base_url, model: p.model });
    setModels([]);
  };
  // The key typed, or none for the one kept on the server.
  const typed = form.api_key?.trim() ?? "";
  const input: AiSettingsInput = { ...form, ...(typed ? { api_key: typed } : { api_key: undefined }) };
  const run = async (what: Busy, f: () => Promise<void>) => {
    setBusy(what);
    try {
      await f();
    } catch (e) {
      failed(e);
    } finally {
      setBusy(null);
    }
  };
  // Longer than the server waits for the model, a minute, so that its answer comes back.
  const test = () => run("test", async () => setResult(await api<AiTestResult>("POST", "/ai/test", input, 70_000)));
  const listModels = () =>
    run("models", async () => {
      const asked: AiModelsInput = { base_url: form.base_url, api_key: input.api_key };
      const { models } = await api<AiModels>("POST", "/ai/models", asked, 30_000);
      setModels(models);
      if (!models.length) snack("接口没有列出模型，请手动填写");
    });
  const save = () =>
    run("save", async () => {
      if (place === "local") {
        // The server checks these when they are handed over, which may be much later.
        if (!/^https:\/\/\S+$/.test(form.base_url)) return setError("接口地址要以 https:// 开头，不含空格");
        if (!form.model) return setError("请填写模型");
        if (!typed) return setError("请填写 API 密钥");
        const config: AiConfig = { provider: form.provider, base_url: form.base_url, model: form.model, thinking: form.thinking, api_key: typed };
        keepAiConfig(user.id, config);
        if (kept) await api("DELETE", "/ai/settings");
        setKept(null);
      } else {
        setKept(await api<AiSettings>("PUT", "/ai/settings", input));
        keepAiConfig(user.id, null);
      }
      if (app === "android") await setAiReady(true);
      snack(place === "local" ? "已保存在本机" : "已同步到服务器");
      history.back();
    });
  const clear = async () => {
    const body = `${inLocalMode() ? "" : "本机和服务器上保存的都会删除，"}等待中的请求要配置后才会整理。`;
    if (!(await confirm({ title: "清除 AI 整理的配置？", body, ok: "清除", danger: true }))) return;
    await run("clear", async () => {
      keepAiConfig(user.id, null);
      if (kept) await api("DELETE", "/ai/settings");
      if (app === "android") await setAiReady(false);
      history.back();
    });
  };
  const keyHint =
    place === "server" && kept ? (kept.key_end ? `已保存，末 4 位 ${kept.key_end}；不改就留空` : "服务器解不开保存的密钥，请重新填写") : "只发给所选的服务商";

  return (
    <>
      <p className="empty">
        在添加框点 ✦ 切到 AI 整理，写下一段话，由所选的模型整理成事项。原文、文件夹名和其中未完成的事项会发给这个服务商。<Help to="ai#key">如何获取 API 密钥</Help>
      </p>
      <div className="field">
        服务商
        <Select label="服务商" items={providers.providers.map((p): [string, string] => [p.id, p.name])} value={provider.id} onChange={pick} />
      </div>
      <label className="field">
        <span>
          接口地址 <span className="hint">OpenAI 兼容，https 开头</span>
        </span>
        <input value={form.base_url} onChange={(e) => set({ base_url: e.target.value.trim() })} placeholder="https://api.example.com/v1" inputMode="url" />
      </label>
      <label className="field">
        <span>
          API 密钥 <span className="hint">{keyHint}</span>
        </span>
        <input type="password" autoComplete="off" value={form.api_key ?? ""} onChange={(e) => set({ api_key: e.target.value })} />
      </label>
      <div className="field">
        <span>
          模型{" "}
          {/* Saying so where it was asked: the list comes under the field, a while later. */}
          <button type="button" className="hint link-btn" onClick={listModels} disabled={busy !== null}>
            {busy === "models" ? "正在获取…" : "从接口获取列表"}
          </button>
        </span>
        <Suggest label="模型" value={form.model} options={models} onChange={(model) => set({ model: model.trim() })} />
      </div>
      {provider.thinking !== "none" && (
        <div className="field">
          思考程度
          <Seg<AiThinking>
            label="思考程度"
            items={[
              ["off", "关闭"],
              ["low", "低"],
              ["high", "高"],
            ]}
            value={form.thinking}
            onChange={(thinking) => set({ thinking })}
          />
        </div>
      )}
      {/* In local mode the server is this device, without the key to keep a configuration with
          (docs/local.md#界面). */}
      {!inLocalMode() && (
        <div className="field">
          <span>
            保存位置 <span className="hint">{place === "local" ? "这台设备联网时才会整理" : providers.sync ? "提交后设备可以离线，服务器自己整理" : ""}</span>
          </span>
          <Seg<Place>
            label="保存位置"
            items={[
              ["local", "仅本机"],
              ["server", "同步到服务器"],
            ]}
            value={place}
            onChange={(p) => (p === "server" && !providers.sync ? setError("服务器没有设置 NEXTTODO_SECRET_KEY，配置只能保存在本机") : setPlace(p))}
          />
        </div>
      )}
      {result && <p className={result.ok ? "form-ok" : "form-error"}>{result.message}</p>}
      {error && <p className="form-error">{error}</p>}
      <div className="actions">
        {(kept || localAiConfig(user.id)) && (
          <button className="text-btn danger-text" onClick={clear} disabled={busy !== null}>
            清除
          </button>
        )}
        <button className="tonal-btn" onClick={test} disabled={busy !== null}>
          测试
        </button>
        <button className="filled-btn" onClick={save} disabled={busy !== null}>
          保存
        </button>
      </div>
    </>
  );
}

/** A text field with `options` listed under it: all of them as it is focused or the options come,
 * then those with what is typed in them. Arrow keys and Enter pick one too. */
function Suggest({ label, value, options, onChange }: { label: string; value: string; options: string[]; onChange(v: string): void }) {
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  // Typed since the field was focused; null for all the options.
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(-1);
  const [came, setCame] = useState(options);
  if (came !== options) {
    setCame(options);
    setOpen(options.length > 0);
    setQuery(null);
    setActive(-1);
  }
  const shown = query === null ? options : options.filter((o) => o.toLowerCase().includes(query.toLowerCase()));
  const listed = open && shown.length > 0;
  useBack("dialog", listed ? () => setOpen(false) : null);
  const show = (q: string | null) => {
    setQuery(q);
    setActive(-1);
    setOpen(true);
  };
  const choose = (o: string) => {
    onChange(o);
    setOpen(false);
  };
  const key = (e: React.KeyboardEvent) => {
    if (!listed) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = shown.length;
      const i = e.key === "ArrowDown" ? (active + 1) % n : active <= 0 ? n - 1 : active - 1;
      setActive(i);
      list.current!.children[i].scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && active >= 0) {
      e.preventDefault();
      choose(shown[active]);
    }
  };
  return (
    <Popover.Root open={listed} onOpenChange={setOpen}>
      <Popover.Anchor asChild>
        <input
          ref={input}
          role="combobox"
          aria-label={label}
          aria-autocomplete="list"
          aria-expanded={listed}
          aria-controls={id}
          aria-activedescendant={listed && active >= 0 ? `${id}-${active}` : undefined}
          autoComplete="off"
          value={value}
          onFocus={() => show(null)}
          onClick={() => setOpen(true)}
          onChange={(e) => (onChange(e.target.value), show(e.target.value.trim()))}
          onKeyDown={key}
        />
      </Popover.Anchor>
      {/* Not in a portal: inside the panel, the list scrolls, which the panel stops outside it. */}
      <Popover.Content
        ref={list}
        id={id}
        role="listbox"
        className="menu suggest"
        align="start"
        sideOffset={4}
        onOpenAutoFocus={(e) => e.preventDefault()}
        onCloseAutoFocus={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.target === input.current && e.preventDefault()}
      >
        {shown.map((o, i) => (
          <div
            key={o}
            id={`${id}-${i}`}
            role="option"
            aria-selected={i === active}
            className="menu-item"
            data-highlighted={i === active ? "" : undefined}
            onMouseMove={() => setActive(i)}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => choose(o)}
          >
            {o}
          </div>
        ))}
      </Popover.Content>
    </Popover.Root>
  );
}
