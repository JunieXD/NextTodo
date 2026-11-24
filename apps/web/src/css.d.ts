import "react";

declare module "react" {
  /** Custom properties, `--name`, as a `style` sets them. */
  interface CSSProperties {
    [name: `--${string}`]: string | number | undefined;
  }
}
