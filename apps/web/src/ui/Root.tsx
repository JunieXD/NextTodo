import { type ReactNode, useState } from "react";
import { FeedbackProvider, RootContext } from "./feedback";

/** The app's outer element, which panels and dialogs open inside so that its layout styles apply.
 * `outside`: the app has been opened from outside, to quick add or to a task; dialogs make way. */
export function Root({ className, outside = false, children }: { className: string; outside?: boolean; children: ReactNode }) {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  return (
    <div ref={setEl} className={className}>
      <RootContext value={el}>
        <FeedbackProvider outside={outside}>{children}</FeedbackProvider>
      </RootContext>
    </div>
  );
}
