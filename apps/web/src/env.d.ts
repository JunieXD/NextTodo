interface ImportMetaEnv {
  /** The version of the release the web app is built for: see vite.config.ts. */
  readonly WEB_VERSION: string;
  /** Built otherwise than by deploy/build.sh: its version is package.json's, which says nothing. */
  readonly WEB_DEV: boolean;
}
