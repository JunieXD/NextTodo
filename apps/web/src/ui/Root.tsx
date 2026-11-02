import { type ReactNode, useState } from "react";
import { FeedbackProvider, RootContext } from "./feedback";

/** The app's outer element, which panels and dialogs open inside so that its layout styles apply. */
export function Root({ className, children }: { className: string; children: ReactNode }) {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  return (
    <div ref={setEl} className={className}>
      <RootContext value={el}>
        <FeedbackProvider>{children}</FeedbackProvider>
      </RootContext>
    </div>
  );
}
