// How lists behave (docs/ui.md#设置): done tasks kept, where quick add puts tasks, and the keys that send.

import { useState } from "react";
import { phoneApp } from "../api/http";
import { useView } from "../session";
import { Action, Seg, Stepper } from "../ui/controls";
import { useEnterSends } from "../ui/keys";
import { fixAddTo, unfixAddTo, useAddTo, useFixedAddTo } from "./add-to";
import { MAX_KEEP_DONE, setKeepDone, useKeepDone } from "./keep-done";
import { FolderPicker } from "./pickers";

/** How the lists keep what is done, where quick add puts a task, and which keys send AI organizing's
 * text. */
export function Lists() {
  const keepDone = useKeepDone();
  return (
    <>
      <h2 className="group-title">已完成的事项</h2>
      <div className="group settings">
        <div className="row">
          <div className="row-inner">
            <span className="folder-main">
              <span>保留最近完成的</span>
              <span className="folder-sub full">在文件夹里划掉后留在原处，更早的收进“已完成”</span>
            </span>
            <Stepper label="保留最近完成的事项数" value={keepDone} min={0} max={MAX_KEEP_DONE} onChange={setKeepDone} />
          </div>
        </div>
      </div>
      <h2 className="group-title">快速添加</h2>
      <AddTo />
      {!phoneApp && <SendKeys />}
    </>
  );
}

/** Which keys send AI organizing's text, of many lines (docs/ui.md#设置). */
function SendKeys() {
  const [enterSends, setEnterSends] = useEnterSends();
  const mod = /Mac|iPhone|iPad/.test(navigator.userAgent) ? "⌘ + Enter" : "Ctrl + Enter";
  return (
    <>
      <h2 className="group-title">AI 整理</h2>
      <div className="group">
        <div className="row">
          <div className="row-inner controls">
            发送键
            <span className="row-value">
              <Seg<"mod" | "enter">
                small
                label="AI 整理的发送键"
                items={[
                  ["mod", mod],
                  ["enter", "Enter"],
                ]}
                value={enterSends ? "enter" : "mod"}
                onChange={(to) => setEnterSends(to === "enter")}
              />
            </span>
          </div>
        </div>
      </div>
      <p className="group-note">{enterSends ? "Enter 发送，Shift + Enter 换行" : `Enter 换行，${mod} 发送`}</p>
    </>
  );
}

/** What a fixed folder for quick add shows as once deleted or no longer shared. */
const GONE = "已不在";

/** Which folder quick add puts a task in, outside a folder (docs/ui.md#其他). Fixing one picks it
 * first: without a pick, it stays as it was. */
function AddTo() {
  const { board } = useView();
  const fixed = useFixedAddTo(board.me);
  const shown = useAddTo(board);
  const [picking, setPicking] = useState(false);
  const gone = fixed !== undefined && !board.hasFolder(fixed);
  return (
    <>
      <div className="group">
        <div className="row">
          <div className="row-inner controls">
            添加到
            <span className="row-value">
              <Seg<"last" | "fixed">
                small
                label="快速添加到"
                items={[
                  ["last", "上次选的"],
                  ["fixed", "固定的"],
                ]}
                value={fixed === undefined ? "last" : "fixed"}
                onChange={(to) => (to === "last" ? unfixAddTo(board.me, shown) : setPicking(true))}
              />
            </span>
          </div>
        </div>
        {fixed !== undefined && <Action label="文件夹" value={gone ? GONE : board.folderName(fixed)} onClick={() => setPicking(true)} />}
      </div>
      <p className="group-note">
        {fixed === undefined
          ? "“接下来”和日历的快速添加栏使用上次选择的文件夹。"
          : gone
            ? "该文件夹已删除或不再共享，恢复前改用默认位置，也可另选一个。"
            : "“接下来”和日历的快速添加栏总是使用该文件夹，在栏中临时选择的只用一次。"}
      </p>
      {picking && <FolderPicker mode="add" current={fixed} onPick={(f) => fixAddTo(board.me, f)} onClose={() => setPicking(false)} />}
    </>
  );
}
