/** Whether a key goes to an input method still composing, as Enter confirming the letters typed. WebKit,
 * Safari's and the macOS app's, ends the composition before that Enter comes, so `isComposing` is
 * false then; its key code, 229, still tells. */
export const composing = (e: React.KeyboardEvent) => e.nativeEvent.isComposing || e.keyCode === 229;
