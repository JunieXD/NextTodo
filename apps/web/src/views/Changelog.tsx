// The releases' notes: told of once after an update, and all of them from the settings
// (docs/design.md#更新日志).

import { useContext, useEffect, useState } from "react";
import { type Release, RELEASES, byKind, seeVersion, unseen } from "../changelog";
import { Modal, RootContext } from "../ui/feedback";

/** The most releases told of after an update, before the earlier ones are asked for. */
const MOST = 10;

/** After an update, what changed since the version this device last ran, until the user closes it.
 * Not when the app has been opened from outside, to quick add or to a task: the next time it comes to
 * the front then, which on Android may be long before it is next started. */
export function WhatsNew({ outside }: { outside: boolean }) {
  const [news] = useState(unseen);
  const [open, setOpen] = useState(!!news && !outside);
  const [told, setTold] = useState(false);
  const [earlier, setEarlier] = useState(false);
  // Panels open in the app's root element, there once the main view has first rendered.
  const root = useContext(RootContext);
  useEffect(() => {
    if (!news) seeVersion();
  }, [news]);
  useEffect(() => {
    if (!news || told || outside) return;
    const shown = () => document.visibilityState === "visible" && setOpen(true);
    document.addEventListener("visibilitychange", shown);
    return () => document.removeEventListener("visibilitychange", shown);
  }, [news, told, outside]);
  // Opened from outside meanwhile, it makes way, as dialogs do, to be told of the next time.
  if (outside && open) setOpen(false);
  if (!news || !open || !root) return null;
  const close = () => {
    seeVersion();
    setTold(true);
    setOpen(false);
  };
  const { since, releases } = news;
  const listed = earlier ? releases : releases.slice(0, MOST);
  return (
    <Modal
      title={`已更新到 ${import.meta.env.WEB_VERSION}`}
      onClose={close}
      wide
      footer={
        <button className="filled-btn" onClick={close}>
          知道了
        </button>
      }
    >
      <p className="release-since">
        自 {since} 以来的更新
        {releases.length > 1 && ` · ${releases.length} 个版本`}
        {listed.length < releases.length && `，列出最新 ${MOST} 个`}
      </p>
      <Changes releases={listed} tagged={releases.length > 1} />
      {listed.length < releases.length && (
        <button className="text-btn earlier" onClick={() => setEarlier(true)}>
          还有 {releases.length - MOST} 个更早的版本
        </button>
      )}
    </Modal>
  );
}

/** All the releases' notes, the latest first. */
export function Changelog({ onClose }: { onClose(): void }) {
  return (
    <Modal title="更新日志" onClose={onClose} wide>
      {RELEASES.map((r) => (
        <section key={r.version} className="release">
          <h4>
            {r.version}
            <span>{r.date}</span>
          </h4>
          <Changes releases={[r]} tagged={false} />
        </section>
      ))}
    </Modal>
  );
}

/** The changes of `releases` by kind, each with its version when `tagged`. */
function Changes({ releases, tagged }: { releases: Release[]; tagged: boolean }) {
  return byKind(releases).map(({ kind, changes }) => (
    <section key={kind} className="changes">
      <h5>
        {kind}
        <span>{changes.length}</span>
      </h5>
      <ul>
        {changes.map((c) => (
          <li key={`${c.version} ${c.text}`}>
            {c.text}
            {tagged && <span className="version">{c.version}</span>}
          </li>
        ))}
      </ul>
    </section>
  ));
}
