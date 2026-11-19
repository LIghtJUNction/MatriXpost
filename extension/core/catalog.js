/** A directory is not a claim of automated publication support. */
const portal = (id, name, kind, home, composeUrl, description) => ({
  id, name, kind, mode: 'assisted', home, composeUrl,
  description: description || '打开官方发布后台；素材和文案在 MatriXpost 准备，发布由你在平台完成。',
  limits: {}, credentialFields: [], hosts: [],
});

export const PLATFORMS = Object.freeze([
  {
    id: 'youtube', name: 'YouTube', kind: 'video', mode: 'api',
    home: 'https://www.youtube.com', composeUrl: 'https://studio.youtube.com',
    description: '官方 API 分片上传；需要独立 OAuth access token，上传完成后仍可能由平台处理。',
    limits: { title: 100, description: 5000, tags: 500 },
    credentialFields: [{ key: 'accessToken', label: 'OAuth access token', type: 'password', placeholder: '需要 youtube.upload 授权' }],
    hosts: ['https://www.googleapis.com/*'],
  },
  portal('bilibili', '哔哩哔哩', 'video', 'https://www.bilibili.com', 'https://member.bilibili.com/platform/upload/video/frame'),
  portal('douyin', '抖音', 'video', 'https://www.douyin.com', 'https://creator.douyin.com/creator-micro/content/upload'),
  portal('tiktok', 'TikTok', 'video', 'https://www.tiktok.com', 'https://www.tiktok.com/tiktokstudio/upload', '官方创作后台辅助；Direct Post API 需要应用审核和创作者授权，当前未接入。'),
  portal('xiaohongshu', '小红书', 'video', 'https://www.xiaohongshu.com', 'https://creator.xiaohongshu.com/publish/publish'),
  portal('kuaishou', '快手', 'video', 'https://www.kuaishou.com', 'https://cp.kuaishou.com/article/publish/video'),
  portal('wechat_channels', '微信视频号', 'video', 'https://channels.weixin.qq.com', 'https://channels.weixin.qq.com/platform'),
  portal('instagram', 'Instagram', 'video', 'https://www.instagram.com', 'https://www.instagram.com', '官方页面辅助；专业账号发布 API 尚未接入。'),
  portal('facebook', 'Facebook', 'video', 'https://www.facebook.com', 'https://business.facebook.com/latest/home'),
  portal('x', 'X', 'video', 'https://x.com', 'https://x.com/compose/post'),
  portal('linkedin', 'LinkedIn', 'video', 'https://www.linkedin.com', 'https://www.linkedin.com/feed/'),
  portal('vimeo', 'Vimeo', 'video', 'https://vimeo.com', 'https://vimeo.com/upload'),
  portal('dailymotion', 'Dailymotion', 'video', 'https://www.dailymotion.com', 'https://studio.dailymotion.com'),
  portal('toutiao', '今日头条', 'video', 'https://www.toutiao.com', 'https://mp.toutiao.com'),
  portal('baijiahao', '百家号', 'video', 'https://baijiahao.baidu.com', 'https://baijiahao.baidu.com/builder/rc/home'),
  portal('twitch', 'Twitch', 'video', 'https://www.twitch.tv', 'https://dashboard.twitch.tv', '打开创作者后台；上传权限和内容类型由 Twitch 账号资格决定。'),
  {
    id: 'woocommerce', name: 'WooCommerce', kind: 'product', mode: 'api',
    home: 'https://woocommerce.com', composeUrl: null,
    description: '官方 REST v3 创建简单商品、草稿与上架；独立店铺 Consumer Key / Secret。',
    limits: {},
    credentialFields: [
      { key: 'consumerKey', label: 'Consumer key', type: 'password', placeholder: 'ck_…' },
      { key: 'consumerSecret', label: 'Consumer secret', type: 'password', placeholder: 'cs_…' },
    ], hosts: [],
  },
  {
    id: 'shopify', name: 'Shopify', kind: 'product', mode: 'api',
    home: 'https://www.shopify.com', composeUrl: 'https://admin.shopify.com',
    description: '官方 GraphQL API 创建单规格商品、设置价格/SKU，可配置库存和发布渠道。',
    limits: {},
    credentialFields: [{ key: 'accessToken', label: 'Admin API access token', type: 'password', placeholder: '需要 write_products；上架另需 write_publications' }],
    hosts: [],
  },
  portal('taobao', '淘宝', 'product', 'https://www.taobao.com', 'https://myseller.taobao.com'),
  portal('tmall', '天猫', 'product', 'https://www.tmall.com', 'https://myseller.taobao.com'),
  portal('jd', '京东', 'product', 'https://www.jd.com', 'https://shop.jd.com'),
  portal('pinduoduo', '拼多多', 'product', 'https://www.pinduoduo.com', 'https://mms.pinduoduo.com'),
  portal('douyin_shop', '抖店', 'product', 'https://fxg.jinritemai.com', 'https://fxg.jinritemai.com'),
  portal('kuaishou_shop', '快手小店', 'product', 'https://s.kwaixiaodian.com', 'https://s.kwaixiaodian.com'),
  portal('xiaohongshu_shop', '小红书商家', 'product', 'https://ark.xiaohongshu.com', 'https://ark.xiaohongshu.com'),
  portal('amazon', 'Amazon', 'product', 'https://www.amazon.com', 'https://sellercentral.amazon.com'),
  portal('ebay', 'eBay', 'product', 'https://www.ebay.com', 'https://www.ebay.com/sh/ovw', '官方卖家后台辅助；Inventory API 的库存、offer、政策和刊登流程尚未接入。'),
  portal('etsy', 'Etsy', 'product', 'https://www.etsy.com', 'https://www.etsy.com/your/shops/me/dashboard'),
  portal('shopee', 'Shopee', 'product', 'https://shopee.com', 'https://seller.shopee.com', '打开卖家后台；请使用所属市场的卖家中心账号。'),
  portal('lazada', 'Lazada', 'product', 'https://www.lazada.com', 'https://sellercenter.lazada.com'),
  portal('tiktok_shop', 'TikTok Shop', 'product', 'https://seller.tiktokglobalshop.com', 'https://seller.tiktokglobalshop.com'),
]);

export function getPlatform(id) {
  return PLATFORMS.find((platform) => platform.id === id) || null;
}
