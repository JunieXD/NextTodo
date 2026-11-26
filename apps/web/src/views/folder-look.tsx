// A folder's colour, a preset or one of the user's own, and its icon, a preset, shown on its icon alone
// (docs/ui.md#文件夹的颜色和图标); and the dialog that chooses them.

import {
  Baby,
  Bike,
  BookOpen,
  Briefcase,
  Calculator,
  Camera,
  Car,
  Code,
  Coffee,
  Dumbbell,
  FileText,
  Flag,
  FlaskConical,
  Folder as FolderDefault,
  FolderRoot,
  FolderUsers,
  Gamepad2,
  Gift,
  GraduationCap,
  Heart,
  HeartPulse,
  House,
  Laptop,
  Leaf,
  Lightbulb,
  type LucideIcon,
  MapPin,
  Music,
  Palette,
  PawPrint,
  PenLine,
  PiggyBank,
  Pill,
  Plane,
  Rocket,
  Shirt,
  ShoppingCart,
  Star,
  Target,
  Trophy,
  Users,
  Utensils,
  Wallet,
  Wrench,
} from "lucide-react";
import { type CSSProperties, useState } from "react";
import type { ColorPreset } from "../api/types/ColorPreset";
import type { Folder } from "../api/types/Folder";
import type { FolderColor } from "../api/types/FolderColor";
import type { FolderIcon } from "../api/types/FolderIcon";
import type { FolderId } from "../api/types/FolderId";
import type { Board } from "../domain/board";
import { type Hex, parseHex, roles } from "../domain/color";
import { useCommit, useUndo } from "../session";
import { Modal, useFeedback } from "../ui/feedback";
import { ColorPicker } from "../ui/color-picker";
import { Swatch } from "../ui/controls";
import { Icon } from "../ui/icons";

/** Where a colour of one's own starts from, when the folder has none to start from. */
const BLUE: Hex = "#2071f3";

/** In the order the dialog lists them. */
const PRESETS: [ColorPreset, string, Hex][] = [
  ["red", "红", "#f52027"],
  ["orange", "橙", "#fb7c05"],
  ["amber", "琥珀", "#f1a201"],
  ["yellow", "黄", "#e5d513"],
  ["lime", "草绿", "#96d507"],
  ["green", "绿", "#05b047"],
  ["teal", "青绿", "#0eb09e"],
  ["cyan", "青", "#03b5e4"],
  ["blue", "蓝", BLUE],
  ["indigo", "靛蓝", "#5e44e6"],
  ["purple", "紫", "#a23de4"],
  ["pink", "粉", "#f43d99"],
  ["brown", "棕", "#8c5c3a"],
  ["gray", "灰", "#868686"],
];

const PRESET_HEX = new Map<string, Hex>(PRESETS.map(([key, , hex]) => [key, hex]));

/** The colour `color` is, as `#rrggbb`: none for a preset of a newer version than this. */
const hexOfColor = (color: string) => PRESET_HEX.get(color) ?? (color.startsWith("#") ? parseHex(color) : null);

/** The user's own colour, if `color` is one. */
const ownColor = (color: FolderColor | null) => (color?.startsWith("#") ? parseHex(color) : null);

const ICONS: [FolderIcon, LucideIcon, string][] = [
  ["briefcase", Briefcase, "公文包"],
  ["graduation-cap", GraduationCap, "学士帽"],
  ["book", BookOpen, "书"],
  ["pen", PenLine, "笔"],
  ["file", FileText, "文档"],
  ["laptop", Laptop, "笔记本电脑"],
  ["code", Code, "代码"],
  ["flask", FlaskConical, "烧瓶"],
  ["calculator", Calculator, "计算器"],
  ["lightbulb", Lightbulb, "灯泡"],
  ["target", Target, "靶心"],
  ["house", House, "房子"],
  ["cart", ShoppingCart, "购物车"],
  ["utensils", Utensils, "餐具"],
  ["coffee", Coffee, "咖啡"],
  ["shirt", Shirt, "衣服"],
  ["baby", Baby, "婴儿"],
  ["paw", PawPrint, "爪印"],
  ["heart", Heart, "心"],
  ["gift", Gift, "礼物"],
  ["dumbbell", Dumbbell, "哑铃"],
  ["bike", Bike, "自行车"],
  ["heart-pulse", HeartPulse, "心跳"],
  ["pill", Pill, "药丸"],
  ["wallet", Wallet, "钱包"],
  ["piggy-bank", PiggyBank, "存钱罐"],
  ["plane", Plane, "飞机"],
  ["car", Car, "汽车"],
  ["map-pin", MapPin, "地点"],
  ["music", Music, "音乐"],
  ["camera", Camera, "相机"],
  ["palette", Palette, "调色板"],
  ["gamepad", Gamepad2, "游戏手柄"],
  ["star", Star, "星"],
  ["flag", Flag, "旗"],
  ["rocket", Rocket, "火箭"],
  ["trophy", Trophy, "奖杯"],
  ["wrench", Wrench, "扳手"],
  ["leaf", Leaf, "叶子"],
  ["users", Users, "人群"],
];

const GLYPHS = new Map(ICONS.map(([key, icon]) => [key, icon]));

/** The icon of a folder that looks as `look` says, `shared` with others or not. */
const glyph = (look: Pick<Folder, "icon">, shared: boolean): LucideIcon =>
  (look.icon && GLYPHS.get(look.icon)) || (shared ? FolderUsers : FolderDefault);

