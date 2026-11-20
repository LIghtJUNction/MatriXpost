# 平台矩阵

当前目录包含 16 个视频平台和 15 个电商平台。三个官方 API 适配器已实现并完成协议 fixture 测试；真实账号发布验收需要各平台的应用权限和账号授权。

“创作页辅助”提供后台入口、文案准备和通用字段填充。它不会自动选择视频文件、填写所有类目规则或点击平台最终提交按钮。各平台页面和具体账号资格变化时，需要重新验证。

| 平台 | 类型 | 当前路径 | 协议测试 | 真实账号发布 / 后台现场 |
| --- | --- | --- | --- | --- |
| YouTube | 视频 | 官方 API；分片上传与已有视频可见性更新 | 协议 fixture 通过 | 未验收；需授权测试账号 |
| 哔哩哔哩 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| 抖音 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| TikTok | 视频 | 创作页辅助；Direct Post 待接入 | 无自动提交适配器 | 未验收 |
| 小红书 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| 快手 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| 微信视频号 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| Instagram | 视频 | 创作页辅助；专业账号 API 待接入 | 无自动提交适配器 | 未验收 |
| Facebook | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| X | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| LinkedIn | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| Vimeo | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| Dailymotion | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| 今日头条 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| 百家号 | 视频 | 创作页辅助 | 无自动提交适配器 | 未验收 |
| Twitch | 视频 | 创作者后台；上传能力受账号资格限制 | 无自动提交适配器 | 未验收 |
| WooCommerce | 商品 | 官方 REST v3；简单商品、库存、草稿、发布 | 协议 fixture 通过 | 未验收；需授权测试账号 |
| Shopify | 商品 | 官方 GraphQL；单规格商品、库存、渠道发布 | 协议 fixture 通过 | 未验收；需授权测试账号 |
| 淘宝 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| 天猫 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| 京东 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| 拼多多 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| 抖店 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| 快手小店 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| 小红书商家 | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| Amazon | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| eBay | 商品 | 商家后台辅助；Inventory API 待接入 | 无自动提交适配器 | 未验收 |
| Etsy | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |
| Shopee | 商品 | 商家后台辅助；所属市场后台需单独核实 | 无自动提交适配器 | 未验收 |
| Lazada | 商品 | 商家后台辅助；所属市场后台需单独核实 | 无自动提交适配器 | 未验收 |
| TikTok Shop | 商品 | 商家后台辅助 | 无自动提交适配器 | 未验收 |

## 0.6.0 现场覆盖

本轮使用真实 Chromium 扩展验收工作区、账号录入、发布检查、队列恢复、MCP 和数据保留；外网发布被阻断。哔哩哔哩页面账号与 WooCommerce 账号使用本地 fixture，账号标签、加密和 UI 流程通过，未登录其真实后台。上表逐项保留真实账号与后台现场状态。

Shopify 协议和核心回归覆盖已保存远端 ID 的续传，以及取消后再次入队复用原对象。YouTube 上传会话仍先核对已接收字节；三个适配器对创建结果未知的任务保留“结果待核对”。

## API 账号准备

### YouTube

填入使用者自行取得的 OAuth access token。新视频上传需要 `youtube.upload`；把已上传的私密视频改为公开，需要 `videos.update` 接受的 `youtube.force-ssl`、`youtube` 或 `youtubepartner` 权限。更新前会读取当前视频状态并保留相关可写标记。默认上传私密，公开或不公开需要在草稿中明确选择。

本版保存授权凭证但不执行 Google 应用注册、OAuth 登录或自动刷新。平台账号 ID 为用户填写的标签信息；本版没有对上传 token 执行频道身份核验。token 更新应保持同一授权身份。

### WooCommerce

店铺应使用公开 HTTPS 域名。填写 WordPress 站点基地址，例如 `https://shop.example.com`，或带 WordPress 子目录的基地址。Consumer Key / Secret 需要 Read/Write 权限，因为创建前会查询店铺币种。价格字段不会触发汇率换算，草稿币种需要与店铺一致。

本版创建 `simple` 商品，写入名称、描述、价格、SKU、库存和公开图片地址。多规格商品、税类与物流配置需要后续实现。商品描述按纯文本转为安全 HTML。

### Shopify

填写精确的店铺地址 `https://your-store.myshopify.com`，使用 Admin API access token。创建与规格更新需要商品权限；库存需要库存读写权限和实际 Location ID；上架需要渠道权限和 Publication ID。启用库存同步时，本版会按平台要求使用幂等键和库存比较字段。

产品先创建为 DRAFT，再写规格价格与 SKU。选择正式上架时还会启用商品、发布到指定渠道，并查询渠道可见性。商品创建成功但渠道发布失败时，任务会保留已创建商品 ID，便于核对和恢复。

## 扩展接入优先级

后续直连优先覆盖 TikTok、Instagram 和 eBay，再按实际开发者应用资格接入国内视频与电商接口。TikTok Direct Post、B 站开放平台、抖音代发布及国内电商接口均有各自的应用权限和用户授权条件；复杂商品还需要类目规则、SKU、运费与资质模型。

## 官方资料

- [Chrome 扩展跨域请求与权限](https://developer.chrome.com/docs/extensions/develop/concepts/network-requests)
- [Chrome storage.session](https://developer.chrome.com/docs/extensions/reference/api/storage)
- [YouTube videos.insert](https://developers.google.com/youtube/data/reference/rest/v3/videos/insert)
- [YouTube resumable upload](https://developers.google.com/youtube/data/guides/using_resumable_upload_protocol)
- [YouTube videos.update](https://developers.google.com/youtube/data/reference/rest/v3/videos/update)
- [WooCommerce REST API](https://woocommerce.github.io/woocommerce-rest-api-docs/)
- [Shopify productCreate](https://shopify.dev/docs/api/admin-graphql/latest/mutations/productCreate)
- [Shopify publishablePublish](https://shopify.dev/docs/api/admin-graphql/latest/mutations/publishablePublish)
- [Shopify InventoryItemInput](https://shopify.dev/docs/api/admin-graphql/latest/input-objects/InventoryItemInput)
- [Shopify inventorySetQuantities](https://shopify.dev/docs/api/admin-graphql/latest/mutations/inventorySetQuantities)
- [TikTok Direct Post](https://developers.tiktok.com/docs/en/content-posting-api-reference-direct-post)
- [Instagram 内容发布](https://developers.facebook.com/documentation/instagram-platform/content-publishing)
- [eBay 库存到刊登](https://developer.ebay.com/api-docs/sell/static/inventory/inventory-item-to-offer.html)
- [哔哩哔哩开放平台](https://open.bilibili.com/doc/)
- [抖音视频创建](https://developer.open-douyin.com/docs/resource/zh-CN/dop/develop/openapi/video-management/douyin/create-video/video-create)
