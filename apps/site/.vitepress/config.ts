// The website at juniexd.github.io/NextTodo (docs/deploy.md#官网).

import { defineConfig } from "vitepress";

const releases = "https://github.com/JunieXD/NextTodo/releases/latest";

export default defineConfig({
  lang: "zh-CN",
  title: "NextTodo",
  description: "以先后关系为核心的待办应用，可在本机使用，也可自行部署、多端同步",
  base: "/NextTodo/",
  cleanUrls: true,
  head: [["link", { rel: "icon", type: "image/svg+xml", href: "/NextTodo/icon.svg" }]],
  themeConfig: {
    logo: "/icon.svg",
    nav: [
      { text: "快速上手", link: "/guide/" },
      { text: "下载", link: releases },
    ],
    sidebar: [
      {
        text: "开始",
        items: [
          { text: "快速上手", link: "/guide/" },
          { text: "下载和安装", link: "/guide/install" },
          { text: "基本用法", link: "/guide/basics" },
        ],
      },
      {
        text: "进阶",
        items: [
          { text: "AI 整理", link: "/guide/ai" },
          { text: "接入 agent", link: "/guide/agent" },
        ],
      },
      {
        text: "数据",
        items: [
          { text: "在本机使用", link: "/guide/local" },
          { text: "部署服务器", link: "/guide/server" },
        ],
      },
    ],
    socialLinks: [{ icon: "github", link: "https://github.com/JunieXD/NextTodo" }],
    search: {
      provider: "local",
      options: {
        // Chinese has no spaces between words: split it into words as the browser does.
        miniSearch: {
          options: {
            tokenize: (text: string) =>
              [...new Intl.Segmenter("zh", { granularity: "word" }).segment(text)].filter((s) => s.isWordLike).map((s) => s.segment),
          },
        },
        translations: {
          button: { buttonText: "搜索", buttonAriaLabel: "搜索" },
          modal: {
            displayDetails: "显示详细列表",
            resetButtonTitle: "清除",
            backButtonTitle: "关闭",
            noResultsText: "没有找到",
            footer: { selectText: "选择", navigateText: "切换", closeText: "关闭" },
          },
        },
      },
    },
    outline: { level: [2, 3], label: "本页内容" },
    docFooter: { prev: "上一篇", next: "下一篇" },
    darkModeSwitchLabel: "外观",
    lightModeSwitchTitle: "切换到浅色",
    darkModeSwitchTitle: "切换到深色",
    sidebarMenuLabel: "目录",
    returnToTopLabel: "回到顶部",
    notFound: { title: "页面不存在", quote: "这个地址没有内容，可能已经移走了。", linkText: "回到首页" },
    footer: { message: "以 MIT 许可证开源" },
  },
});
