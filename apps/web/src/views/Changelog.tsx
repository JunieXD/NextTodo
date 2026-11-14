// The releases' notes: told of once after an update, and all of them from the settings
// (docs/design.md#更新日志).

import { Suspense, useContext, useEffect, useState } from "react";
import { type Release, RELEASES, seeVersion, unseen } from "../changelog";
import { Modal, RootContext } from "../ui/feedback";
import { Prose } from "../ui/lazy-markdown";

/** The most releases told of after an update, before the earlier ones are asked for. */
const MOST = 10;

/** After an update, what changed since the version this device last ran. Not when the app has been
 * opened from outside, to quick add or to a task: the next time then. */
export function WhatsNew({ outside }: { outside: boolean }) {
  const [releases] = useState(() => (outside ? null : unseen()));
  const [open, setOpen] = useState(!!releases?.length);
  const [earlier, setEarlier] = useState(false);
  // Panels open in the app's root element, there once the main view has first rendered.
  const root = useContext(RootContext);
  useEffect(() => {
    if (releases) seeVersion();
  }, [releases]);
  // Opened from outside meanwhile, it makes way, as dialogs do.
  if (outside && open) setOpen(false);
  if (!releases || !open || !root) return null;
  const close = () => setOpen(false);
  return (
    <Modal title={`已更新到 ${import.meta.env.WEB_VERSION}`} onClose={close}>
      <Releases releases={earlier ? releases : releases.slice(0, MOST)} />
      {!earlier && releases.length > MOST && (
        <button className="text-btn earlier" onClick={() => setEarlier(true)}>
          还有 {releases.length - MOST} 个更早的版本
        </button>
      )}
      <div className="actions">
        <button className="filled-btn" onClick={close}>
          知道了
        </button>
      </div>
    </Modal>
  );
}

/** All the releases' notes, the latest first. */
export function Changelog({ onClose }: { onClose(): void }) {
  return (
    <Modal title="更新日志" onClose={onClose}>
      <Releases releases={RELEASES} />
    </Modal>
  );
}

function Releases({ releases }: { releases: Release[] }) {
  return releases.map((r) => (
    <section key={r.version} className="release">
      <h4>
        {r.version}
        <span>{r.date}</span>
      </h4>
      <div className="md">
        <Suspense>
          <Prose source={r.notes} />
        </Suspense>
      </div>
    </section>
  ));
}
