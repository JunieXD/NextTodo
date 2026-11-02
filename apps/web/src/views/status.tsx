import { useView } from "../session";
import type { Connection } from "../sync/socket";

/** Says that changes wait for the network, when they do. */
export function ConnectionNote({ connection }: { connection: Connection }) {
  const { pending } = useView();
  if (connection !== "offline") return null;
  return <p className="status-note">离线{pending ? `，${pending} 项改动会在联网后同步` : "，改动会在联网后同步"}</p>;
}
