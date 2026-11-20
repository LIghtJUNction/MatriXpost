# Project Z 与运行依赖

登记日期：2026-11-20。范围：MatriXpost 0.6.0 当前源码与构建配置。

## 当前关系

MatriXpost 没有对 Project Z 的代码、服务或包依赖。当前 `package.json` 未声明运行时依赖；扩展和本机桥接也没有导入 Project Z。此次登记没有新增依赖。

| 部分 | 实际依赖 | 证据位置 |
| --- | --- | --- |
| 扩展工作区与队列 | 原生 ES modules、Chromium MV3 API、IndexedDB、Web Crypto | `extension/manifest.json`、`extension/core/`、`extension/background.js` |
| 官方 API 适配器 | 平台 HTTPS API、扩展获得的站点权限 | `extension/adapters/`、`extension/core/catalog.js` |
| 本机 AI / MCP 桥接 | Node.js 内置模块、标准输入输出、本机 HTTP | `bridge/` |
| 检查、单元测试、构建 | Node.js 内置模块和测试运行器；不需要 `npm install` | `package.json`、`scripts/`、`tests/` |
| 真实浏览器验收 | 开发环境提供的 Playwright 和 Chromium；原生权限场景使用 Xvfb / D-Bus | `scripts/browser-test.mjs` |
| 旧版本升级验收 | Git 历史、独立 Chromium 配置、开发环境的 Playwright | `scripts/upgrade-test.mjs` |
| CI / Release | GitHub Actions 的 checkout、setup-node、upload-artifact；Release 使用 GitHub CLI | `.github/workflows/` |

浏览器验收用的工具不打入扩展或桥接安装包。GitHub Actions 属于构建与交付基础设施，也不成为产品运行依赖。

## 后续登记规则

如果以后从 Project Z 导入模块、调用其服务，或让其参与产品构建，再在本文件登记具体入口、版本、用途和失效影响。单纯同属一个项目计划、复用设计想法或开发环境中同时存在，不构成代码依赖。