/** What gives an element of `className` the colour `hex` as `--fc-*`, its look as is for none. */
function tint(className: string, hex: Hex | null): { className: string; style?: CSSProperties } {
  if (!hex) return { className };
  const r = roles(hex);
  return { className: `${className} fc`, style: { "--fc-light": r.light, "--fc-dark": r.dark } };
}

/** What gives an element of `className` folder `f`'s colour. */
const folderTint = (board: Board, f: FolderId | null, className: string) => {
  const color = f ? board.folder(f)?.color : null;
  return tint(className, color ? hexOfColor(color) : null);
};

/** The icon folder `f` shows: the one chosen, else a folder, with two people when it is shared; the
 * root's own. */
const folderIcon = (board: Board, f: FolderId | null): LucideIcon => {
  if (!f) return FolderRoot;
  const folder = board.folder(f);
  return folder ? glyph(folder, board.sharedRoot(f) === f) : FolderDefault;
};

/** Whether folder `f` has a colour or icon chosen. */
export const styled = (board: Board, f: FolderId | null): boolean => {
  const folder = f ? board.folder(f) : undefined;
  return !!((folder?.color && hexOfColor(folder.color)) || folder?.icon);
};

/** Folder `f`'s icon in its colour, or the root's. */
export function FolderGlyph({ board, id, size = 18, className = "" }: { board: Board; id: FolderId | null; size?: number; className?: string }) {
  return (
    <span {...folderTint(board, id, `folder-glyph${className && ` ${className}`}`)}>
      <Icon of={folderIcon(board, id)} size={size} />
    </span>
  );
}

/** A colour or an icon a folder had, by its code, drawn and named, as the change log shows it: one of
 * the user's own colours by its `#rrggbb`. A newer server's that this version doesn't know shows by its
 * code. */
export function LookValue({ field, code }: { field: "color" | "icon"; code: string }) {
  if (field === "color") {
    const hex = hexOfColor(code);
    return (
      <span className="look-value">
        {hex && <span {...tint("look-dot", hex)} />}
        {PRESETS.find(([key]) => key === code)?.[1] ?? code}
      </span>
    );
  }
  const found = ICONS.find(([key]) => key === code);
  return (
    <span className="look-value">
      {found && <Icon of={found[1]} size={16} />}
      {found?.[2] ?? code}
    </span>
  );
}

/** The dialog choosing folder `f`'s colour and icon: one change, which can be undone. */
export function FolderLookDialog({ board, folder: f, onClose }: { board: Board; folder: Folder; onClose(): void }) {
  const commit = useCommit();
  const undo = useUndo();
  const { snack } = useFeedback();
  // Only what the user picks: the rest stays as it is by then, changed meanwhile on another device, say.
  const [picked, setPicked] = useState<{ color?: FolderColor | null; icon?: FolderIcon | null }>({});
  const color = picked.color === undefined ? f.color : picked.color;
  const icon = picked.icon === undefined ? f.icon : picked.icon;
  // One of a newer version than this shows as the default, as it does elsewhere.
  const hex = color && hexOfColor(color);
  const own = ownColor(color);
  const [picking, setPicking] = useState(false);
  const setColor = (c: FolderColor | null) => setPicked((p) => ({ ...p, color: c }));
  const setIcon = (i: FolderIcon | null) => setPicked((p) => ({ ...p, icon: i }));
  const shared = board.sharedRoot(f.id) === f.id;
  const save = () => {
    onClose();
    if (color === f.color && icon === f.icon) return;
    const r = commit([{ op: "folder.update", id: f.id, ...(color !== f.color && { color }), ...(icon !== f.icon && { icon }) }]);
    if (r) snack({ text: `已改“${f.name}”的颜色和图标`, action: "撤销", onAction: () => undo(r.id) });
  };
  const iconChoice = (value: FolderIcon | null, label: string, face: LucideIcon) => (
    <button key={String(value)} type="button" className="look-icon" aria-label={label} aria-pressed={value === icon} title={label} onClick={() => setIcon(value)}>
      <Icon of={face} />
    </button>
  );
  return (
    <Modal
      title="颜色和图标"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="text-btn" onClick={onClose}>
            取消
          </button>
          <button type="button" className="filled-btn" onClick={save}>
            保存
          </button>
        </>
      }
    >
      <div className="look-preview">
        <span {...tint("folder-glyph", hex)}>
          <Icon of={glyph({ icon }, shared)} size={28} />
        </span>
        <span className="truncate">{f.name}</span>
      </div>
      <h2 className="look-title">颜色</h2>
      <div className="color-choices">
        <Swatch label="默认" chosen={!hex} onChoose={() => (setColor(null), setPicking(false))} />
        {PRESETS.map(([key, name, hex]) => (
          <Swatch key={key} label={name} color={hex} on={roles(hex).on} chosen={color === key} onChoose={() => (setColor(key), setPicking(false))} />
        ))}
        <Swatch
          label="自定义"
          color={own ?? undefined}
          on={own ? roles(own).on : undefined}
          rainbow
          chosen={own !== null}
          // From the colour there was, to change a little.
          onChoose={() => (own ? setPicking(!picking) : (setColor(hex || BLUE), setPicking(true)))}
        />
      </div>
      {/* Folded away, it keeps where it was dragged to: black or grey has no hue of its own. */}
      {own && (
        <div hidden={!picking}>
          <ColorPicker value={own} onChange={setColor} />
        </div>
      )}
      <h2 className="look-title">图标</h2>
      <div className="look-icons">
        {iconChoice(null, "默认", shared ? FolderUsers : FolderDefault)}
        {ICONS.map(([key, face, name]) => iconChoice(key, name, face))}
      </div>
    </Modal>
  );
}
